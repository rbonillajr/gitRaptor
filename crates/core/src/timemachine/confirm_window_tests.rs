//! A capture's confirmation waits a short window for the engine to be calm again at its mark
//! (fix/tm-restore-calm-mark, D9 with `CONFIRM_WINDOW`): a momentary not-calm that clears keeps
//! the capture at its original mark; a mark that moves within the window still discards it.
//!
//! A testkit fixture and a synthetic process table whose answers follow the number of times it is
//! read, one read per poll of the calm; never this repo or the real profile (NFR-01). The test
//! sleeps nowhere: the polls are the engine's own.
//!
//! The blip clears after a number of reads, not after a time: between reads the engine waits
//! `PROBE_BACKOFF`, and on a loaded runner those reads do not fit in the real `CONFIRM_WINDOW`
//! (the capture was discarded in CI, `Err(Discarded)`). The engine here confirms over a window
//! long enough for any runner; a calm poll still decides at once, so no test waits it out.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::CalmExclusions;
use super::calm_git_alive_tests::{GIT, Marks, Procs, REPO, Rig};
use super::*;
use crate::daemon::{LogLimits, Logger};
use crate::detect::{GitCall, ProcEntry, ProcLister, git_call};
use crate::profile::ProfileDirs;
use crate::timemachine::continuous::CaptureDeps;
use crate::timemachine::manual::{self, ManualAsk, ManualError};
use crate::timemachine::oplog::{
    Channel, Oplog, Requester, RequesterOrigin, SnapshotFilter, SnapshotLevel, SnapshotState,
};
use crate::timemachine::protected::TmRepos;

/// One `git status` in the worktree, present at the reads in `busy`; at read `bump_at` the repo's
/// mark moves too (an event persisted).
struct Blip {
    reads: AtomicUsize,
    busy: Range<usize>,
    bump_at: Option<usize>,
    marks: Arc<RepoMarks>,
    cwd: PathBuf,
}

impl ProcLister for Blip {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if self.bump_at == Some(read) {
            self.marks.set_seq(REPO, self.marks.seq(REPO) + 1);
        }
        Some(if self.busy.contains(&read) {
            vec![ProcEntry {
                pid: 130,
                ppid: 1,
                start_us: 130,
                exe: Some(PathBuf::from(GIT)),
            }]
        } else {
            Vec::new()
        })
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        Some(self.cwd.clone())
    }
    fn git_redirect(&self, _pid: u32) -> Option<bool> {
        Some(false)
    }
    fn git_call(&self, _pid: u32) -> Option<GitCall> {
        Some(git_call(&[std::ffi::OsString::from("status")]))
    }
}

/// The blip of these tests (three busy reads, `PROBE_BACKOFF` apart, then a calm one) fits in the
/// real `CONFIRM_WINDOW` on a machine that gets the CPU: [`PATIENT_WINDOW`] only absorbs a
/// starved runner's delays.
const _: () = assert!(3 * PROBE_BACKOFF.as_millis() < CONFIRM_WINDOW.as_millis());

/// How long [`Patient`] confirms: far over any runner's scheduling delay, never waited out.
const PATIENT_WINDOW: Duration = Duration::from_secs(30);

/// The daemon's link ([`Marks`]) whose confirmation is the default one, `settle_excluding` at the
/// sealed mark, over [`PATIENT_WINDOW`] instead of `CONFIRM_WINDOW`.
struct Patient(Marks);

impl EngineLink for Patient {
    fn mark(&self, repo_id: &str) -> i64 {
        self.0.mark(repo_id)
    }
    fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        self.0.settle(repo_id, worktrees, limit)
    }
    fn raw_events(&self, repo_id: &str, worktree: &Path) -> Option<Vec<RawGitEvent>> {
        self.0.raw_events(repo_id, worktree)
    }
    fn generation_floor(&self, repo_id: &str) -> i64 {
        self.0.generation_floor(repo_id)
    }
    fn activity(&self, repo_id: &str) -> u64 {
        self.0.activity(repo_id)
    }
    fn settle_excluding(
        &self,
        repo_id: &str,
        worktrees: &[PathBuf],
        limit: Duration,
        ex: &CalmExclusions<'_>,
    ) -> Option<i64> {
        self.0.settle_excluding(repo_id, worktrees, limit, ex)
    }
    fn hook_begin(&self, repo_id: &str, worktree: &Path) -> HookInFlight {
        self.0.hook_begin(repo_id, worktree)
    }
    fn confirm(
        &self,
        repo_id: &str,
        worktrees: &[PathBuf],
        mark: i64,
        ex: &CalmExclusions<'_>,
    ) -> bool {
        self.0
            .settle_excluding(repo_id, worktrees, PATIENT_WINDOW, ex)
            == Some(mark)
    }
}

/// A manual capture of a repo whose engine persisted everything at mark 1, with the process
/// table `blip(worktree, marks)`; its result and the manual points kept at mark 1.
fn manual_capture(
    blip: impl FnOnce(&Path, &Arc<RepoMarks>) -> Blip,
) -> (Result<manual::ManualCaptured, ManualError>, usize) {
    let f = gitraptor_testkit::Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let r = Rig {
        common: gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap(),
        wt: f.repo.clone(),
        f,
        marks: Arc::new(RepoMarks::default()),
        procs: Arc::new(Procs::default()),
    };
    r.publish(1);
    r.marks.set_procs(Arc::new(blip(&r.wt, &r.marks)));
    let dirs = ProfileDirs::under_root(&r.f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    repos.insert(REPO, &r.common, Oplog::open(&dirs, REPO, 1).unwrap().0);
    let deps = CaptureDeps {
        repos: Arc::clone(&repos),
        engine: Arc::new(Patient(Marks(Arc::clone(&r.marks)))),
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
        after_mark: None,
        seals: Default::default(),
    };
    let ask = ManualAsk {
        repo_id: REPO.into(),
        worktree: r.wt.clone(),
        common_dir: r.common.clone(),
        label: "before the migration".into(),
        requester: Requester::Agent {
            name: "claude-code".into(),
            origin: RequesterOrigin::Detected,
            session_id: "s1".into(),
        },
        channel: Channel::Mcp,
    };
    let out = manual::capture(&deps, &ask, 1_728_000_000_000);
    let kept = repos
        .oplog(REPO)
        .unwrap()
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .iter()
        .filter(|s| {
            s.record.level == SnapshotLevel::Manual
                && s.state == SnapshotState::Complete
                && s.record.engine_mark == Some(1)
        })
        .count();
    (out, kept)
}

/// The mark is taken at read 1; a `git` is alive at the confirmation's first polls (reads 2 to 4)
/// and gone at the next one: the capture holds, at its original mark.
#[test]
fn a_momentary_not_calm_within_the_window_keeps_the_capture_at_its_mark() {
    let (out, kept) = manual_capture(|wt, marks| Blip {
        reads: AtomicUsize::new(0),
        busy: 2..5,
        bump_at: None,
        marks: Arc::clone(marks),
        cwd: wt.to_path_buf(),
    });
    assert!(out.is_ok(), "a blip that clears is not a change: {out:?}");
    assert_eq!(kept, 1, "the point is kept at mark 1");
}

/// The same blip, but an event is persisted during it (read 2): once calm again, the mark is
/// another one, and the capture is discarded.
#[test]
fn a_mark_that_moves_within_the_window_discards_the_capture() {
    let (out, kept) = manual_capture(|wt, marks| Blip {
        reads: AtomicUsize::new(0),
        busy: 2..5,
        bump_at: Some(2),
        marks: Arc::clone(marks),
        cwd: wt.to_path_buf(),
    });
    assert!(
        matches!(out, Err(ManualError::Discarded)),
        "a moved mark never seals: {out:?}"
    );
    assert_eq!(kept, 0, "nothing kept at mark 1");
}
