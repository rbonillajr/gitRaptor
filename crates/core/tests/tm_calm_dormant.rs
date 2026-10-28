//! A Time Machine operation wakes a dormant repo before it waits for the engine's calm
//! (fix/tm-restore-calm-mark, D7): a branch moved while the repo slept is a ref the engine has not
//! persisted, and the undo must not end in `repo-busy` for it.
//!
//! A real daemon (in-process) over a testkit fixture and a temporary profile; never this repo or
//! the real profile (NFR-01). Every step waits for a state (a log line, a snapshot) with a
//! deadline, never a fixed time. macOS and Linux, like the other daemon tier tests.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::timemachine::UndoResult;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
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
const KEY: &str = "wt-wt-feat-login";
const EDITED: &[u8] = b"fn api() { login(); }\n";

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

/// Observes `fx`'s repo in a fresh profile, with tiers on and a short threshold, and the
/// continuous capture with a short quiet time.
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
            ..TierConfig::default()
        },
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
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

    /// Waits for a snapshot of worktree `key` holding `path` with `content`.
    fn snapshot_with(&self, key: &str, path: &str, content: &[u8]) -> String {
        let start = Instant::now();
        loop {
            if let Some(store) = SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
                .ok()
                .flatten()
            {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    let has = store
                        .files(id, key)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
                        .any(|(p, _, oid)| p == path && store.read_blob(oid).unwrap() == content);
                    if has {
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
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

#[test]
fn a_tm_operation_wakes_a_dormant_repo_before_settling() {
    let fx = Fixture::with_commit(&git());
    fx.write("api.rs", "fn api() {}\n");
    fx.git(&["add", "api.rs"]);
    fx.git(&["commit", "-q", "-m", "api"]);
    fx.git(&["branch", "feat-login"]);
    fx.git(&["branch", "side"]);
    let wt = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let r = start(fx);
    // Something to undo: an edit the Time Machine captured, then a raw `reset --hard`.
    r.logged("repo_dormant", 1);
    std::fs::write(wt.join("api.rs"), EDITED).unwrap();
    r.logged("repo_woken", 1);
    let captured = r.snapshot_with(KEY, "api.rs", EDITED);
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    // The repo sleeps again, and a branch moves while it sleeps.
    r.logged("repo_dormant", 2);
    let older = r.fx.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    r.fx.git(&["update-ref", "refs/heads/side", &older]);
    // The undo waits for a calm that covers the moved branch: it wakes the repo, it is not busy.
    let result = connect(&r.tp).call(
        methods::TM_UNDO,
        json!({ "worktree": wt.to_str().unwrap(), "surface": "cli" }),
    );
    let undo: UndoResult = match result {
        Ok(value) => serde_json::from_value(value).unwrap(),
        Err(ClientError::Rpc(e)) => panic!(
            "the undo was refused ({}): {:?}\n{}",
            e.code,
            e.data,
            r.log()
        ),
        Err(e) => panic!("{e:?}"),
    };
    assert_eq!(undo.target_snapshot_id, captured, "{}", r.log());
    assert_eq!(std::fs::read(wt.join("api.rs")).unwrap(), EDITED);
}
