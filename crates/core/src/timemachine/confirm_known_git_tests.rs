//! The confirmation window absorbs only a not-calm whose cause is unknown (fix/tm-restore-calm-mark,
//! D9): a `git` known to work in this repo (its folder in the worktree, or a redirection resolved
//! into it) seen at any poll of the window discards the capture, even if it exits before the
//! window ends (`git apply` changes files only: no reflog, no ref, no index). A `git` whose scope
//! cannot be read (the CI blip) may still be absorbed. And two details of `git_call`: a read-only
//! subcommand that writes a file (`--output`) is not read-only, and `--attr-source <tree-ish>`
//! takes its value like the other global options.
//!
//! A testkit fixture and a synthetic process table whose answers follow the number of times it is
//! read, one read per poll; never this repo or the real profile (NFR-01). Nothing sleeps.

use std::ffi::OsString;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::calm_git_alive_tests::{GIT, Marks, Procs, REPO, Rig};
use super::*;
use crate::daemon::{LogLimits, Logger};
use crate::detect::{GitCall, ProcEntry, ProcLister, git_call};
use crate::profile::ProfileDirs;
use crate::timemachine::continuous::CaptureDeps;
use crate::timemachine::engine::RawGitEvent;
use crate::timemachine::manual::{self, ManualAsk, ManualError};
use crate::timemachine::oplog::{
    Channel, Oplog, Requester, RequesterOrigin, SnapshotFilter, SnapshotLevel, SnapshotState,
};
use crate::timemachine::protected::TmRepos;

/// The engine as the daemon links it: [`Marks`], with the daemon's confirmation.
struct Daemon(Marks);

impl EngineLink for Daemon {
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
    fn confirm(
        &self,
        repo_id: &str,
        worktrees: &[PathBuf],
        mark: i64,
        ex: &CalmExclusions<'_>,
    ) -> bool {
        self.0.0.confirm(repo_id, worktrees, mark, ex)
    }
}

/// How the `git` of the table is placed.
#[derive(Clone, Copy)]
enum Scope {
    /// Its folder is the worktree.
    InWorktree,
    /// Its folder is elsewhere and it redirects into this repo (`-C <worktree>`).
    RedirectedIn,
    /// Its folder cannot be read.
    Unreadable,
}

/// One `git apply` present at the reads in `busy`, placed as `scope`.
struct Present {
    reads: AtomicUsize,
    busy: Range<usize>,
    scope: Scope,
    wt: PathBuf,
    elsewhere: PathBuf,
}

impl ProcLister for Present {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        Some(if self.busy.contains(&read) {
            vec![ProcEntry {
                pid: 140,
                ppid: 1,
                start_us: 140,
                exe: Some(PathBuf::from(GIT)),
            }]
        } else {
            Vec::new()
        })
    }
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        entry.exe.clone()
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        match self.scope {
            Scope::InWorktree => Some(self.wt.clone()),
            Scope::RedirectedIn => Some(self.elsewhere.clone()),
            Scope::Unreadable => None,
        }
    }
    fn git_redirect(&self, _pid: u32) -> Option<bool> {
        Some(matches!(self.scope, Scope::RedirectedIn))
    }
    fn git_redirect_into(&self, _pid: u32, in_repo: &dyn Fn(&Path) -> bool) -> Option<bool> {
        Some(matches!(self.scope, Scope::RedirectedIn) && in_repo(&self.wt))
    }
    fn git_call(&self, _pid: u32) -> Option<GitCall> {
        Some(git_call(&[
            OsString::from("apply"),
            OsString::from("p.diff"),
        ]))
    }
}

/// A manual capture of a repo whose engine persisted everything at mark 1; the mark is taken at
/// read 1 and the `git` placed as `scope` is alive at reads 2 to 4 only, inside the confirmation
/// window. Its result and the manual points kept at mark 1.
fn manual_capture(scope: Scope) -> (Result<manual::ManualCaptured, ManualError>, usize) {
    let f = gitraptor_testkit::Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let r = Rig {
        common: gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap(),
        wt: f.repo.clone(),
        f,
        marks: Arc::new(RepoMarks::default()),
        procs: Arc::new(Procs::default()),
    };
    r.publish(1);
    r.marks.set_procs(Arc::new(Present {
        reads: AtomicUsize::new(0),
        busy: 2..5,
        scope,
        wt: r.wt.clone(),
        elsewhere: r.f.home.clone(),
    }));
    let dirs = ProfileDirs::under_root(&r.f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    repos.insert(REPO, &r.common, Oplog::open(&dirs, REPO, 1).unwrap().0);
    let deps = CaptureDeps {
        repos: Arc::clone(&repos),
        engine: Arc::new(Daemon(Marks(Arc::clone(&r.marks)))),
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
        after_mark: None,
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

/// A `git apply` working in the worktree during the window, gone before it ends: discarded.
#[test]
fn a_known_git_in_the_worktree_during_the_window_discards_the_capture() {
    let (out, kept) = manual_capture(Scope::InWorktree);
    assert!(
        matches!(out, Err(ManualError::Discarded)),
        "a git known in the worktree is never absorbed: {out:?}"
    );
    assert_eq!(kept, 0);
}

/// The same from another folder, redirected into this repo: discarded.
#[test]
fn a_git_redirected_into_the_repo_during_the_window_discards_the_capture() {
    let (out, kept) = manual_capture(Scope::RedirectedIn);
    assert!(
        matches!(out, Err(ManualError::Discarded)),
        "a git redirected into the repo is never absorbed: {out:?}"
    );
    assert_eq!(kept, 0);
}

/// A `git` whose scope cannot be read, gone before the window ends: absorbed, the point kept at
/// its mark.
#[test]
fn an_unreadable_git_during_the_window_is_absorbed() {
    let (out, kept) = manual_capture(Scope::Unreadable);
    assert!(out.is_ok(), "an unknown blip is absorbed: {out:?}");
    assert_eq!(kept, 1);
}

/// A read-only subcommand that writes a file is not read-only.
#[test]
fn a_read_only_subcommand_with_output_is_not_read_only() {
    let call = |argv: &[&str]| git_call(&argv.iter().map(OsString::from).collect::<Vec<_>>());
    assert!(call(&["diff"]).read_only);
    assert!(!call(&["diff", "--output=patch.diff"]).read_only);
    assert!(!call(&["log", "--output", "log.txt"]).read_only);
    assert!(!call(&["show", "--output=x", "HEAD"]).read_only);
}

/// `--attr-source <tree-ish>` takes the next argument: the subcommand is after it.
#[test]
fn attr_source_takes_its_value() {
    let call = git_call(&[
        OsString::from("--attr-source"),
        OsString::from("HEAD"),
        OsString::from("log"),
    ]);
    assert_eq!(call.subcommand, "log");
    assert!(call.read_only);
}
