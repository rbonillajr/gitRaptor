//! US-TMC-004 end to end: what is done outside GitRaptor (the editor, raw
//! Git) is captured as a recoverable point, and raw Git is in the stack of
//! `timemachine.undo`. A real daemon (in-process) observing a testkit fixture
//! (temporary repo, worktrees and home) with a separate temporary profile;
//! never this repo or the real profile (NFR-01). The continuous capture runs
//! with a short quiet time; no test waits a fixed time: each step waits for
//! a state, with a deadline.
//!
//! In-process clients and the test's own `git` resolve as "unattributed",
//! so the developer undoes unattributed raw Git. The agent's case, with a
//! simulated Claude Code, is the end-to-end test of the CLI
//! (`apps/cli/tests/raw_git_undo.rs`).
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{TmRejectReason, TmRejectedData, UndoResult};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LOG_FILE, LogLimits, ShutdownHandle, StopCause, StopReport,
    TmCapture,
};
use gitraptor_core::timemachine::continuous::{CaptureConfig, CaptureLayer};
use gitraptor_core::timemachine::oplog::{
    OpRef, Oplog, SnapshotFilter, SnapshotLevel, SnapshotState, SnapshotView, Target,
};
use gitraptor_core::timemachine::store::{CaptureError, SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(20);

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

struct Running {
    fx: Fixture,
    tp: TempProfile,
    repo_id: String,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

fn connect(tp: &TempProfile) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Observes `fx`'s repo in a fresh profile and starts the daemon with a
/// short quiet time; `layer` makes every continuous capture fail.
fn start(fx: Fixture, layer: Option<CaptureLayer>) -> Running {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: true,
                quiet: Duration::from_millis(150),
                max_interval: Duration::from_millis(600),
            },
            layer,
            no_free_space_floor: true,
        },
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let r = Running {
        fx,
        tp,
        repo_id: entry.repo_id,
        handle,
        join: Some(join),
    };
    // Serving: the channel answers.
    drop(connect(&r.tp));
    r
}

impl Running {
    fn undo(&self, worktree: &Path) -> Result<Value, ClientError> {
        connect(&self.tp).call(
            methods::TM_UNDO,
            json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
        )
    }

    fn undo_ok(&self, worktree: &Path) -> UndoResult {
        serde_json::from_value(self.undo(worktree).unwrap()).unwrap()
    }

    fn store(&self) -> Option<SnapshotStore> {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .ok()
            .flatten()
    }

    /// `path → bytes` of worktree `key` in snapshot `id`.
    fn files(&self, store: &SnapshotStore, id: &str, key: &str) -> Vec<(String, Vec<u8>)> {
        store
            .files(id, key)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
            .map(|(p, _, oid)| (p, store.read_blob(oid).unwrap()))
            .collect()
    }

    /// Waits for a snapshot of worktree `key` whose files satisfy `ok`;
    /// its id. Snapshots are read from the store: a ref exists only once
    /// its oplog row is written (ADR-TMC-001 § 4).
    fn snapshot_when(&self, key: &str, ok: impl Fn(&[(String, Vec<u8>)]) -> bool) -> String {
        let start = Instant::now();
        loop {
            if let Some(store) = self.store() {
                let refs = snapshot_refs(&store).unwrap_or_default();
                for id in refs.keys() {
                    if ok(&self.files(&store, id, key)) {
                        return id.clone();
                    }
                }
            }
            if start.elapsed() >= DEADLINE {
                let seen: Vec<String> = self
                    .store()
                    .map(|store| {
                        snapshot_refs(&store)
                            .unwrap_or_default()
                            .keys()
                            .map(|id| {
                                let meta = store.meta(id).map(|m| {
                                    m.worktrees
                                        .iter()
                                        .map(|w| w.key.clone())
                                        .collect::<Vec<_>>()
                                });
                                let files: Vec<String> = self
                                    .files(&store, id, key)
                                    .into_iter()
                                    .map(|(p, _)| p)
                                    .collect();
                                format!("{id}: {meta:?} {files:?}")
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                panic!(
                    "no snapshot of {key} as expected: {seen:#?}\nlog:\n{}",
                    self.log()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.tp.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// Waits until the daemon log has `event`.
    fn logged(&self, event: &str) {
        let log = self.tp.dirs().state.join(LOG_FILE);
        let start = Instant::now();
        while !std::fs::read_to_string(&log).is_ok_and(|t| t.contains(event)) {
            assert!(start.elapsed() < DEADLINE, "{event} never logged");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stops the daemon and opens the repo's oplog.
    fn stop_and_oplog(mut self) -> (Fixture, Oplog) {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap();
        let oplog = Oplog::open(&self.tp.dirs(), &self.repo_id, 2).unwrap().0;
        let fx = std::mem::replace(&mut self.fx, Fixture::new(&git()));
        (fx, oplog)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn reject_reason(result: Result<Value, ClientError>) -> TmRejectedData {
    match result {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, code::OPERATION_REJECTED, "{e:?}");
            serde_json::from_value(e.data.unwrap()).unwrap()
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn has(files: &[(String, Vec<u8>)], path: &str, content: &[u8]) -> bool {
    files.iter().any(|(p, b)| p == path && b == content)
}

fn lacks(files: &[(String, Vec<u8>)], path: &str) -> bool {
    !files.iter().any(|(p, _)| p == path)
}

fn snapshot(oplog: &Oplog, id: &str) -> SnapshotView {
    oplog.snapshot(id).unwrap().unwrap()
}

/// A repo with `api.rs` and `.gitignore` (`.env`) committed, and a linked
/// worktree "feat-login" on its own branch. The worktree's key in a
/// snapshot is `wt-wt-feat-login` (its folder is `wt-feat-login`).
fn repo_with_login() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("api.rs", "fn api() {}\n");
    fx.write(".gitignore", ".env\n");
    fx.git(&["add", "api.rs", ".gitignore"]);
    fx.git(&["commit", "-q", "-m", "api"]);
    fx.git(&["branch", "feat-login"]);
    let wt = fx.add_worktree("feat-login", "feat-login");
    (fx, canonical(&wt))
}

const KEY: &str = "wt-wt-feat-login";
const EDITED: &[u8] = b"fn api() { login(); }\n";

// ----- Scenarios ----------------------------------------------------------------

/// Escenario: Una edición fuera de GitRaptor queda capturada.
#[test]
fn an_edit_outside_gitraptor_is_captured() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    // Dado un repo observado sin hooks de Guardrails: the fixture has none.
    // Cuando se modifica "api.rs" y se crea "util.rs" sin seguimiento.
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    std::fs::write(wt.join("util.rs"), "fn util() {}\n").unwrap();
    // Entonces la Time Machine guarda un punto recuperable con ambos.
    let id = r.snapshot_when(KEY, |f| {
        has(f, "api.rs", EDITED) && has(f, "util.rs", b"fn util() {}\n")
    });
    let (_fx, oplog) = r.stop_and_oplog();
    let s = snapshot(&oplog, &id);
    // Y ese punto figura como "capturado por observación".
    assert_eq!(s.record.level, SnapshotLevel::Observation);
    assert_eq!(s.state, SnapshotState::Complete);
    assert!(s.record.cause_operation.is_none());
}

/// Escenario: Un reset destructivo con Git crudo deja recuperable el último
/// estado capturado. Then the undo of the raw reset recovers it, and its
/// own echo in the engine is not raw Git: a second undo has nothing left.
#[test]
fn a_raw_reset_leaves_the_last_capture_restorable() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    std::fs::write(wt.join("util.rs"), "fn util() {}\n").unwrap();
    // Dado que el último estado capturado incluye "api.rs" modificado.
    let captured = r.snapshot_when(KEY, |f| {
        has(f, "api.rs", EDITED) && has(f, "util.rs", b"fn util() {}\n")
    });
    // Cuando un agente ejecuta un reset destructivo con Git crudo.
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), b"fn api() {}\n");

    // The raw reset is the last operation of the worktree; its target, the
    // last capture before it.
    let undo = r.undo_ok(&wt);
    assert!(
        undo.undone_operation_id.starts_with("git-event-"),
        "{undo:?}"
    );
    assert_eq!(
        undo.undone_subtype.as_ref().map(|s| s.sanitized()),
        Some("reset".to_owned())
    );
    assert_eq!(undo.target_snapshot_id, captured);
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), EDITED);
    assert_eq!(
        std::fs::read(wt.join("util.rs")).unwrap(),
        b"fn util() {}\n"
    );

    // The undo's own writes are its echo, not raw Git.
    assert_eq!(
        reject_reason(r.undo(&wt)).reason,
        TmRejectReason::NothingToUndo
    );

    let (_fx, oplog) = r.stop_and_oplog();
    // Entonces el último estado capturado antes del reset sigue disponible
    // para restaurar, y figura como "capturado por observación", no como
    // "snapshot previo".
    let s = snapshot(&oplog, &captured);
    assert!(s.state.is_available());
    assert_eq!(s.record.level, SnapshotLevel::Observation);
    let rec = oplog.operation(&undo.operation_id).unwrap().unwrap();
    let seq: i64 = undo
        .undone_operation_id
        .trim_start_matches("git-event-")
        .parse()
        .unwrap();
    assert_eq!(rec.record.target, Target::Undo(vec![OpRef::GitEvent(seq)]));
    // The anchor of the undo: the state it left, with the undo as cause.
    let anchors = oplog
        .snapshots(&SnapshotFilter {
            operation_id: Some(undo.operation_id.clone()),
            ..SnapshotFilter::default()
        })
        .unwrap();
    assert!(
        anchors
            .iter()
            .any(|a| a.record.level == SnapshotLevel::Observation),
        "{anchors:#?}"
    );
}

/// Escenario: Los archivos ignorados y las credenciales no se capturan.
#[test]
fn ignored_files_and_credentials_are_not_captured() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    // Dado que se modifica ".env", ignorado, y se crea "deploy.pem" sin
    // seguimiento y sin ignorar; un perfil sin opción para incluir
    // credenciales (the temporary profile has no settings).
    std::fs::write(wt.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(wt.join("deploy.pem"), "-----BEGIN KEY-----\n").unwrap();
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    // Cuando la Time Machine captura los cambios de "feat-login".
    let id = r.snapshot_when(KEY, |f| has(f, "api.rs", EDITED));
    let store = r.store().unwrap();
    let files = r.files(&store, &id, KEY);
    // Entonces el punto no contiene ".env" ni "deploy.pem".
    assert!(lacks(&files, ".env"), "{files:?}");
    assert!(lacks(&files, "deploy.pem"), "{files:?}");
    let (_fx, oplog) = r.stop_and_oplog();
    // Y el punto declara "deploy.pem" como excluido por credenciales.
    let s = snapshot(&oplog, &id);
    let exclusions = s.complete.unwrap().exclusions;
    assert!(
        exclusions
            .iter()
            .any(|e| e.path.ends_with("deploy.pem") && e.reason == "credential"),
        "{exclusions:?}"
    );
    assert!(!exclusions.iter().any(|e| e.path.ends_with(".env")));
}

/// Escenario: Un archivo por encima del tope deja la captura parcial.
#[test]
fn a_file_over_the_cap_makes_the_capture_partial() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    let dump = std::fs::File::create(wt.join("dump.bin")).unwrap();
    dump.set_len(51 * 1024 * 1024).unwrap();
    drop(dump);
    let id = r.snapshot_when(KEY, |f| has(f, "api.rs", EDITED));
    let store = r.store().unwrap();
    // Entonces el punto contiene "api.rs" y no contiene "dump.bin".
    assert!(lacks(&r.files(&store, &id, KEY), "dump.bin"));
    let (_fx, oplog) = r.stop_and_oplog();
    // Y el punto figura como captura parcial con "dump.bin" en lo omitido.
    let exclusions = snapshot(&oplog, &id).complete.unwrap().exclusions;
    assert!(
        exclusions
            .iter()
            .any(|e| e.path.ends_with("dump.bin") && e.reason == "too-large"),
        "{exclusions:?}"
    );
}

/// Escenario: Una captura que falla no se presenta como protegida.
#[test]
fn a_failed_capture_is_not_presented_as_protected() {
    let (fx, wt) = repo_with_login();
    // Dado que guardar una captura de "feat-login" falla.
    let fail: CaptureLayer =
        Arc::new(|_| Some(CaptureError::Io(std::io::Error::from_raw_os_error(28))));
    let r = start(fx, Some(fail));
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    r.logged("tm_capture_failed");
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    let before = std::fs::read(wt.join("api.rs")).unwrap();
    // Cuando se consulta el historial: the raw reset has no point before it,
    // and the undo says so instead of restoring anything.
    assert_eq!(
        reject_reason(r.undo(&wt)).reason,
        TmRejectReason::TargetUnavailable
    );
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), before);
    let (_fx, oplog) = r.stop_and_oplog();
    // Entonces ese cambio figura sin punto recuperable, y ningún punto se
    // presenta como protegido para ese cambio.
    let points = oplog.snapshots(&SnapshotFilter::default()).unwrap();
    assert!(
        !points
            .iter()
            .any(|s| s.state == SnapshotState::Complete
                && s.record.worktrees.iter().any(|w| w == KEY)),
        "{points:#?}"
    );
}

// ----- Seeding (D12) --------------------------------------------------------------

/// The first continuous capture of a repo whose store lacks its history waits for the store to
/// be seeded from the repo's packs (ADR-TMC-001 § 3), then captures.
#[test]
fn the_store_is_seeded_from_the_packs_before_the_first_capture() {
    let (fx, wt) = repo_with_login();
    fx.git(&["gc", "-q"]);
    let r = start(fx, None);
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    let id = r.snapshot_when(KEY, |f| has(f, "api.rs", EDITED));
    let log = r.log();
    assert!(log.contains("tm_seeded"), "{log}");
    let packs =
        r.tp.dirs()
            .data
            .join("tm")
            .join(&r.repo_id)
            .join("store.git/objects/pack");
    assert!(
        std::fs::read_dir(&packs)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".pack")),
        "no pack seeded"
    );
    let (_fx, oplog) = r.stop_and_oplog();
    assert_eq!(
        snapshot(&oplog, &id).record.level,
        SnapshotLevel::Observation
    );
}
