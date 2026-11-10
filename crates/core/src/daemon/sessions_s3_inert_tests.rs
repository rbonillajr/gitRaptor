//! Contract tests of TD-GRP-007 (written by the criterion-tester): the `s3_evidence` line
//! carries the two new counters as integers (GRP007-LOG, SEC-04).

use std::collections::HashMap;

use gitraptor_api::messages::GitEventKind;

use std::time::Duration;

use gitraptor_git::resolve::ResolveConfig;

use super::s3_log_fields;
use crate::daemon::{Daemon, DaemonConfig, DaemonEnv, Field, LOG_FILE, LogLimits, Logger};
use crate::detect::{Diagnostics, S3Counts, S3Scope};
use crate::profile::ProfileDirs;

#[test]
fn the_s3_log_line_carries_daemon_inert_and_daemon_write_window_as_counted_integers() {
    let counts = S3Counts {
        daemon_inert: 7,
        daemon_write_window: 3,
        ..S3Counts::default()
    };
    // A repo id that is a path: the canary must never reach the line.
    let fields = s3_log_fields(
        "/home/u/canary-repo",
        GitEventKind::Commit,
        "ambiguous",
        Diagnostics::default(),
        S3Scope::Repo,
        counts,
    );
    assert!(
        fields.contains(&("daemon_inert", Field::Int(7))),
        "GRP007-LOG: s3_evidence carries daemon_inert with the counted value: {fields:?}"
    );
    assert!(
        fields.contains(&("daemon_write_window", Field::Int(3))),
        "GRP007-LOG: s3_evidence carries daemon_write_window with the counted value: {fields:?}"
    );

    let tmp = tempfile::tempdir().unwrap();
    let logger = Logger::open(tmp.path(), LogLimits::default()).unwrap();
    logger.info("s3_evidence", &fields);
    logger.flush();
    let text = std::fs::read_to_string(tmp.path().join(LOG_FILE)).unwrap();
    let line = text.lines().next().expect("one line");
    let map: HashMap<&str, &str> = line
        .split(' ')
        .skip(3)
        .map(|w| w.split_once('=').expect("key=value"))
        .collect();
    assert_eq!(
        map.get("daemon_inert"),
        Some(&"7"),
        "GRP007-LOG: the line has daemon_inert=7: {line}"
    );
    assert_eq!(
        map.get("daemon_write_window"),
        Some(&"3"),
        "GRP007-LOG: the line has daemon_write_window=3: {line}"
    );
    for key in ["daemon_inert", "daemon_write_window"] {
        let value = map[key];
        assert!(value.parse::<i64>().is_ok(), "{key} is an integer: {line}");
    }
    for (key, value) in &map {
        if !["repo", "event", "outcome", "scope"].contains(key) {
            assert!(value.parse::<i64>().is_ok(), "{key} is an integer: {line}");
        }
        assert!(!value.contains('/'), "{key} holds no path: {line}");
    }
    for canary in ["canary", "/home", "GIT_DIR", "--git-dir", "--work-tree"] {
        assert!(!line.contains(canary), "{canary} leaked: {line}");
    }
}

/// A real daemon, started and not run, over a temporary profile (NFR-01).
fn daemon(git: impl FnOnce(&DaemonEnv) -> ResolveConfig) -> (Daemon, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let daemon = Daemon::start(DaemonConfig {
        dirs: ProfileDirs::under_root(tmp.path().join("profile")),
        git: git(&env),
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
    })
    .unwrap();
    (daemon, tmp)
}

#[test]
fn engine_git_of_the_detector_is_the_canonical_path_of_the_daemons_git() {
    let (mut daemon, _tmp) = daemon(|env| env.git_resolve_config(None));
    let resolved = daemon
        .report
        .git
        .as_ref()
        .expect("this machine has a Git the daemon resolves")
        .path
        .clone();
    let _hooks = daemon.detector_hooks();
    let detector = daemon.detector.as_ref().expect("detector_hooks starts it");
    assert_eq!(
        detector.engine_git(),
        Some(resolved),
        "GRP007-ENGINE: detector_hooks must give the detector the daemon's SystemGit path"
    );
}

#[test]
fn engine_git_of_the_detector_is_none_without_a_resolved_git() {
    let (mut daemon, _tmp) = daemon(|_| ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    });
    assert!(daemon.report.git.is_none(), "no Git can be resolved here");
    let _hooks = daemon.detector_hooks();
    let detector = daemon.detector.as_ref().expect("detector_hooks starts it");
    assert_eq!(
        detector.engine_git(),
        None,
        "GRP007-ENGINE: without a resolved Git no git of the daemon is inert"
    );
}
