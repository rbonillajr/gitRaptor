//! TS-GRP-006 end to end: observation tiers in a real daemon (in-process)
//! over a testkit fixture and a temporary profile; never this repo or the
//! real profile (NFR-01). The threshold is short so the repo sleeps during
//! the test; every step waits for a state (a log line, a snapshot), never a
//! fixed time.
//!
//! The NFR-01 condition of the PO (Q49): dormancy must not reduce the
//! protection. An edit in a dormant repo followed by a raw `reset --hard`
//! leaves the edit recoverable from the Time Machine.
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::timemachine::UndoResult;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LOG_FILE, LogLimits, ShutdownHandle, StopCause, StopReport,
    TierConfig, TmCapture,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use serde_json::json;

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

/// Observes `fx`'s repo in a fresh profile, with tiers on and a short
/// threshold, and the continuous capture with a short quiet time.
fn start(fx: Fixture) -> Running {
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
        tiers: TierConfig {
            dormant_after: Some(Duration::from_millis(800)),
            check_every: Duration::from_millis(50),
        },
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: true,
                quiet: Duration::from_millis(150),
                max_interval: Duration::from_millis(600),
            },
            layer: None,
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
    drop(connect(&r.tp));
    r
}

impl Running {
    fn log(&self) -> String {
        std::fs::read_to_string(self.tp.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// Waits until the daemon log has `event` at least `n` times.
    fn logged(&self, event: &str, n: usize) {
        let start = Instant::now();
        while self.log().matches(event).count() < n {
            assert!(
                start.elapsed() < DEADLINE,
                "{event} not logged {n} times:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn store(&self) -> Option<SnapshotStore> {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .ok()
            .flatten()
    }

    fn files(&self, store: &SnapshotStore, id: &str, key: &str) -> Vec<(String, Vec<u8>)> {
        store
            .files(id, key)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
            .map(|(p, _, oid)| (p, store.read_blob(oid).unwrap()))
            .collect()
    }

    /// Waits for a snapshot of worktree `key` holding `path` with `content`.
    fn snapshot_with(&self, key: &str, path: &str, content: &[u8]) -> String {
        let start = Instant::now();
        loop {
            if let Some(store) = self.store() {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    if self
                        .files(&store, id, key)
                        .iter()
                        .any(|(p, b)| p == path && b == content)
                    {
                        return id.clone();
                    }
                }
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no snapshot of {key} with {path}:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn undo_ok(&self, worktree: &Path) -> UndoResult {
        let value = connect(&self.tp)
            .call(
                methods::TM_UNDO,
                json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
            )
            .unwrap();
        serde_json::from_value(value).unwrap()
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

/// `api.rs` committed, and a linked worktree "feat-login" on its own
/// branch (key `wt-wt-feat-login` in a snapshot).
fn repo_with_login() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("api.rs", "fn api() {}\n");
    fx.git(&["add", "api.rs"]);
    fx.git(&["commit", "-q", "-m", "api"]);
    fx.git(&["branch", "feat-login"]);
    let wt = fx.add_worktree("feat-login", "feat-login");
    (fx, canonical(&wt))
}

const KEY: &str = "wt-wt-feat-login";
const EDITED: &[u8] = b"fn api() { login(); }\n";

/// NFR-01 (Q49): an edit in a dormant repo wakes it through its sentinel,
/// the Time Machine captures it, and after a raw `reset --hard` the undo
/// brings the edit back.
#[test]
fn edit_then_reset_hard_in_a_dormant_repo_is_recoverable() {
    let (fx, wt) = repo_with_login();
    let r = start(fx);
    // Dado un repo dormido.
    r.logged("repo_dormant", 1);
    // Cuando se edita un archivo.
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    // El centinela lo despierta y la Time Machine lo captura.
    r.logged("repo_woken", 1);
    let captured = r.snapshot_with(KEY, "api.rs", EDITED);
    // Y un agente hace un reset destructivo con Git crudo.
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), b"fn api() {}\n");
    // Entonces la edición se recupera.
    let undo = r.undo_ok(&wt);
    assert_eq!(undo.target_snapshot_id, captured, "{}", r.log());
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), EDITED);
}

/// A repo that sleeps and wakes again keeps its events: three commits
/// made while dormant are three events in the history, in order.
#[test]
fn commits_while_dormant_reach_the_history_in_order() {
    let (fx, _wt) = repo_with_login();
    let r = start(fx);
    r.logged("repo_dormant", 1);
    let mut commits = Vec::new();
    for i in 0..3 {
        r.fx.write(&format!("c{i}.txt"), "c\n");
        r.fx.git(&["add", "."]);
        r.fx.git(&["commit", "-q", "-m", &format!("c{i}")]);
        commits.push(r.fx.git(&["rev-parse", "HEAD"]).trim().to_owned());
    }
    r.logged("repo_woken", 1);
    // The history is a request about the repo: it wakes it if it slept
    // again, and its page has the three commits in order.
    let start = Instant::now();
    loop {
        let page: serde_json::Value = connect(&r.tp)
            .call(
                methods::EVENTS_HISTORY,
                json!({ "repo_id": r.repo_id, "limit": 100 }),
            )
            .unwrap();
        let seen: Vec<String> = page
            .as_array()
            .or_else(|| page.get("events").and_then(|e| e.as_array()))
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|e| e["kind"] == "commit")
            .filter_map(|e| e["details"]["new_commit"].as_str().map(str::to_owned))
            .collect();
        if seen.ends_with(&commits) {
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "{seen:?} vs {commits:?}\n{}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
