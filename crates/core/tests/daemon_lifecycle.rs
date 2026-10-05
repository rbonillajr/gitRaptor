//! TS-GRP-003: lifecycle of the daemon, run in-process against a temporary
//! profile and temporary repos (NFR-01). The process-level scenarios
//! (signals, `kill -9`, environment) are in `apps/cli/tests`.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, DaemonError, EngineState, LOG_FILE, LogLimits, StopCause,
    running_pid,
};
use gitraptor_core::profile::{DaemonRun, GapCause, Profile, ProfileDirs};
use gitraptor_git::resolve::ResolveConfig;

fn config(dirs: ProfileDirs, git: ResolveConfig) -> DaemonConfig {
    DaemonConfig {
        dirs,
        env: DaemonEnv::from_vars(std::env::vars_os()),
        git,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
    }
}

fn system_git() -> ResolveConfig {
    DaemonEnv::from_vars(std::env::vars_os()).git_resolve_config(None)
}

fn no_git() -> ResolveConfig {
    ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    }
}

/// A temporary profile with one observed repo; returns the worktree.
fn profile_with_repo(tp: &TempProfile) -> PathBuf {
    let repos = tp.root.path().join("repos");
    std::fs::create_dir_all(&repos).unwrap();
    let repo = init_repo(&repos, "r", true);
    let mut profile = tp.open();
    profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    repo
}

fn reopen(dirs: ProfileDirs) -> Profile {
    Profile::open(dirs).unwrap().0
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = files_under(dir)
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    files.sort();
    files
}

#[test]
fn reaches_observing_with_a_repo_and_valid_git() {
    let tp = TempProfile::new();
    profile_with_repo(&tp);
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    assert_eq!(daemon.state(), EngineState::Observing);
    assert!(daemon.report().git.is_some());
    assert_eq!(daemon.report().repos.len(), 1);
    assert_eq!(daemon.report().previous, DaemonRun::Never);
    daemon.stop(StopCause::Signal("TERM"));
}

#[test]
fn declared_states_without_repos_or_without_git() {
    let tp = TempProfile::new();
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    assert_eq!(daemon.state(), EngineState::NoRepos);
    daemon.stop(StopCause::Signal("TERM"));

    profile_with_repo(&tp);
    let daemon = Daemon::start(config(tp.dirs(), no_git())).unwrap();
    assert_eq!(
        daemon.state(),
        EngineState::WaitingForGit,
        "without Git the engine waits even with repos"
    );
    daemon.stop(StopCause::Signal("TERM"));
}

#[test]
fn second_daemon_exits_without_touching_the_store() {
    let tp = TempProfile::new();
    profile_with_repo(&tp);
    let first = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    let data_before = snapshot(&tp.dirs().data);
    let log_before = std::fs::read(tp.dirs().state.join(LOG_FILE)).unwrap();

    match Daemon::start(config(tp.dirs(), system_git())) {
        Err(DaemonError::AlreadyRunning { pid }) => assert_eq!(pid, Some(std::process::id())),
        Err(err) => panic!("expected AlreadyRunning, got {err}"),
        Ok(_) => panic!("a second daemon started"),
    }

    assert_eq!(snapshot(&tp.dirs().data), data_before, "store changed");
    assert_eq!(
        std::fs::read(tp.dirs().state.join(LOG_FILE)).unwrap(),
        log_before,
        "the second daemon wrote to the log"
    );
    first.stop(StopCause::Signal("TERM"));
}

#[test]
fn orderly_stop_persists_observed_until_and_releases_the_lock() {
    let tp = TempProfile::new();
    profile_with_repo(&tp);
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    let repo_id = daemon.report().repos[0].repo_id.clone();
    assert_eq!(
        running_pid(&tp.dirs().state).unwrap(),
        Some(std::process::id())
    );

    let handle = daemon.shutdown_handle();
    let runner = std::thread::spawn(move || daemon.run());
    assert!(handle.request(StopCause::Signal("TERM")));
    let report = runner.join().unwrap();
    assert!(report.recorded);

    assert_eq!(
        running_pid(&tp.dirs().state).unwrap(),
        None,
        "lock still held"
    );
    let profile = reopen(tp.dirs());
    let (store, _) = profile.open_store(&repo_id).unwrap();
    assert_eq!(store.observed_until().unwrap(), Some(report.stopped_ms));
    assert_eq!(
        profile.daemon_run().unwrap(),
        DaemonRun::Stopped {
            stopped_ms: report.stopped_ms,
            cause: "signal".into(),
            requested_by: Some("signal:TERM".into()),
        }
    );
    let log = std::fs::read_to_string(tp.dirs().state.join(LOG_FILE)).unwrap();
    assert!(
        log.contains("daemon_stopped cause=signal recorded=true signal=TERM"),
        "{log}"
    );
}

#[test]
fn heartbeat_persists_observed_until_while_observing() {
    let tp = TempProfile::new();
    profile_with_repo(&tp);
    let mut cfg = config(tp.dirs(), system_git());
    cfg.heartbeat = Duration::from_millis(20);
    let daemon = Daemon::start(cfg).unwrap();
    let repo_id = daemon.report().repos[0].repo_id.clone();
    let handle = daemon.shutdown_handle();
    let runner = std::thread::spawn(move || daemon.run());
    std::thread::sleep(Duration::from_millis(200));
    // Read while the daemon runs: WAL lets a reader see committed marks.
    let profile = Profile::open(tp.dirs());
    let marked = profile
        .ok()
        .and_then(|(p, _)| p.open_store(&repo_id).ok())
        .and_then(|(s, _)| s.observed_until().ok().flatten());
    handle.request(StopCause::Signal("TERM"));
    runner.join().unwrap();
    assert!(marked.is_some(), "no periodic observed-until mark");
}

#[test]
fn crash_during_active_session_is_marked_on_next_start() {
    let tp = TempProfile::new();
    let repo = profile_with_repo(&tp);
    {
        let profile = tp.open();
        let entry = profile.repos().unwrap().remove(0);
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        store.write_batch(&sample_batch(&repo, "s1", 1)).unwrap();
    }
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    // Dropping without `stop` is what a crash leaves behind: the OS frees the
    // lock and the run mark stays "running".
    drop(daemon);

    let daemon = Daemon::start(config(tp.dirs(), system_git()))
        .expect("a new daemon gets the lock without manual cleanup");
    assert!(matches!(
        daemon.report().previous,
        DaemonRun::Running { .. }
    ));
    let gap = daemon.report().repos[0].pending_gap.clone().unwrap();
    assert_eq!(gap.cause, GapCause::DaemonDownDuringSession);
    assert_eq!(gap.requested_by, None);
    daemon.stop(StopCause::Signal("TERM"));

    // An orderly stop by signal with the session still active is not an
    // attributed stop either (SEC-13).
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    let gap = daemon.report().repos[0].pending_gap.clone().unwrap();
    assert_eq!(gap.cause, GapCause::DaemonDownDuringSession);
    assert_eq!(gap.requested_by.as_deref(), Some("signal:TERM"));
    assert!(gap.from_ms.is_some());
    daemon.stop(StopCause::Signal("TERM"));
}

#[test]
fn crash_without_sessions_is_a_plain_daemon_down_gap() {
    let tp = TempProfile::new();
    profile_with_repo(&tp);
    drop(Daemon::start(config(tp.dirs(), system_git())).unwrap());
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    let gap = daemon.report().repos[0].pending_gap.clone().unwrap();
    assert_eq!(gap.cause, GapCause::DaemonDown);
    daemon.stop(StopCause::Signal("TERM"));
}

#[test]
fn authorized_stop_command_is_an_attributed_stop() {
    let tp = TempProfile::new();
    profile_with_repo(&tp);
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    daemon.stop(StopCause::StopCommand {
        requested_by: "client-1".into(),
    });
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    let gap = daemon.report().repos[0].pending_gap.clone().unwrap();
    assert_eq!(gap.cause, GapCause::DaemonStopped);
    assert_eq!(gap.requested_by.as_deref(), Some("client-1"));
    daemon.stop(StopCause::Signal("TERM"));
}

#[cfg(unix)]
#[test]
fn insecure_state_folder_stops_the_start() {
    use std::os::unix::fs::PermissionsExt;
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    std::fs::create_dir_all(&dirs.state).unwrap();
    std::fs::set_permissions(&dirs.state, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(
        dirs.state.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    match Daemon::start(config(dirs.clone(), system_git())) {
        Err(DaemonError::Profile(_)) => {}
        Err(err) => panic!("unexpected error {err}"),
        Ok(_) => panic!("started on an insecure state folder"),
    }
    assert!(!dirs.state.join("daemon.lock").exists());
}

/// TS-TMC-002: the start recovers the oplog of every observed repo before
/// accepting operations, even when the engine store cannot be opened.
#[cfg(unix)]
#[test]
fn start_recovers_the_time_machine_oplog() {
    use gitraptor_core::timemachine::oplog::{
        Channel, CompleteInfo, NewOperation, NewSnapshot, OperationKind, OperationState,
        OperationTransition, Oplog, Requester, Scope, SnapshotLevel, Target, file_identity,
    };

    let tp = TempProfile::new();
    let repo = profile_with_repo(&tp);
    let entry = reopen(tp.dirs()).repos().unwrap().remove(0);
    let wt = repo.to_str().unwrap().to_owned();

    // A previous daemon died while applying an operation that held the
    // index lock of the repo.
    let (mut oplog, _) = Oplog::open(&tp.dirs(), &entry.repo_id, 1).unwrap();
    let op = oplog
        .record_operation(
            &NewOperation {
                kind: OperationKind::Protected,
                subtype: Some("checkout".into()),
                scope: Scope {
                    worktrees: vec![wt.clone()],
                    refs: vec![],
                },
                requester: Requester::Unattributed,
                channel: Channel::Cli,
                confirmed: true,
                target: Target::None,
                warnings: vec![],
                engine_mark: 0,
            },
            2,
        )
        .unwrap();
    let snap = oplog
        .begin_snapshot(
            &NewSnapshot {
                level: SnapshotLevel::GuaranteedPrior,
                worktrees: vec![wt.clone()],
                engine_mark: Some(0),
                cause_operation: Some(op.clone()),
                cause_event_seq: None,
            },
            3,
        )
        .unwrap();
    oplog
        .complete_snapshot(&snap, &CompleteInfo::default(), 4)
        .unwrap();
    for t in [
        OperationTransition::PriorSnapshot { snapshot_id: &snap },
        OperationTransition::Ready,
        OperationTransition::Applying { step: 1 },
    ] {
        oplog.advance_operation(&op, t, 5).unwrap();
    }
    let lock = entry.canonical_path.join("index.lock");
    std::fs::write(&lock, b"").unwrap();
    oplog
        .record_lock_taken(&op, &lock, file_identity(&lock).unwrap().unwrap(), 6)
        .unwrap();
    let foreign = entry.canonical_path.join("HEAD.lock");
    std::fs::write(&foreign, b"").unwrap();
    drop(oplog);

    // The engine store comes from a newer binary: the repo is not observed,
    // but its oplog is still recovered.
    let store = reopen(tp.dirs()).store_path(&entry.repo_id);
    rusqlite::Connection::open(&store)
        .unwrap()
        .pragma_update(None, "user_version", 999)
        .unwrap();

    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    assert_eq!(daemon.report().unavailable, vec![entry.repo_id.clone()]);
    let tm = &daemon.report().time_machine[0];
    assert_eq!(tm.recovery.interrupted_operations, vec![op.clone()]);
    assert_eq!(tm.recovery.released_locks, vec![lock.clone()]);
    assert!(!lock.exists());
    assert!(foreign.exists(), "a lock not in the journal stays");
    let oplog = daemon.oplog(&entry.repo_id).unwrap();
    assert_eq!(
        oplog.operation(&op).unwrap().unwrap().state,
        OperationState::Interrupted
    );
    assert_eq!(oplog.pending_notices(Some(&wt)).unwrap().len(), 1);
    daemon.stop(StopCause::Signal("TERM"));

    // An orderly restart finds nothing new to recover.
    let daemon = Daemon::start(config(tp.dirs(), system_git())).unwrap();
    assert!(daemon.report().time_machine[0].recovery.is_clean());
    daemon.stop(StopCause::Signal("TERM"));
}
