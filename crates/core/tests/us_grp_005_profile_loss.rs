//! US-GRP-005 (PR-B): the engine notices that the profile it serves is gone
//! (or that another instance took its lock) and stops, instead of running
//! on a profile that no longer exists. Real daemons (in-process) over
//! temporary profiles and repos (NFR-01). No fixed waits: thread joins and
//! answers with a 10 s ceiling. The cause is compared by `as_str()`.

mod common;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, HeadView, SessionsListResult, WorktreeStatus,
};
use gitraptor_api::methods;
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, DaemonError, EngineState, InstanceLock, LogLimits,
    ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{ProfileDirs, ProfileError};
use serde_json::json;

const DEADLINE: Duration = Duration::from_secs(10);

fn config(dirs: ProfileDirs) -> DaemonConfig {
    let env = DaemonEnv::from_vars(std::env::vars_os());
    DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_millis(50),
        log: LogLimits::default(),
        stop_deadline: None,
        // The Claude Code session that runs the tests is not taken for an agent (`repo.add` is reserved to the developer).
        channel: ChannelConfig {
            agents: AgentMatcher::only(vec!["raptor-fake-agent".into()]),
            ..ChannelConfig::default()
        },
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        tm_capture: Default::default(),
    }
}

/// A daemon on its own thread; the report arrives through a channel so the
/// wait has a ceiling.
struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<()>>,
    report: Receiver<StopReport>,
}

impl Running {
    fn start(dirs: ProfileDirs) -> Self {
        let daemon = Daemon::start(config(dirs.clone())).unwrap();
        Self::spawn(dirs, daemon)
    }

    fn spawn(dirs: ProfileDirs, daemon: Daemon) -> Self {
        let handle = daemon.shutdown_handle();
        let (tx, report) = channel();
        let join = Some(std::thread::spawn(move || {
            let _ = tx.send(daemon.run());
        }));
        Self {
            dirs,
            handle,
            join,
            report,
        }
    }

    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    /// The cause the engine stopped by on its own, or a clear failure after
    /// the ceiling (the engine kept running: the loss was not noticed).
    fn stopped_by_itself(&mut self, expected: &str) {
        match self.report.recv_timeout(DEADLINE) {
            Ok(report) => {
                assert_eq!(report.cause.as_str(), expected, "{report:?}");
                self.join.take().unwrap().join().unwrap();
            }
            Err(_) => panic!(
                "the engine did not stop within {DEADLINE:?}: it does not detect the loss \
                 (expected stop cause {expected})"
            ),
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

/// Starts an engine over a fresh profile, waits until it serves, and returns
/// it with its instance id.
fn serving(tp: &TempProfile) -> (Running, String) {
    let live = Running::start(tp.dirs());
    let id = live.client().hello().instance_id.clone();
    (live, id)
}

/// After the loss: a new start works, in `NoRepos`, with another instance
/// id and a private `data/`.
fn assert_next_start_works(tp: &TempProfile, old_id: &str) -> Running {
    let daemon = Daemon::start(config(tp.dirs()))
        .unwrap_or_else(|e| panic!("the next start must work after the loss: {e}"));
    assert_eq!(daemon.state(), EngineState::NoRepos);
    let live = Running::spawn(tp.dirs(), daemon);
    let new_id = live.client().hello().instance_id.clone();
    assert_ne!(new_id, old_id, "a new profile has a new instance id");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&tp.dirs().data)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "data/ is recreated private");
    }
    live
}

#[cfg(unix)]
#[test]
fn deleting_the_whole_profile_stops_the_engine_and_the_next_start_works() {
    let tp = TempProfile::new();
    let (mut live, old_id) = serving(&tp);
    std::fs::remove_dir_all(tp.root.path().join("profile")).unwrap();
    live.stopped_by_itself("profile-lost");
    let _next = assert_next_start_works(&tp, &old_id);
}

/// Stops the engine by deleting `data/`; `None` when the OS refuses the
/// deletion (Windows keeps the open files), so the test is skipped.
fn lose_data(tp: &TempProfile) -> Option<(Running, String)> {
    let (mut live, old_id) = serving(tp);
    if std::fs::remove_dir_all(&tp.dirs().data).is_err() {
        eprintln!("skipped: data/ cannot be deleted while the engine holds it");
        return None;
    }
    live.stopped_by_itself("profile-lost");
    Some((live, old_id))
}

#[test]
fn deleting_the_profile_data_stops_the_engine_and_the_next_start_works() {
    let tp = TempProfile::new();
    let Some((_gone, old_id)) = lose_data(&tp) else {
        return;
    };
    let _next = assert_next_start_works(&tp, &old_id);
}

#[test]
fn a_repo_added_again_after_the_loss_has_no_agent_before_it() {
    let tp = TempProfile::new();
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "demo", true);
    let Some((_gone, old_id)) = lose_data(&tp) else {
        return;
    };
    drop(assert_next_start_works(&tp, &old_id));
    // `repo.add` is reserved to the developer and refuses a caller that descends from the
    // daemon, which here is the test process itself: the repo is added to the recreated
    // profile directly, as `repo.add` does, and the next engine serves it.
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    drop(profile);
    let next = Running::spawn(
        tp.dirs(),
        Daemon::start(config(tp.dirs())).expect("the engine starts with the repo"),
    );
    let mut c = next.client();
    let snap: ScopeSnapshot = c
        .call(
            methods::SCOPE_SNAPSHOT,
            json!({"scope": Scope::Repo { repo_id: entry.repo_id.clone() }}),
        )
        .unwrap();
    let ScopeSnapshot::Repo(added) = snap else {
        panic!("{snap:?}")
    };
    let wt = &added.repo.worktrees[0];
    match &wt.status {
        WorktreeStatus::Ready {
            head: HeadView::Branch { name },
            ..
        } => assert_eq!(name.raw(), "main", "the snapshot shows the current HEAD"),
        other => panic!("expected a ready worktree on main, got {other:?}"),
    }
    let history: EventsHistoryResult = c
        .call(
            methods::EVENTS_HISTORY,
            json!({"repo_id": entry.repo_id, "limit": 500}),
        )
        .unwrap();
    assert!(
        history
            .events
            .iter()
            .all(|e| e.kind != GitEventKind::Commit),
        "no commit event before the repo was added again: {:?}",
        history.events
    );
    let sessions: SessionsListResult = c.call(methods::SESSIONS_LIST, json!({})).unwrap();
    assert!(sessions.sessions.is_empty(), "{:?}", sessions.sessions);
}

fn data_state(tp: &TempProfile) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = files_under(&tp.dirs().data);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect()
}

#[test]
fn another_instance_holding_the_lock_stops_this_daemon_without_writing() {
    let tp = TempProfile::new();
    let (mut live, _id) = serving(&tp);
    let dirs = tp.dirs();
    let before = data_state(&tp);
    std::fs::remove_file(dirs.state.join("daemon.lock")).unwrap();
    let _other = InstanceLock::acquire(&dirs.state).expect("another instance takes the lock");
    live.stopped_by_itself("instance-lost");
    let after = data_state(&tp);
    assert_eq!(
        before, after,
        "a daemon that lost its lock must not write to data/ (content and size)"
    );
}

#[cfg(unix)]
#[test]
fn an_insecure_recreated_folder_is_refused_by_the_next_start() {
    use std::os::unix::fs::PermissionsExt;
    let tp = TempProfile::new();
    let (mut live, _id) = serving(&tp);
    let data = tp.dirs().data;
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_file(data.join("index.sqlite")).unwrap();
    live.stopped_by_itself("profile-lost");
    match Daemon::start(config(tp.dirs())) {
        Err(DaemonError::Profile(ProfileError::InsecureDir { .. })) => {}
        Err(other) => panic!("expected InsecureDir, got {other}"),
        Ok(_) => panic!("the next start must refuse a data/ folder that is not private"),
    }
}

#[test]
fn a_lost_profile_defers_the_stop_while_a_protected_write_runs() {
    let tp = TempProfile::new();
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "demo", true);
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&common_dir(&repo).canonicalize().unwrap(), None, 1)
        .unwrap();
    drop(profile);
    let (mut live, _id) = serving(&tp);
    let guard = gitraptor_core::repo_lock::try_lock(&entry.repo_id)
        .expect("the repo is free before the protected write");
    std::fs::remove_dir_all(&tp.dirs().data).unwrap();
    let log = tp.dirs().state.join("daemon.log");
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("profile_reset_deferred") {
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "the engine did not log profile_reset_deferred while the repo lock is held"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        live.report.try_recv().is_err(),
        "the engine must not stop while a protected write runs"
    );
    drop(guard);
    live.stopped_by_itself("profile-lost");
}
