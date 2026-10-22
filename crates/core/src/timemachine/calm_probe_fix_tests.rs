//! Fixes of the process probe of the engine's calm after review (fix/tm-restore-calm-mark):
//! a process that becomes `git` by `exec` keeps its identity and must still be seen; read-only
//! `git` subcommands that never write a worktree or a ref do not hold the calm (D22).
//!
//! Testkit fixtures and a synthetic process table; never this repo or the real profile (NFR-01).
//! Each pass of the probe is a `settle(ZERO)`, so nothing waits on time.

use std::ffi::OsString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::calm_git_alive_tests::{GIT, Procs, Rig};
use super::*;
use crate::detect::{GitCall, ProcEntry, ProcLister, git_call};

/// One process, `pid` 120 in the worktree, whose executable is `sh` until its `exec_at`-th read
/// and `git` from then on: the same `(pid, start_us)`, as `exec` keeps them.
struct Execs {
    cwd: PathBuf,
    args: Vec<OsString>,
    exe_reads: AtomicUsize,
    exec_at: usize,
}

impl ProcLister for Execs {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        Some(vec![ProcEntry {
            pid: 120,
            ppid: 1,
            start_us: 120,
            exe: None,
        }])
    }
    fn exe(&self, _entry: &ProcEntry) -> Option<PathBuf> {
        let read = self.exe_reads.fetch_add(1, Ordering::SeqCst) + 1;
        Some(PathBuf::from(if read >= self.exec_at {
            GIT
        } else {
            "/bin/sh"
        }))
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        Some(self.cwd.clone())
    }
    fn git_redirect(&self, _pid: u32) -> Option<bool> {
        Some(false)
    }
    fn git_call(&self, _pid: u32) -> Option<GitCall> {
        Some(git_call(&self.args))
    }
}

/// A process table with one `git` in the worktree running `argv`.
struct One {
    cwd: PathBuf,
    args: Mutex<Vec<OsString>>,
}

impl ProcLister for One {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        Some(vec![ProcEntry {
            pid: 121,
            ppid: 1,
            start_us: 121,
            exe: Some(PathBuf::from(GIT)),
        }])
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        Some(self.cwd.clone())
    }
    fn git_redirect(&self, _pid: u32) -> Option<bool> {
        Some(false)
    }
    fn git_call(&self, _pid: u32) -> Option<GitCall> {
        Some(git_call(&self.args.lock().unwrap()))
    }
}

/// A repo whose engine persisted everything at mark 1, and the processes `procs(worktree)`.
fn rig_with(procs: impl FnOnce(&Path) -> Arc<dyn ProcLister>) -> Rig {
    let f = gitraptor_testkit::Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let r = Rig {
        common: gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap(),
        wt: f.repo.clone(),
        f,
        marks: Arc::new(RepoMarks::default()),
        procs: Arc::new(Procs::default()),
    };
    r.publish(1);
    r.marks.set_procs(procs(&r.wt));
    r
}

/// A `bash -c '…; git reset --hard'` is `sh` at the first pass and `git` at the second, with the
/// same identity: the second pass sees the `git`.
#[test]
fn a_process_that_execs_git_is_seen_after_the_exec() {
    let r = rig_with(|wt| {
        Arc::new(Execs {
            cwd: wt.to_path_buf(),
            args: vec![OsString::from("reset"), OsString::from("--hard")],
            exe_reads: AtomicUsize::new(0),
            exec_at: 2,
        })
    });
    assert_eq!(r.settle_now(), Some(1), "first pass: still sh");
    assert_eq!(r.settle_now(), None, "second pass: it is git now");
}

/// A pager-held read-only `git` (`log`, `show`, `diff`, `blame`…) does not hold the calm; a
/// `git commit` waiting on its editor does.
#[test]
fn a_read_only_git_does_not_block_and_a_writing_one_does() {
    let mut table = None;
    let r = rig_with(|wt| {
        let one = Arc::new(One {
            cwd: wt.to_path_buf(),
            args: Mutex::new(Vec::new()),
        });
        table = Some(Arc::clone(&one));
        one
    });
    let one = table.unwrap();
    let set = |argv: &[&str]| *one.args.lock().unwrap() = argv.iter().map(OsString::from).collect();
    for argv in [
        &["log"][..],
        &["-c", "core.pager=less", "show", "HEAD"],
        &["diff"],
        &["blame", "a.txt"],
        &["grep", "x"],
        &["ls-files"],
        &["ls-tree", "HEAD"],
        &["rev-parse", "HEAD"],
        &["rev-list", "HEAD"],
        &["cat-file", "-p", "HEAD"],
        &["config", "--get", "user.name"],
        &["config", "get", "user.name"],
        &["shortlog"],
        &["describe"],
        &["for-each-ref"],
        &["help", "log"],
        &["version"],
    ] {
        set(argv);
        assert_eq!(r.settle_now(), Some(1), "read-only: git {argv:?}");
    }
    for argv in [
        &["commit"][..],
        &["rebase", "-i", "HEAD~1"],
        &["status"],
        &["config", "user.name", "x"],
        &["reset", "--hard"],
    ] {
        set(argv);
        assert_eq!(r.settle_now(), None, "writes: git {argv:?}");
    }
}

/// `--git-dir=<path>` with a path that is not UTF-8 resolves the same as `--git-dir <path>`.
#[cfg(target_os = "linux")]
#[test]
fn a_non_utf8_git_dir_value_resolves_like_the_separated_form() {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    let f = gitraptor_testkit::Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let link = f.root.join(std::ffi::OsStr::from_bytes(b"repo-\xff"));
    std::os::unix::fs::symlink(f.repo.join(".git"), &link).unwrap();
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    let in_repo = |p: &Path| p.starts_with(&common);
    let mut joined = b"--git-dir=".to_vec();
    joined.extend_from_slice(link.as_os_str().as_bytes());
    let attached = [OsString::from_vec(joined), OsString::from("status")];
    let separated = [
        OsString::from("--git-dir"),
        link.clone().into_os_string(),
        OsString::from("status"),
    ];
    assert!(crate::detect::git_redirect_into(
        &separated,
        &[],
        &f.home,
        &in_repo
    ));
    assert!(crate::detect::git_redirect_into(
        &attached,
        &[],
        &f.home,
        &in_repo
    ));
}
