//! The temporary entries the sweep after a crash keeps (DS-TS-TMC-003, Enmienda T2), end to
//! end: an application dies between its two moves and leaves the displaced file under a
//! temporary name whose path someone took meanwhile; the daemon starts, keeps it and shows it in
//! the snapshot of a connection with `timemachine.kept-temps`; `timemachine.undo` takes the
//! interrupted operation back, the temporary entry goes away (captured in the undo's own prior
//! snapshot, never lost) and so does the line.
//!
//! A real daemon (in-process) on a separate temporary profile, a real client over the channel,
//! real Git and a testkit fixture; never this repo or the real profile (NFR-01). The interrupted
//! operation is written before the daemon starts with the applier of TS-TMC-003 and its crash
//! hook, as `tm_apply.rs` does. No test waits on time: each step waits for the daemon's answer.
//!
//! macOS only, like the other channel tests. Linux: Pendiente: etapa de validación
//! multiplataforma.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::{ClientKind, Snapshot};
use gitraptor_api::timemachine::{KeptTempsView, UndoResult};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
};
use gitraptor_core::timemachine::apply::{
    Applier, ApplyError, ApplyHooks, ApplyPlan, PlanWorktree, RefScope,
};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationState, OperationTransition, Oplog, Requester,
    Scope, SnapshotLevel, Target,
};
use gitraptor_core::timemachine::store::{CaptureRequest, SnapshotStore, WorktreeScope};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::{Invoker, SystemGit};
use gitraptor_testkit::Fixture;
use serde_json::json;

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn system_git() -> SystemGit {
    match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default()) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git >= 2.38: {diagnostics:?}"),
    }
}

fn temps(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".gitraptor-tm-"))
        .collect();
    found.sort();
    found
}

/// The repo observed in a fresh profile, with an operation interrupted between the two moves of
/// `extra.txt` ("agent work"), and the path then taken by someone else. Returns the profile, the
/// canonical worktree, the repo id, the operation and its prior snapshot.
fn interrupted(fx: &Fixture) -> (TempProfile, PathBuf, String, String, String) {
    let tp = TempProfile::new();
    let root = fx.repo.canonicalize().unwrap();
    let (entry, _) = tp
        .open()
        .add_repo(&root.join(".git").canonicalize().unwrap(), None, 1)
        .unwrap();
    let dirs = tp.dirs();
    let (store, _) = SnapshotStore::open_or_create(&dirs, &entry.repo_id).unwrap();
    let oplog = Mutex::new(Oplog::open(&dirs, &entry.repo_id, 1).unwrap().0);
    let request = |level| CaptureRequest {
        level,
        repo: root.clone(),
        worktrees: vec![WorktreeScope {
            key: "main".into(),
            path: root.clone(),
            hint: None,
        }],
        engine_mark: None,
        cause_operation: None,
        cause_event_seq: None,
        include_credentials: false,
        still_valid: None,
        give_way: None,
    };
    let target = store
        .capture(&oplog, &request(SnapshotLevel::Observation))
        .unwrap()
        .snapshot_id;
    std::fs::write(root.join("extra.txt"), "agent work\n").unwrap();
    let op = oplog
        .lock()
        .unwrap()
        .record_operation(
            &NewOperation {
                kind: OperationKind::Restore,
                subtype: None,
                scope: Scope {
                    worktrees: vec![root.to_string_lossy().into_owned()],
                    refs: vec![],
                },
                requester: Requester::Unattributed,
                channel: Channel::Cli,
                confirmed: true,
                target: Target::Snapshot(target.clone()),
                warnings: vec![],
                engine_mark: 1,
            },
            10,
        )
        .unwrap();
    let mut req = request(SnapshotLevel::GuaranteedPrior);
    req.cause_operation = Some(op.clone());
    let prior = store.capture(&oplog, &req).unwrap().snapshot_id;
    {
        let mut log = oplog.lock().unwrap();
        log.advance_operation(
            &op,
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior,
            },
            11,
        )
        .unwrap();
        log.advance_operation(&op, OperationTransition::Ready, 12)
            .unwrap();
    }
    let write = WriteContext::new(
        system_git(),
        Invoker::default(),
        &dirs.data.join("tm").join(&entry.repo_id),
    )
    .unwrap();
    let plan = ApplyPlan {
        target_snapshot: target,
        prior_snapshot: prior.clone(),
        worktrees: vec![PlanWorktree {
            key: "main".into(),
            root: root.clone(),
            recreate_id: None,
        }],
        refs: RefScope::All,
    };
    let result = Applier::new(
        &store,
        &write,
        &oplog,
        root.clone(),
        tp.root.path().join("profile"),
    )
    .with_clock(|| 20)
    .with_hooks(ApplyHooks {
        simulate_crash_between_moves: true,
        ..Default::default()
    })
    .apply(&op, &plan);
    assert!(
        matches!(result, Err(ApplyError::Interrupted { step: 6, .. })),
        "{result:?}"
    );
    assert_eq!(temps(&root).len(), 1);
    // Someone takes the path before the daemon starts: the sweep cannot put it back.
    std::fs::write(root.join("extra.txt"), "someone else\n").unwrap();
    (tp, root, entry.repo_id, op, prior)
}

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    /// With `kept_temps`, the daemon serves `timemachine.kept-temps`; without it, it plays an
    /// older daemon that does not.
    fn start(tp: TempProfile, kept_temps: bool) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let config = DaemonConfig {
            dirs: tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: ChannelConfig {
                capabilities: gitraptor_api::capability::all()
                    .map(|c| c.name)
                    .filter(|c| kept_temps || *c != methods::CAP_TM_KEPT_TEMPS.name)
                    .collect(),
                ..ChannelConfig::default()
            },
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery: Default::default(),
            tm_capture: TmCapture::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            tp,
            handle,
            join: Some(join),
        }
    }

    fn connect(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// The kept temporary entries of the only repo, as a connection sees them (the client
    /// accepts every capability the daemon serves).
    fn kept(&self) -> Option<KeptTempsView> {
        let snap: Snapshot = self
            .connect()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        assert_eq!(snap.repos.len(), 1, "{snap:?}");
        snap.repos[0].kept_temps.clone()
    }

    fn stop(mut self) -> TempProfile {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap();
        // Nothing to take: `Drop` finds the daemon already stopped.
        std::mem::replace(&mut self.tp, TempProfile::new())
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

/// Escenario: `raptor status` muestra los temporales conservados y `raptor undo` los limpia.
#[test]
fn a_kept_temporary_file_shows_until_the_undo_takes_it_away() {
    let fx = Fixture::busy(&git());
    let (tp, root, repo_id, op, prior) = interrupted(&fx);
    let kept_name = temps(&root);
    // A daemon without the capability sweeps and keeps it, and shows nothing.
    let older = Running::start(tp, false);
    assert_eq!(older.kept(), None);
    let tp = older.stop();
    assert_eq!(temps(&root), kept_name);
    // The next start sweeps again and keeps it again.
    let r = Running::start(tp, true);

    // The sweep kept it, untouched, and the path keeps the other content.
    assert_eq!(temps(&root), kept_name);
    assert_eq!(
        std::fs::read(root.join(&kept_name[0])).unwrap(),
        b"agent work\n"
    );
    assert_eq!(
        std::fs::read(root.join("extra.txt")).unwrap(),
        b"someone else\n"
    );
    assert_eq!(
        r.kept(),
        Some(KeptTempsView {
            count: 1,
            foreign: 0,
            operation_id: op.clone(),
            undo_next: true,
        })
    );

    let undo: UndoResult = serde_json::from_value(
        r.connect()
            .call(
                methods::TM_UNDO,
                json!({ "worktree": root.to_str().unwrap(), "surface": "cli" }),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(undo.undone_operation_id, op);
    assert_eq!(undo.target_snapshot_id, prior);

    // The worktree is the state before the interrupted operation, with no temporary entry
    // left; what was there (the other content and the temporary file) is in the undo's prior.
    assert!(temps(&root).is_empty(), "{:?}", temps(&root));
    assert_eq!(
        std::fs::read(root.join("extra.txt")).unwrap(),
        b"agent work\n"
    );
    assert_eq!(r.kept(), None);
    let tp = r.stop();
    let dirs = tp.dirs();
    let oplog = Oplog::open(&dirs, &repo_id, 2).unwrap().0;
    let undo_op = oplog.operation(&undo.operation_id).unwrap().unwrap();
    assert_eq!(undo_op.state, OperationState::Finished);
    let undo_prior = undo_op.prior_snapshot.unwrap();
    let store = SnapshotStore::open_existing(&dirs, &repo_id)
        .unwrap()
        .unwrap();
    let files = store.files(&undo_prior, "main").unwrap();
    assert!(
        files.iter().any(|(p, _, _)| p == &kept_name[0]),
        "the temporary file is in the undo's prior snapshot: {:?}",
        files.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
    );
}
