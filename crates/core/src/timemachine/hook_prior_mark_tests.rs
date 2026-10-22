//! A hook prior whose mark is not confirmed tries again while it has time (fix/tm-restore-calm-mark,
//! D11): one discard is never a denial; only the deadline is.
//!
//! Every test runs on a testkit fixture (temporary repo, home and profile), never this repo or the
//! real profile (NFR-01). The engine is a real `RepoMarks` behind a link whose `settle` moves the
//! repo's mark after it answers, a counted number of times: an event of another worktree persisted
//! between the mark and its confirmation. The deadline is the engine's too: its K-th wait answers
//! as a `settle` whose time ran out, while the capture's own deadline is far away. Nothing depends
//! on how long a capture takes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize};

use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;

use super::*;
use crate::daemon::{LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::engine::{EngineLink, RawGitEvent, RepoMarks, git_dir_of};
use crate::timemachine::oplog::RequesterOrigin;
use crate::timemachine::protected::TmRepos;

const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const T0: i64 = 1_728_000_000_000;

/// `RepoMarks` whose `settle` moves the mark after answering, `bumps` times (`-1`: always). A
/// `settle` for the mark is a wait; one over `CONFIRM_WINDOW` is a confirmation. The wait number
/// `expire_at` (`0`: never) answers `None`, as a `settle` whose remaining time ran out.
struct Moving {
    marks: Arc<RepoMarks>,
    bumps: AtomicI64,
    expire_at: usize,
    waits: AtomicUsize,
    confirmations: AtomicUsize,
}

impl EngineLink for Moving {
    fn mark(&self, repo_id: &str) -> i64 {
        self.marks.seq(repo_id)
    }
    fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        if limit == crate::timemachine::engine::CONFIRM_WINDOW {
            self.confirmations.fetch_add(1, Ordering::SeqCst);
        } else {
            let wait = self.waits.fetch_add(1, Ordering::SeqCst) + 1;
            if wait == self.expire_at {
                return None;
            }
        }
        let mark = self.marks.settle(repo_id, worktrees, limit);
        let left = self.bumps.load(Ordering::SeqCst);
        if left != 0 {
            if left > 0 {
                self.bumps.store(left - 1, Ordering::SeqCst);
            }
            self.marks.set_seq(repo_id, self.marks.seq(repo_id) + 1);
        }
        mark
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(Vec::new())
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, repo_id: &str) -> u64 {
        self.marks.activity(repo_id)
    }
}

struct Rig {
    _f: Fixture,
    deps: CaptureDeps,
    engine: Arc<Moving>,
    ask: HookPriorAsk,
    oplog: Arc<Mutex<Oplog>>,
}

fn rig(bumps: i64, expire_at: usize) -> Rig {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let dirs = ProfileDirs::under_root(&f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    repos.insert(REPO, &common, oplog);
    let marks = Arc::new(RepoMarks::default());
    let reader = RepoReader::open(&common, &ReaderOptions::default()).unwrap();
    let branches = crate::watch::read_local_branches(&reader).unwrap();
    marks.set_persisted(REPO, 1, &common, branches);
    let git_dir = git_dir_of(&f.repo).unwrap();
    let len = std::fs::metadata(git_dir.join("logs/HEAD")).map_or(0, |m| m.len());
    marks.set_head_logs(REPO, &[(f.repo.clone(), len)]);
    marks.set_heads(
        REPO,
        &[(f.repo.clone(), std::fs::read(git_dir.join("HEAD")).unwrap())],
    );
    let engine = Arc::new(Moving {
        marks,
        bumps: AtomicI64::new(bumps),
        expire_at,
        waits: AtomicUsize::new(0),
        confirmations: AtomicUsize::new(0),
    });
    let oplog = repos.oplog(REPO).unwrap();
    let deps = CaptureDeps {
        repos,
        engine: Arc::clone(&engine) as Arc<dyn EngineLink>,
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
        after_mark: None,
    };
    let ask = HookPriorAsk {
        repo_id: REPO.into(),
        worktree: f.repo.clone(),
        common_dir: common,
        requester: Requester::Agent {
            name: "claude-code".into(),
            origin: RequesterOrigin::Detected,
            session_id: "s1".into(),
        },
        git: None,
    };
    Rig {
        _f: f,
        deps,
        engine,
        ask,
        oplog,
    }
}

impl Rig {
    fn waits(&self) -> usize {
        self.engine.waits.load(Ordering::SeqCst)
    }

    fn confirmations(&self) -> usize {
        self.engine.confirmations.load(Ordering::SeqCst)
    }

    /// Hook priors kept as points.
    fn complete(&self) -> usize {
        self.oplog
            .lock()
            .unwrap()
            .snapshots(&crate::timemachine::oplog::SnapshotFilter::default())
            .unwrap()
            .iter()
            .filter(|s| {
                s.record.level == SnapshotLevel::HookPrior
                    && s.state == crate::timemachine::oplog::SnapshotState::Complete
            })
            .count()
    }
}

/// C18: one discard followed by calm is a prior; sustained discards are `TimeLimit`, and only
/// once the deadline has passed: the prior keeps taking a new mark while its wait has time.
#[test]
fn a_hook_prior_denies_by_deadline_not_by_one_discard() {
    // The capture's own deadline never comes during the test; the engine says when time ran out.
    let far = || Instant::now() + Duration::from_secs(3600);

    // One discard, then calm: a second mark, confirmed, and a point.
    let r = rig(1, 0);
    let out = capture(&r.deps, &r.ask, T0, far());
    assert!(out.is_ok(), "one discard is never a denial: {out:?}");
    assert_eq!(r.waits(), 2, "a new mark after the discard");
    assert_eq!(
        r.confirmations(),
        2,
        "the first one discarded, the second one held"
    );
    assert_eq!(r.complete(), 1);

    // Discards on every attempt, until the fourth wait finds its time run out.
    const EXPIRES_AT: usize = 4;
    let r = rig(-1, EXPIRES_AT);
    let out = capture(&r.deps, &r.ask, T0, far());
    assert!(
        matches!(out, Err(HookPriorError::TimeLimit)),
        "sustained discards end at the deadline: {out:?}"
    );
    assert_eq!(
        r.waits(),
        EXPIRES_AT,
        "the prior took a new mark after every discard, until its time ran out"
    );
    assert_eq!(
        r.confirmations(),
        EXPIRES_AT - 1,
        "every attempt before the deadline was confirmed and discarded"
    );
    assert_eq!(r.complete(), 0, "no point was kept");
}
