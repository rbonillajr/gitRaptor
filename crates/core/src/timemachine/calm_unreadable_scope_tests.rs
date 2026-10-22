//! An unreadable `git` holds the calm only if it is placed in this repo (fix/tm-restore-calm-mark,
//! D20 as revised): its arguments cannot be read, but its folder can, and that folder is a
//! worktree or the common directory of this repo. One whose folder cannot be read, or whose
//! folder is in another repo, never blocks; one that exited (a zombie included) does not count.
//!
//! Testkit fixtures and a synthetic process table; never this repo or the real profile (NFR-01).
//! The only wait is a `settle` past the grace window, which polls.

use std::sync::atomic::{AtomicBool, Ordering};

use super::calm_git_alive_tests::{GIT, Procs, REPO, Rig};
use super::*;
use crate::daemon::{LOG_FILE, LogLimits, Logger};
use crate::detect::{GitCall, ProcEntry, ProcLister};

/// One live `git`, pid 150, whose arguments cannot be read, working in `cwd` (`None`: its folder
/// cannot be read either). With `exited`, its identity no longer answers: it ended (a zombie).
struct Unreadable {
    cwd: Option<PathBuf>,
    exited: AtomicBool,
}

impl ProcLister for Unreadable {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        Some(vec![ProcEntry {
            pid: 150,
            ppid: 1,
            start_us: 150,
            exe: None,
        }])
    }
    fn exe(&self, _entry: &ProcEntry) -> Option<PathBuf> {
        (!self.exited.load(Ordering::SeqCst)).then(|| PathBuf::from(GIT))
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        self.cwd.clone()
    }
    fn git_redirect(&self, _pid: u32) -> Option<bool> {
        None
    }
    fn git_redirect_into(&self, _pid: u32, _in_repo: &dyn Fn(&Path) -> bool) -> Option<bool> {
        None
    }
    fn git_call(&self, _pid: u32) -> Option<GitCall> {
        None
    }
}

/// A repo with a linked worktree `wt-side`, persisted at mark 1, its processes being one
/// unreadable `git` placed by `cwd(rig)`; the calm is asked for the main worktree.
fn rig_with(cwd: impl FnOnce(&Rig) -> Option<PathBuf>, exited: bool) -> Rig {
    let f = gitraptor_testkit::Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    f.git(&["branch", "side"]);
    f.add_worktree("side", "side");
    let r = Rig {
        common: gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap(),
        wt: f.repo.clone(),
        f,
        marks: Arc::new(RepoMarks::default()),
        procs: Arc::new(Procs::default()),
    };
    r.publish(1);
    let procs = Unreadable {
        cwd: cwd(&r),
        exited: AtomicBool::new(exited),
    };
    r.marks.set_procs(Arc::new(procs));
    r
}

/// A `git` that ended (a zombie whose identity no longer answers) does not count.
#[test]
fn an_exited_git_does_not_count() {
    let r = rig_with(|r| Some(r.wt.clone()), true);
    assert_eq!(r.settle_now(), Some(1));
}

/// A `git` whose folder and arguments cannot be read never blocks.
#[test]
fn a_git_with_no_readable_folder_never_blocks() {
    let r = rig_with(|_| None, false);
    assert_eq!(r.settle_now(), Some(1));
}

/// A `git` working in another repo, its arguments unreadable, never blocks.
#[test]
fn an_unreadable_git_in_another_repo_never_blocks() {
    let r = rig_with(|r| Some(r.f.other_repo.clone()), false);
    assert_eq!(r.settle_now(), Some(1));
}

/// A `git` working in another worktree of this repo, its arguments unreadable (it may redirect
/// into this one): it holds the calm for the grace window, then is logged once.
#[test]
fn an_unreadable_git_in_this_repo_blocks_for_the_grace_window_then_is_logged() {
    let r = rig_with(
        |r| {
            Some(gitraptor_testkit::fixture::canonical_dir(
                &r.f.root.join("wt-side"),
            ))
        },
        false,
    );
    let dirs = crate::profile::ProfileDirs::under_root(&r.f.profile);
    r.marks
        .set_logger(Logger::open(&dirs.state, LogLimits::default()).unwrap());
    assert_eq!(r.settle_now(), None, "within the window it counts");
    let wts = std::slice::from_ref(&r.wt);
    assert_eq!(
        r.marks.settle(REPO, wts, Duration::from_secs(4)),
        Some(1),
        "past the window it no longer counts"
    );
    let log = std::fs::read_to_string(dirs.state.join(LOG_FILE)).unwrap_or_default();
    let logged = log
        .lines()
        .filter(|l| l.contains("tm_calm_probe_unreadable"))
        .count();
    assert_eq!(logged, 1, "once:\n{log}");
}
