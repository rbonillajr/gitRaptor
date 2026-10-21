//! A capture's mark is confirmed at its validity point (fix/tm-restore-calm-mark, case 2): a
//! capture sealed with an engine mark never holds content from after an event the mark does not
//! cover.
//!
//! Every test runs on a testkit fixture (temporary repo, home and profile; Git with the fixture's
//! home and no system config), never this repo or the real profile (NFR-01). The engine is a real
//! `RepoMarks`; the `git` between the mark and the read is injected at a fixed point (the
//! `after_mark` hook, or the engine's own `settle` for the paths without that hook), so nothing
//! waits on time.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gitraptor_api::messages::GitEventKind;
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;

use super::*;
use crate::daemon::{LOG_FILE, LogLimits};
use crate::timemachine::engine::{RawGitEvent, RepoMarks, git_dir_of};
use crate::timemachine::hook_prior::{self, HookPriorAsk};
use crate::timemachine::manual::{self, ManualAsk};
use crate::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationTransition, Oplog, Requester, RequesterOrigin,
    Scope, SnapshotFilter, SnapshotState, SnapshotView, Target,
};
use crate::timemachine::undo::{RawSide, external_events_in};

const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const T0: i64 = 1_728_000_000_000;

/// The engine as the daemon links it: marks and calm from a real `RepoMarks`.
struct Marks(Arc<RepoMarks>);

impl EngineLink for Marks {
    fn mark(&self, repo_id: &str) -> i64 {
        self.0.seq(repo_id)
    }
    fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        self.0.settle(repo_id, worktrees, limit)
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(Vec::new())
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, repo_id: &str) -> u64 {
        self.0.activity(repo_id)
    }
}

/// [`Marks`], whose first `settle` runs `inject` after it took its answer: the paths that have no
/// `after_mark` hook (manual, hook prior) get the same `git` between the mark and the read.
struct InjectOnce {
    marks: Marks,
    inject: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl EngineLink for InjectOnce {
    fn mark(&self, repo_id: &str) -> i64 {
        self.marks.mark(repo_id)
    }
    fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        let mark = self.marks.settle(repo_id, worktrees, limit);
        let inject = self.inject.lock().unwrap().take();
        if let Some(inject) = inject {
            inject();
        }
        mark
    }
    fn raw_events(&self, repo_id: &str, worktree: &Path) -> Option<Vec<RawGitEvent>> {
        self.marks.raw_events(repo_id, worktree)
    }
    fn generation_floor(&self, repo_id: &str) -> i64 {
        self.marks.generation_floor(repo_id)
    }
    fn activity(&self, repo_id: &str) -> u64 {
        self.marks.activity(repo_id)
    }
}

/// What the engine persisted, published to the marks: the branches and each worktree's `HEAD`
/// and reflog size, read now.
#[derive(Clone)]
struct Engine {
    marks: Arc<RepoMarks>,
    common: PathBuf,
    wt: PathBuf,
}

impl Engine {
    fn publish(&self, seq: i64) {
        let reader = RepoReader::open(&self.common, &ReaderOptions::default()).unwrap();
        let branches: BTreeMap<String, String> =
            crate::watch::read_local_branches(&reader).unwrap();
        self.marks.set_persisted(REPO, seq, &self.common, branches);
        let git_dir = git_dir_of(&self.wt).unwrap();
        let len = std::fs::metadata(git_dir.join("logs/HEAD")).map_or(0, |m| m.len());
        self.marks.set_head_logs(REPO, &[(self.wt.clone(), len)]);
        self.marks.set_heads(
            REPO,
            &[(
                self.wt.clone(),
                std::fs::read(git_dir.join("HEAD")).unwrap(),
            )],
        );
    }

    /// An event of another worktree persisted: the repo's mark moves, its calm holds.
    fn bump(&self) {
        self.marks.set_seq(REPO, self.marks.seq(REPO) + 1);
    }
}

struct Rig {
    f: Arc<Fixture>,
    dirs: ProfileDirs,
    repos: Arc<TmRepos>,
    engine: Engine,
}

fn rig() -> Rig {
    let f = Arc::new(Fixture::with_commit(
        &gitraptor_testkit::fixture::git_from_path(),
    ));
    let dirs = ProfileDirs::under_root(&f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    repos.insert(REPO, &common, oplog);
    let engine = Engine {
        marks: Arc::new(RepoMarks::default()),
        common,
        wt: f.repo.clone(),
    };
    // The agent's work, not committed: what a raw `reset --hard` loses.
    f.write("a.txt", "agent work\n");
    engine.publish(1);
    Rig {
        f,
        dirs,
        repos,
        engine,
    }
}

impl Rig {
    fn deps(&self, engine: Arc<dyn EngineLink>, after_mark: Option<Hook>) -> CaptureDeps {
        CaptureDeps {
            repos: Arc::clone(&self.repos),
            engine,
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer: None,
            free_space_floor: None,
            after_mark,
        }
    }

    fn marks(&self) -> Arc<dyn EngineLink> {
        Arc::new(Marks(Arc::clone(&self.engine.marks)))
    }

    fn observe(&self, deps: &CaptureDeps) -> Result<CaptureOutcome, Failure> {
        observe(
            deps,
            REPO,
            std::slice::from_ref(&self.engine.wt),
            None,
            None,
            Duration::ZERO,
            false,
        )
    }

    fn snapshots(&self) -> Vec<SnapshotView> {
        self.repos
            .oplog(REPO)
            .unwrap()
            .lock()
            .unwrap()
            .snapshots(&SnapshotFilter::default())
            .unwrap()
    }

    /// Complete points sealed with `mark`.
    fn complete_at(&self, mark: i64) -> Vec<SnapshotView> {
        self.snapshots()
            .into_iter()
            .filter(|s| s.state == SnapshotState::Complete && s.record.engine_mark == Some(mark))
            .collect()
    }

    fn complete_observations(&self) -> usize {
        self.snapshots()
            .iter()
            .filter(|s| {
                s.state == SnapshotState::Complete && s.record.level == SnapshotLevel::Observation
            })
            .count()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dirs.state.join(LOG_FILE)).unwrap_or_default()
    }

    /// The raw `reset --hard` that drops the agent's work.
    fn reset(&self) -> impl Fn() + Send + Sync + 'static {
        let f = Arc::clone(&self.f);
        move || {
            f.git(&["reset", "-q", "--hard"]);
        }
    }
}

type Hook = Arc<dyn Fn() + Send + Sync>;

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

/// C10: the reset is persisted between the mark and the read (the mark moves to 2).
#[test]
fn an_event_persisted_between_mark_and_read_discards_the_observation() {
    let r = rig();
    let (reset, engine) = (r.reset(), r.engine.clone());
    let hook: Hook = Arc::new(move || {
        reset();
        engine.publish(2);
    });
    let out = r.observe(&r.deps(r.marks(), Some(hook)));
    assert!(
        matches!(out, Err(Failure::Capture(CaptureError::Discarded))),
        "a capture whose mark moved before its read is discarded: {out:?}"
    );
    assert!(
        r.complete_at(1).is_empty(),
        "no point holds the reset's content with the mark before it"
    );
}

/// C11: the reset is on disk but not persisted yet (the mark stays 1, the reflog moved on).
#[test]
fn an_event_not_yet_persisted_between_mark_and_read_discards_the_observation() {
    let r = rig();
    let reset = r.reset();
    let hook: Hook = Arc::new(reset);
    let out = r.observe(&r.deps(r.marks(), Some(hook)));
    assert!(
        matches!(out, Err(Failure::Capture(CaptureError::Discarded))),
        "a capture whose engine is no longer calm at its read is discarded: {out:?}"
    );
    assert!(r.complete_at(1).is_empty());
}

/// C12 (guard): without a `git` in between, the capture holds with its mark.
#[test]
fn a_calm_unmoved_mark_still_captures() {
    let r = rig();
    let out = r.observe(&r.deps(r.marks(), None));
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(r.complete_at(1).len(), 1);
}

/// C13: the manual and the hook-prior captures confirm their mark too. The hook prior tries again
/// while it has time, so it may end with a point; never one sealed with the mark before the reset.
#[test]
fn manual_and_hook_prior_captures_confirm_their_mark() {
    // Manual.
    let r = rig();
    let (reset, engine) = (r.reset(), r.engine.clone());
    let inject = InjectOnce {
        marks: Marks(Arc::clone(&r.engine.marks)),
        inject: Mutex::new(Some(Box::new(move || {
            reset();
            engine.publish(2);
        }))),
    };
    let deps = r.deps(Arc::new(inject), None);
    let ask = ManualAsk {
        repo_id: REPO.into(),
        worktree: r.engine.wt.clone(),
        common_dir: r.engine.common.clone(),
        label: "before the migration".into(),
        requester: agent("s1"),
        channel: Channel::Mcp,
    };
    let out = manual::capture(&deps, &ask, T0);
    assert!(
        format!("{out:?}").contains("Discarded"),
        "a manual capture whose mark moved is discarded: {out:?}"
    );
    assert!(r.complete_at(1).is_empty(), "manual: no point at mark 1");

    // Hook prior.
    let r = rig();
    let (reset, engine) = (r.reset(), r.engine.clone());
    let inject = InjectOnce {
        marks: Marks(Arc::clone(&r.engine.marks)),
        inject: Mutex::new(Some(Box::new(move || {
            reset();
            engine.publish(2);
        }))),
    };
    let deps = r.deps(Arc::new(inject), None);
    let ask = HookPriorAsk {
        repo_id: REPO.into(),
        worktree: r.engine.wt.clone(),
        common_dir: r.engine.common.clone(),
        requester: agent("s1"),
        git: None,
    };
    let out = hook_prior::capture(&deps, &ask, T0, Instant::now() + Duration::from_secs(25));
    assert!(
        r.complete_at(1).is_empty(),
        "hook prior: no point holds the reset's content with the mark before it: {out:?}"
    );
}

/// C16: the mark is the repo's, so a sibling worktree's events discard this one's captures. A
/// bounded burst still lets an observation through before the retries run out; an endless one
/// gives up once, with `discarded-exhausted`. The sibling's events are injected at each attempt's
/// mark (the `after_mark` hook), so the burst is counted in attempts, not in time.
#[test]
fn sibling_activity_still_lets_an_observation_through_within_a_bound() {
    const BURST: usize = 3;
    let config = CaptureConfig {
        enabled: true,
        quiet: Duration::from_millis(10),
        max_interval: Duration::from_millis(50),
    };

    // A bounded burst.
    let r = rig();
    r.observe(&r.deps(r.marks(), None))
        .expect("the seed observation");
    let seeded = r.complete_observations();
    let (engine, calls) = (r.engine.clone(), Arc::new(AtomicUsize::new(0)));
    let counted = Arc::clone(&calls);
    let hook: Hook = Arc::new(move || {
        if counted.fetch_add(1, Ordering::SeqCst) < BURST {
            engine.bump();
        }
    });
    let started = Instant::now();
    let capture = ContinuousCapture::start(config, r.deps(r.marks(), Some(hook))).unwrap();
    capture.activity(REPO, &r.engine.wt);
    let bound = SETTLE * MAX_RETRIES;
    while r.complete_observations() == seeded {
        assert!(
            started.elapsed() < bound,
            "no observation within {bound:?} ({} attempts)",
            calls.load(Ordering::SeqCst)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    capture.stop();
    let last = r
        .snapshots()
        .into_iter()
        .filter(|s| s.record.level == SnapshotLevel::Observation)
        .filter_map(|s| s.record.engine_mark)
        .max();
    assert_eq!(
        last,
        Some(1 + BURST as i64),
        "the observation that got through is sealed after the burst"
    );

    // An endless burst.
    let r = rig();
    r.observe(&r.deps(r.marks(), None))
        .expect("the seed observation");
    let seeded = r.complete_observations();
    let engine = r.engine.clone();
    let hook: Hook = Arc::new(move || engine.bump());
    let capture = ContinuousCapture::start(config, r.deps(r.marks(), Some(hook))).unwrap();
    capture.activity(REPO, &r.engine.wt);
    let started = Instant::now();
    while !r.log().contains("discarded-exhausted") {
        assert_eq!(
            r.complete_observations(),
            seeded,
            "no observation is sealed while the sibling never stops"
        );
        assert!(
            started.elapsed() < bound * 2,
            "the retries never ran out:\n{}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    capture.stop();
    assert_eq!(r.log().matches("discarded-exhausted").count(), 1);
}

/// C17: an anchor that cannot be confirmed in its three attempts leaves the operation's echo with
/// it: the events after the operation's mark, up to the next capture, are caused by it.
#[test]
fn a_failed_anchor_leaves_the_echo_with_its_operation() {
    let r = rig();
    let key = r.engine.wt.display().to_string();
    let (oplog, store) = r.repos.repo(REPO).unwrap();
    let store = store.unwrap();
    // The protected operation, at mark 1, finished.
    let op = oplog
        .lock()
        .unwrap()
        .record_operation(
            &NewOperation {
                kind: OperationKind::Protected,
                subtype: None,
                scope: Scope {
                    worktrees: vec![key.clone()],
                    refs: vec![],
                },
                requester: Requester::Unattributed,
                channel: Channel::Cli,
                confirmed: true,
                target: Target::None,
                warnings: vec![],
                engine_mark: 1,
            },
            10,
        )
        .unwrap();
    let prior = store
        .capture(
            &oplog,
            &CaptureRequest {
                level: SnapshotLevel::GuaranteedPrior,
                common_dir: r.engine.common.clone(),
                worktrees: vec![WorktreeScope {
                    key: "main".into(),
                    path: r.engine.wt.clone(),
                    hint: None,
                }],
                engine_mark: Some(1),
                cause_operation: Some(op.clone()),
                cause_event_seq: None,
                include_credentials: false,
                still_valid: None,
                give_way: None,
            },
        )
        .unwrap()
        .snapshot_id;
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
        log.advance_operation(&op, OperationTransition::Applying { step: 1 }, 13)
            .unwrap();
        log.advance_operation(&op, OperationTransition::Finished, 14)
            .unwrap();
    }
    // Its echo lands at every attempt of its anchor.
    let engine = r.engine.clone();
    let hook: Hook = Arc::new(move || engine.bump());
    let anchored = anchor(
        &r.deps(r.marks(), Some(hook)),
        REPO,
        std::slice::from_ref(&r.engine.wt),
        &op,
    );
    assert_eq!(anchored, None, "no attempt of the anchor holds its mark");
    // The next capture of the worktree, with the echo persisted.
    r.observe(&r.deps(r.marks(), None))
        .expect("the next observation");
    let last = r.engine.marks.seq(REPO);
    let raw = RawSide {
        events: (2..=last)
            .map(|seq| RawGitEvent {
                seq,
                kind: GitEventKind::BranchUpdate,
                branch: Some("main".into()),
                actor: Requester::Unattributed,
            })
            .collect(),
        floor: 0,
    };
    let log = oplog.lock().unwrap();
    let ops = log.operations(&Default::default()).unwrap();
    let snaps = log.snapshots(&SnapshotFilter::default()).unwrap();
    let wt_key = snaps
        .iter()
        .find(|s| s.record.level == SnapshotLevel::Observation)
        .and_then(|s| s.record.worktrees.first().cloned())
        .unwrap();
    for e in external_events_in(&ops, &snaps, &raw, &key, &wt_key) {
        assert_eq!(
            e.caused_by.as_deref(),
            Some(op.as_str()),
            "event {} is the operation's echo",
            e.seq
        );
    }
}
