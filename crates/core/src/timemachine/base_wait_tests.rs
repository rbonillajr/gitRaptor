//! TS-TMC-005: the undo waits, with its own deadline and an explicit signal, for the capture that
//! gives the base point B of an agent's change, instead of answering `overlap-unverified` at once.
//! Child module of `timemachine::base_wait`, mounted there as `mod tests`.
//!
//! The continuous capture runs for real ([`ContinuousCapture`]) over a temporary repo and profile;
//! the fault layer ([`CaptureLayer`]) holds the capture of the reset's event as long as each
//! criterion says, and the test hands the capture the engine's event (`git_event`) the way the
//! daemon does. Never this repo nor the real profile (NFR-01).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::messages::GitEventKind;
use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::{Invoker, SystemGit};
use gitraptor_testkit::Fixture;

use super::BASE_WAIT;
use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::{ProcError, ProcInfo, ProcSource};
use crate::channel::requester::Who;
use crate::daemon::{LOG_FILE, LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::confirm::Confirmation;
use crate::timemachine::continuous::{CaptureConfig, CaptureDeps, CaptureLayer, ContinuousCapture};
use crate::timemachine::engine::{EngineLink, RawGitEvent, SessionAttribution};
use crate::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationState, OperationTransition, Oplog, Requester,
    RequesterOrigin, Scope, SnapshotFilter, SnapshotLevel, Target,
};
use crate::timemachine::protected::{McpRepos, TimeMachineBackend, TmRepos};
use crate::timemachine::store::{CaptureError, CaptureRequest, WorktreeScope};
use crate::timemachine::undo::{
    OVERLAP_BUDGET, Selection, TmRepoHandle, UndoDone, UndoEnv, UndoError, undo_selected,
};

const REPO: &str = "5b0e7a1c-2d3f-4e5a-9b6c-7d8e9f0a1b2c";
const A: &str = "100:1";
const SPAN_MS: i64 = 600_000;
/// The engine sequence number of the reset every test undoes.
const RESET_SEQ: i64 = 2;
/// CA-1: how long the layer holds the reset's capture.
const HOLD: Duration = Duration::from_millis(500);
/// CA-2: a hold longer than `BASE_WAIT`.
const LONG_HOLD: Duration = Duration::from_secs(5);

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn agent() -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: A.into(),
    }
}

fn agent_who() -> Who {
    Who {
        actor: Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        },
        requester: agent(),
    }
}

/// A calm engine at mark 1 000 that reports `events` for the worktree and attributes session `A`
/// to the agent.
struct Engine {
    events: Vec<RawGitEvent>,
}

impl EngineLink for Engine {
    fn mark(&self, _repo_id: &str) -> i64 {
        1_000
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
        Some(1_000)
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(self.events.clone())
    }
    fn session_attribution(&self, _repo_id: &str) -> Option<SessionAttribution> {
        Some(SessionAttribution(HashMap::from([(A.to_owned(), agent())])))
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, _repo_id: &str) -> u64 {
        0
    }
}

struct NoProcs;

impl ProcSource for NoProcs {
    fn read(&self, _pid: u32) -> Result<ProcInfo, ProcError> {
        Err(ProcError::Gone)
    }
    fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
        None
    }
}

fn system_git() -> SystemGit {
    match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default()) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git >= 2.38: {diagnostics:?}"),
    }
}

/// A layer that holds the capture caused by event `seq` once, up to `hold` (or until the test
/// drops the returned sender), runs `then` and lets the capture go on.
fn holding(
    seq: i64,
    hold: Duration,
    then: impl Fn() + Send + Sync + 'static,
) -> (CaptureLayer, Sender<()>) {
    let (tx, rx) = channel::<()>();
    let rx = Mutex::new(rx);
    let held = AtomicBool::new(false);
    let layer: CaptureLayer = Arc::new(move |req: &CaptureRequest| {
        if req.cause_event_seq == Some(seq) && !held.swap(true, Ordering::SeqCst) {
            let _ = rx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(hold);
            then();
        }
        None
    });
    (layer, tx)
}

/// A layer that discards every capture caused by event `seq`, so the capture gives it up after
/// `MAX_RETRIES`.
fn discarding(seq: i64) -> CaptureLayer {
    Arc::new(move |req: &CaptureRequest| {
        (req.cause_event_seq == Some(seq)).then_some(CaptureError::Discarded)
    })
}

struct Rig {
    f: Fixture,
    dirs: ProfileDirs,
    repos: Arc<TmRepos>,
    repo: TmRepoHandle,
    seals: Arc<super::CaptureSeals>,
}

fn rig() -> Rig {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    f.write("keep.rs", "fn keep() {}\n");
    f.git(&["add", "keep.rs"]);
    f.git(&["commit", "-q", "-m", "keep"]);
    let dirs = ProfileDirs::under_root(&f.profile);
    let common = crate::observe::locate(&f.repo).unwrap();
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    repos.insert(REPO, &common, oplog);
    let backend = TimeMachineBackend::new(Arc::clone(&repos), None, Arc::new(McpRepos::new([])));
    let repo = backend
        .repo_of(&f.repo)
        .expect("the temporary repo is observed");
    Rig {
        f,
        dirs,
        repos,
        repo,
        seals: Arc::default(),
    }
}

impl Rig {
    fn head(&self) -> String {
        self.f.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn commit(&self, path: &str, content: &str) -> String {
        self.f.write(path, content);
        self.f.git(&["add", path]);
        self.f.git(&["commit", "-q", "-m", path]);
        self.head()
    }

    fn read(&self, rela: &str) -> Option<String> {
        std::fs::read_to_string(self.f.repo.join(rela)).ok()
    }

    /// The worktree as the engine names it (its canonical root).
    fn worktree(&self) -> PathBuf {
        self.repo.repo.worktree.clone()
    }

    /// An observation point of the worktree at engine mark `mark`: the state before the reset.
    fn point(&self, mark: i64) -> String {
        let req = CaptureRequest {
            level: SnapshotLevel::Observation,
            echo_settled: None,
            common_dir: crate::observe::locate(&self.f.repo).unwrap(),
            worktrees: vec![WorktreeScope {
                key: "main".into(),
                path: self.f.repo.clone(),
                hint: None,
            }],
            engine_mark: Some(mark),
            cause_operation: None,
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        };
        self.repo
            .store
            .as_ref()
            .expect("the store opens")
            .capture(&self.repo.repo.oplog, &req)
            .unwrap()
            .snapshot_id
    }

    /// `actor`'s `git reset --hard <to>` on main, as the engine reports it with `RESET_SEQ`.
    fn reset(&self, to: &str, actor: Requester) -> RawGitEvent {
        let old = self.head();
        self.f.git(&["reset", "-q", "--hard", to]);
        RawGitEvent {
            seq: RESET_SEQ,
            kind: GitEventKind::Reset,
            branch: Some("main".into()),
            actor,
            observed_ms: now_ms() - 1_000,
            in_gap: false,
            inferred: false,
            old_commit: Some(old),
            new_commit: Some(to.to_owned()),
        }
    }

    fn deps(&self, events: Vec<RawGitEvent>, layer: Option<CaptureLayer>) -> CaptureDeps {
        CaptureDeps {
            repos: Arc::clone(&self.repos),
            engine: Arc::new(Engine { events }),
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer,
            free_space_floor: None,
            after_mark: None,
            seals: Arc::clone(&self.seals),
        }
    }

    /// Starts the continuous capture and hands it the engine's event of the reset, as the daemon
    /// does once the event is persisted.
    fn capture(&self, deps: &CaptureDeps) -> ContinuousCapture {
        let capture = ContinuousCapture::start(CaptureConfig::default(), deps.clone())
            .expect("the capture thread starts");
        capture.git_event(REPO, &self.worktree(), RESET_SEQ);
        capture
    }

    fn undo(
        &self,
        deps: Option<&CaptureDeps>,
        who: &Who,
        selection: Selection,
        git: &SystemGit,
    ) -> Result<UndoDone, UndoError> {
        let marks = Arc::new(ExecutorMarks::default());
        let stopping = AtomicBool::new(false);
        let invoker = Invoker::default();
        let env = UndoEnv {
            marks: &marks,
            procs: &NoProcs,
            stopping: &stopping,
            prior_deadline: Duration::from_secs(30),
            git: Some(git),
            invoker: &invoker,
            engine: deps,
            fallback_mark: 1_000,
        };
        undo_selected(
            &self.repo,
            who,
            Channel::Cli,
            &Confirmation::unavailable(),
            &env,
            selection,
        )
    }

    /// Every `tm_base_wait` the daemon log holds: its `base_wait_ms` and `result`.
    fn base_waits(&self) -> Vec<(i64, String)> {
        let log = std::fs::read_to_string(self.dirs.state.join(LOG_FILE)).unwrap_or_default();
        log.lines()
            .filter(|line| line.split(' ').nth(2) == Some("tm_base_wait"))
            .map(|line| {
                let field = |key: &str| {
                    line.split(' ')
                        .find_map(|t| t.strip_prefix(key)?.strip_prefix('='))
                        .map(str::to_owned)
                };
                (
                    field("base_wait_ms")
                        .and_then(|ms| ms.parse().ok())
                        .unwrap_or(-1),
                    field("result").unwrap_or_default(),
                )
            })
            .collect()
    }

    /// The captures the reset caused: the candidates for its base B.
    fn caused_by_reset(&self) -> Vec<(String, i64)> {
        self.repo
            .repo
            .oplog
            .lock()
            .unwrap()
            .snapshots(&SnapshotFilter::default())
            .unwrap()
            .into_iter()
            .filter(|s| s.record.cause_event_seq == Some(RESET_SEQ))
            .map(|s| (s.record.snapshot_id.clone(), s.record.seq))
            .collect()
    }

    /// The oplog sequence of snapshot `id`.
    fn snapshot_seq(&self, id: &str) -> i64 {
        self.repo
            .repo
            .oplog
            .lock()
            .unwrap()
            .snapshots(&SnapshotFilter::default())
            .unwrap()
            .into_iter()
            .find(|s| s.record.snapshot_id == id)
            .map(|s| s.record.seq)
            .expect("the snapshot is in the oplog")
    }

    /// The rejection `err` recorded in the oplog, checked to be a rejected undo.
    fn assert_rejection_recorded(&self, err: &UndoError) {
        let (UndoError::Rejected {
            operation_id: Some(id),
            ..
        }
        | UndoError::Overlap {
            operation_id: Some(id),
            ..
        }) = err
        else {
            panic!("expected a recorded rejection, got {err:?}");
        };
        let view = self
            .repo
            .repo
            .oplog
            .lock()
            .unwrap()
            .operation(id)
            .unwrap()
            .expect("the rejection is in the oplog");
        assert_eq!(view.record.kind, OperationKind::Undo);
        assert_eq!(view.state, OperationState::Rejected);
    }
}

/// The reason of a refusal; an overlap that could not be checked is `overlap-unverified`.
fn reason(err: &UndoError) -> TmRejectReason {
    match err {
        UndoError::Rejected { reason, .. } => *reason,
        UndoError::Overlap { report, .. } if report.unverified => TmRejectReason::OverlapUnverified,
        UndoError::Overlap { .. } => TmRejectReason::Overlap,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// The agent's history every test starts from: `api.rs` at v2 (the agent's work), a point of it,
/// and the agent's raw `git reset --hard` back to v1. Returns (v1, v2, the point before).
fn agent_history(r: &Rig) -> (String, String, String) {
    let first = r.commit("api.rs", "v1\n");
    let second = r.commit("api.rs", "v2\n");
    let before = r.point(1);
    (first, second, before)
}

// --- CA-1 ---

#[test]
fn ca1_the_undo_right_after_a_raw_reset_waits_for_its_base_and_recovers_the_work() {
    let r = rig();
    let (first, second, before) = agent_history(&r);
    let reset = r.reset(&first, agent());
    let (layer, release) = holding(RESET_SEQ, HOLD, || {});
    let deps = r.deps(vec![reset], Some(layer));
    let capture = r.capture(&deps);
    let git = system_git();

    let out = r.undo(Some(&deps), &agent_who(), Selection::Last, &git);

    drop(release);
    capture.stop();
    let done = out.unwrap_or_else(|e| {
        panic!(
            "TMC005-CA1: the undo right after the reset must wait for the capture of its base \
             and recover the agent's work at the first try, got {e:?}"
        )
    });
    assert_eq!(
        done.target_snapshot_id, before,
        "TMC005-CA1: the undo returns to the point before the reset"
    );
    assert_eq!(r.head(), second, "TMC005-CA1: the branch goes back to v2");
    assert_eq!(r.read("api.rs").as_deref(), Some("v2\n"));
}

#[test]
fn ca1_agent_the_undo_by_agent_right_after_a_raw_reset_waits_for_its_base() {
    let r = rig();
    let (first, second, _) = agent_history(&r);
    let reset = r.reset(&first, agent());
    let (layer, release) = holding(RESET_SEQ, HOLD, || {});
    let deps = r.deps(vec![reset], Some(layer));
    let capture = r.capture(&deps);
    let git = system_git();

    let out = r.undo(
        Some(&deps),
        &agent_who(),
        Selection::Agent {
            selector: A.to_owned(),
            span_ms: SPAN_MS,
        },
        &git,
    );

    drop(release);
    capture.stop();
    let done = out.unwrap_or_else(|e| {
        panic!(
            "TMC005-CA1: `undo --agent` right after the reset must wait for the capture of its \
             base and recover the agent's work at the first try, got {e:?}"
        )
    });
    assert_eq!(done.undone_all.len(), 1);
    assert_eq!(r.head(), second, "TMC005-CA1: the branch goes back to v2");
    assert_eq!(r.read("api.rs").as_deref(), Some("v2\n"));
}

// --- CA-2 ---

#[test]
fn ca2_a_capture_held_past_base_wait_ends_overlap_unverified_within_the_deadline() {
    let r = rig();
    let (first, _, _) = agent_history(&r);
    let reset = r.reset(&first, agent());
    let (layer, release) = holding(RESET_SEQ, LONG_HOLD, || {});
    let deps = r.deps(vec![reset], Some(layer));
    let capture = r.capture(&deps);
    let git = system_git();
    let status_before = r.f.git(&["status", "--porcelain"]);

    let started = Instant::now();
    let out = r.undo(Some(&deps), &agent_who(), Selection::Last, &git);
    let took = started.elapsed();

    drop(release);
    capture.stop();
    let err = out.expect_err("TMC005-CA2: without its base the undo is refused (D7)");
    assert_eq!(reason(&err), TmRejectReason::OverlapUnverified);
    assert!(
        took <= BASE_WAIT + OVERLAP_BUDGET,
        "TMC005-CA2: the answer comes within BASE_WAIT + OVERLAP_BUDGET, took {took:?}"
    );
    assert_eq!(
        r.head(),
        first,
        "TMC005-CA2: nothing changed: the branch stays"
    );
    assert_eq!(r.read("api.rs").as_deref(), Some("v1\n"));
    assert_eq!(r.f.git(&["status", "--porcelain"]), status_before);
    r.assert_rejection_recorded(&err);
    let waits = r.base_waits();
    assert!(
        waits.iter().any(|(ms, result)| result == "expired"
            && u128::try_from(*ms).unwrap_or(0) >= BASE_WAIT.as_millis()),
        "TMC005-CA2: the undo waits BASE_WAIT for its base and logs tm_base_wait \
         result=expired with base_wait_ms >= {} ms; logged: {waits:?}",
        BASE_WAIT.as_millis()
    );
}

// --- CA-3 ---

#[test]
fn ca3_a_capture_given_up_after_max_retries_is_not_waited_for() {
    let r = rig();
    let (first, _, _) = agent_history(&r);
    let reset = r.reset(&first, agent());
    let deps = r.deps(vec![reset], Some(discarding(RESET_SEQ)));
    let capture = r.capture(&deps);
    // The capture gives the reset up after MAX_RETRIES discarded tries; the seals say when.
    let _ = r
        .seals
        .await_bases(REPO, &[RESET_SEQ], Duration::from_secs(60));
    let git = system_git();

    let started = Instant::now();
    let out = r.undo(Some(&deps), &agent_who(), Selection::Last, &git);
    let took = started.elapsed();

    capture.stop();
    let err = out.expect_err("TMC005-CA3: without its base the undo is refused (D7)");
    assert_eq!(reason(&err), TmRejectReason::OverlapUnverified);
    assert!(
        took < BASE_WAIT,
        "TMC005-CA3: no capture is pending: the undo does not wait, took {took:?}"
    );
    let waits = r.base_waits();
    assert!(
        waits
            .iter()
            .any(|(ms, result)| result == "not-pending" && (0..100).contains(ms)),
        "TMC005-CA3: the undo logs tm_base_wait result=not-pending with base_wait_ms < 100; \
         logged: {waits:?}"
    );
}

#[test]
fn ca3_without_an_engine_the_undo_does_not_wait() {
    let r = rig();
    // The agent's own operation with its prior and no anchor: it has no base point.
    std::fs::write(r.f.repo.join("since.txt"), "agent").unwrap();
    let key = r.worktree().to_string_lossy().into_owned();
    let oplog = &r.repo.repo.oplog;
    let id = oplog
        .lock()
        .unwrap()
        .record_operation(
            &NewOperation {
                kind: OperationKind::Protected,
                subtype: Some("reset-hard".into()),
                scope: Scope {
                    worktrees: vec![key],
                    refs: Vec::new(),
                },
                requester: agent(),
                channel: Channel::Cli,
                confirmed: true,
                target: Target::None,
                warnings: Vec::new(),
                engine_mark: 1_000,
            },
            now_ms(),
        )
        .unwrap();
    let prior = r.point(1_000);
    {
        let mut log = oplog.lock().unwrap();
        for step in [
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior,
            },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            OperationTransition::Finished,
        ] {
            log.advance_operation(&id, step, now_ms()).unwrap();
        }
    }
    let git = system_git();

    let started = Instant::now();
    let out = r.undo(None, &agent_who(), Selection::Last, &git);
    let took = started.elapsed();

    let err = out.expect_err("TMC005-CA3: without its base the undo is refused (D7)");
    assert_eq!(reason(&err), TmRejectReason::OverlapUnverified);
    assert!(
        took < BASE_WAIT,
        "TMC005-CA3: without an engine there is no capture to wait for, took {took:?}"
    );
}

// --- CA-4 ---

#[test]
fn ca4_an_unattributed_reference_never_waits() {
    let r = rig();
    let (first, second, _) = agent_history(&r);
    let reset = r.reset(&first, Requester::Unattributed);
    let (layer, release) = holding(RESET_SEQ, LONG_HOLD, || {});
    let deps = r.deps(vec![reset], Some(layer));
    let capture = r.capture(&deps);
    let git = system_git();

    let out = r.undo(Some(&deps), &Who::unattributed(), Selection::Last, &git);

    drop(release);
    capture.stop();
    let waits = r.base_waits();
    assert!(
        waits.iter().all(|(ms, _)| *ms <= 0),
        "TMC005-CA4: an unattributed undo never waits: no tm_base_wait with base_wait_ms > 0; \
         logged: {waits:?}"
    );
    out.unwrap_or_else(|e| {
        panic!("TMC005-CA4: an unattributed reference needs no B (D7): the undo runs, got {e:?}")
    });
    assert_eq!(r.head(), second);
}

// --- CA-5 ---

#[test]
fn ca5_the_wait_holds_neither_the_oplog_lock_nor_the_store_writer() {
    let r = rig();
    let (first, second, _) = agent_history(&r);
    let reset = r.reset(&first, agent());
    let free: Arc<Mutex<Option<bool>>> = Arc::default();
    let seen = Arc::clone(&free);
    let repos = Arc::clone(&r.repos);
    // At the end of the hold the undo is waiting: the oplog lock must be free then.
    let (layer, release) = holding(RESET_SEQ, HOLD, move || {
        let (oplog, _) = repos.repo(REPO).expect("the repo is observed");
        let is_free = oplog.try_lock().is_ok();
        *seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(is_free);
    });
    let deps = r.deps(vec![reset], Some(layer));
    let capture = r.capture(&deps);
    let git = system_git();

    let out = r.undo(Some(&deps), &agent_who(), Selection::Last, &git);

    drop(release);
    capture.stop();
    let done = out.unwrap_or_else(|e| {
        panic!(
            "TMC005-CA5: the capture seals its point while the undo waits (no deadlock on the \
             oplog lock or the store writer), so the undo finishes; got {e:?}"
        )
    });
    assert_eq!(
        *free.lock().unwrap(),
        Some(true),
        "TMC005-CA5: the oplog lock is free while the undo waits"
    );
    assert_eq!(
        r.caused_by_reset().len(),
        1,
        "TMC005-CA5: the capture sealed the reset's point while the undo waited"
    );
    assert!(!done.prior_snapshot_id.is_empty());
    assert_eq!(r.head(), second);
}

// --- CA-6 ---

#[test]
fn ca6_the_base_after_waiting_is_the_one_a_later_retry_gets() {
    let r = rig();
    let (first, second, before) = agent_history(&r);
    let reset = r.reset(&first, agent());
    let (layer, release) = holding(RESET_SEQ, HOLD, || {});
    let deps = r.deps(vec![reset], Some(layer));
    let capture = r.capture(&deps);
    let git = system_git();

    let out = r.undo(Some(&deps), &agent_who(), Selection::Last, &git);

    drop(release);
    capture.stop();
    let done = out.unwrap_or_else(|e| {
        panic!("TMC005-CA6: the undo waits for its base and finishes, got {e:?}")
    });
    // A retry after the capture settled takes, by `base_point_of`, the capture the reset caused:
    // on this history there is exactly one.
    let caused = r.caused_by_reset();
    assert_eq!(
        caused.len(),
        1,
        "TMC005-CA6: one capture caused by the reset, the B a retry gets: {caused:?}"
    );
    let (base, base_seq) = caused[0].clone();
    assert_ne!(
        base, done.prior_snapshot_id,
        "TMC005-CA6: B is never the undo's own prior (it would undo too much, NFR-01)"
    );
    assert!(
        base_seq < r.snapshot_seq(&done.prior_snapshot_id),
        "TMC005-CA6: B was sealed before the undo took its prior: the undo planned on the \
         same B a retry would"
    );
    assert_eq!(done.target_snapshot_id, before);
    assert_eq!(r.head(), second, "TMC005-CA6: exactly the reset is undone");
    assert_eq!(r.read("api.rs").as_deref(), Some("v2\n"));
    assert_eq!(r.read("keep.rs").as_deref(), Some("fn keep() {}\n"));
}

// --- CA-7 ---

const FORBIDDEN: [&str; 3] = ["sleep", "park_timeout", "try_recv"];
const BEGIN: &str = "// base-wait: begin";
const END: &str = "// base-wait: end";

/// The text between the fixed markers of `file`: each exactly once, in order, not empty.
fn wait_block<'s>(name: &str, file: &'s str) -> &'s str {
    assert_eq!(
        file.matches(BEGIN).count(),
        1,
        "TMC005-CA7: `{BEGIN}` appears exactly once in {name}"
    );
    assert_eq!(
        file.matches(END).count(),
        1,
        "TMC005-CA7: `{END}` appears exactly once in {name}"
    );
    let start = file.find(BEGIN).unwrap() + BEGIN.len();
    let end = file.find(END).unwrap();
    assert!(
        start <= end,
        "TMC005-CA7: `{BEGIN}` comes before `{END}` in {name}"
    );
    let block = &file[start..end];
    assert!(
        !block.trim().is_empty(),
        "TMC005-CA7: the wait block of {name} is not empty"
    );
    block
}

#[test]
fn ca7_the_wait_path_has_no_sleep_nor_polling() {
    let base_wait = include_str!("base_wait.rs");
    let undo = wait_block("undo.rs", include_str!("undo.rs"));
    let undo_agent = wait_block("undo_agent.rs", include_str!("undo_agent.rs"));
    for (name, text) in [
        ("base_wait.rs", base_wait),
        ("undo.rs (wait block)", undo),
        ("undo_agent.rs (wait block)", undo_agent),
    ] {
        for word in FORBIDDEN {
            assert!(
                !text.contains(word),
                "TMC005-CA7: {name} waits on an explicit signal, never `{word}`"
            );
        }
    }
}
