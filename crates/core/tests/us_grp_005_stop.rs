//! US-GRP-005: an orderly stop saves the last known state, so the next start does not find a
//! change the stopped engine had already seen; a killed engine leaves it to the next start.
//! Temporary profile and repo only (NFR-01); no waits.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;

fn config(dirs: ProfileDirs) -> DaemonConfig {
    let env = DaemonEnv::from_vars(std::env::vars_os());
    DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: Default::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
        tm_capture: Default::default(),
    }
}

/// A started engine over an observed repo, then a change in its worktree, then `end` (a stop
/// or a drop), then another start. Returns how many `reconciled` events the repo has.
fn reconciled_after(end: impl FnOnce(Daemon)) -> usize {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let common: PathBuf = fx.repo.join(".git").canonicalize().unwrap();
    let (entry, _) = profile.add_repo(&common, None, 1).unwrap();
    drop(profile);
    let daemon = Daemon::start(config(tp.dirs())).unwrap();
    std::fs::write(fx.repo.join("dirty.txt"), "changed\n").unwrap();
    end(daemon);
    drop(Daemon::start(config(tp.dirs())).unwrap());
    let (store, _) = tp.open().open_store(&entry.repo_id).unwrap();
    store
        .events_page(None, None, 500)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "reconciled")
        .count()
}

#[test]
fn a_clean_stop_right_after_a_change_leaves_no_reconciled_event() {
    let n = reconciled_after(|d| {
        d.stop(StopCause::StopCommand {
            requested_by: "cli".into(),
        });
    });
    assert_eq!(n, 0);
}

#[test]
fn a_killed_daemon_keeps_the_reconciled_event() {
    assert_eq!(reconciled_after(drop), 1);
}
