//! C6 of `docs/dev-briefs/tm-prior-point-binding.md`, the premise of D6: the undo of a raw Git
//! event takes its own guaranteed prior before it restores, so restoring a point older than
//! needed is always recoverable (NFR-01). In-process daemon as in `us_tmc_004.rs`; temporary
//! repo and profile only.

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
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::oplog::{Oplog, SnapshotFilter, SnapshotLevel, SnapshotState};
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use serde_json::json;

const DEADLINE: Duration = Duration::from_secs(20);
const KEY: &str = "wt-wt-feat-login";
const ORIGINAL: &[u8] = b"fn api() {}\n";
const EDITED: &[u8] = b"fn api() { login(); }\n";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    gitraptor_git::paths::canonicalize(p).unwrap()
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

struct Running {
    fx: Fixture,
    tp: TempProfile,
    repo_id: String,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

/// Observes `fx`'s repo in a fresh profile and starts the daemon with a short quiet time.
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
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
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
    fn undo_ok(&self, worktree: &Path) -> UndoResult {
        let v = connect(&self.tp)
            .call(
                methods::TM_UNDO,
                json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
            )
            .unwrap();
        serde_json::from_value(v).unwrap()
    }

    fn store(&self) -> Option<SnapshotStore> {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .ok()
            .flatten()
    }

    /// Waits for a snapshot of `KEY` whose `api.rs` is `content`; its id.
    fn snapshot_with(&self, content: &[u8]) -> String {
        let start = Instant::now();
        loop {
            if let Some(store) = self.store() {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    if api_rs(&store, id).as_deref() == Some(content) {
                        return id.clone();
                    }
                }
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no snapshot of {KEY} as expected"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stops the daemon; the store and the repo's oplog. The caller keeps `self` (and with it
    /// the temporary profile) alive while it reads them.
    fn stop(&mut self) -> (SnapshotStore, Oplog) {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap();
        let store = self.store().expect("store");
        let oplog = Oplog::open(&self.tp.dirs(), &self.repo_id, 2).unwrap().0;
        (store, oplog)
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

/// `api.rs` of worktree `KEY` in snapshot `id`.
fn api_rs(store: &SnapshotStore, id: &str) -> Option<Vec<u8>> {
    store
        .files(id, KEY)
        .unwrap_or_default()
        .into_iter()
        .find(|(p, k, _)| p == "api.rs" && *k != TreeEntryKind::Gitlink)
        .map(|(_, _, oid)| store.read_blob(oid).unwrap())
}

/// The journal seq of the first entry of `subject` in state `state`.
fn journal_seq(oplog: &Oplog, subject: &str, state: &str) -> i64 {
    oplog
        .journal(subject)
        .unwrap()
        .into_iter()
        .find(|e| e.state.as_deref() == Some(state))
        .unwrap_or_else(|| panic!("{subject} never reached {state}"))
        .seq
}

#[test]
#[cfg_attr(
    windows,
    ignore = "a reserved command needs a person's console (TQ-14): not from SSH or CI"
)]
fn raw_git_undo_takes_its_own_guaranteed_prior_before_restoring() {
    let fx = Fixture::with_commit(&git());
    fx.write("api.rs", "fn api() {}\n");
    fx.git(&["add", "api.rs"]);
    fx.git(&["commit", "-q", "-m", "api"]);
    fx.git(&["branch", "feat-login"]);
    let wt = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let mut r = start(fx);
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    let captured = r.snapshot_with(EDITED);
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), ORIGINAL);

    let undo = r.undo_ok(&wt);
    assert!(
        undo.undone_operation_id.starts_with("git-event-"),
        "{undo:?}"
    );
    assert_eq!(undo.target_snapshot_id, captured);
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), EDITED);

    let (store, oplog) = r.stop();
    let op = oplog
        .operation(&undo.operation_id)
        .unwrap()
        .expect("undo row");
    let prior_id = op.prior_snapshot.clone().expect("the undo took a prior");
    let prior = oplog.snapshot(&prior_id).unwrap().expect("prior row");
    assert_eq!(prior.record.level, SnapshotLevel::GuaranteedPrior);
    assert_eq!(prior.state, SnapshotState::Complete);
    assert_eq!(
        prior.record.cause_operation.as_deref(),
        Some(undo.operation_id.as_str())
    );
    // The prior holds the state the undo replaced (after the reset), not its target.
    assert_eq!(api_rs(&store, &prior_id).as_deref(), Some(ORIGINAL));
    // Completed before the undo started applying.
    assert!(
        journal_seq(&oplog, &prior_id, "complete")
            < journal_seq(&oplog, &undo.operation_id, "applying"),
        "the prior was completed after the undo started applying"
    );
    let priors = oplog
        .snapshots(&SnapshotFilter {
            operation_id: Some(undo.operation_id.clone()),
            ..SnapshotFilter::default()
        })
        .unwrap();
    assert!(
        priors.iter().any(|s| s.record.snapshot_id == prior_id),
        "{priors:#?}"
    );
}
