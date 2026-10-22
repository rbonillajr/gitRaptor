//! A `git` working outside the worktree holds its calm only if its redirection points into this
//! repo (fix/tm-restore-calm-mark, D20 as decided): `-C`, `--git-dir`, `--work-tree` and the
//! `GIT_*` variables resolved as `git` would, relative paths from its effective folder, a missing
//! path not counting. A `git` whose redirection cannot be read counts while it is alive, for the
//! grace window of a `settle` at most, and then says so once; one that exited does not count.
//!
//! Testkit fixtures and a synthetic process table whose redirection answer goes through the real
//! resolver; never this repo or the real profile (NFR-01). Nothing sleeps: the only wait is a
//! `settle` past the grace window, which polls.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Mutex;

use super::calm_git_alive_tests::{GIT, Procs, Rig};
use super::*;
use crate::detect::procs::git_redirects;
use crate::detect::{GitCall, ProcEntry, ProcLister, git_call, git_redirect_into};

/// How the table answers a read of a process's command line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Probe {
    Read,
    /// It exits while it is being read.
    ExitsWhileRead,
}

struct Proc {
    entry: ProcEntry,
    cwd: PathBuf,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    probe: Probe,
}

/// A process table whose `git`s carry their arguments and `GIT_*` variables.
#[derive(Default)]
struct Table(Mutex<HashMap<u32, Proc>>);

impl Table {
    fn add(&self, pid: u32, cwd: &Path, args: &[&str], env: &[(&str, &Path)], probe: Probe) {
        self.0.lock().unwrap().insert(
            pid,
            Proc {
                entry: ProcEntry {
                    pid,
                    ppid: 1,
                    start_us: u64::from(pid),
                    exe: Some(PathBuf::from(GIT)),
                },
                cwd: cwd.to_path_buf(),
                args: args.iter().map(OsString::from).collect(),
                env: env
                    .iter()
                    .map(|(n, v)| (OsString::from(n), v.as_os_str().to_owned()))
                    .collect(),
                probe,
            },
        );
    }

    fn kill(&self, pid: u32) {
        self.0.lock().unwrap().remove(&pid);
    }
}

impl ProcLister for Table {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        Some(
            self.0
                .lock()
                .unwrap()
                .values()
                .map(|p| p.entry.clone())
                .collect(),
        )
    }
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        let table = self.0.lock().unwrap();
        let p = table.get(&entry.pid)?;
        (p.entry.start_us == entry.start_us)
            .then(|| p.entry.exe.clone())
            .flatten()
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.0.lock().unwrap().get(&pid).map(|p| p.cwd.clone())
    }
    fn git_redirect(&self, pid: u32) -> Option<bool> {
        let table = self.0.lock().unwrap();
        let p = table.get(&pid)?;
        (p.probe == Probe::Read)
            .then(|| git_redirects(&p.args, p.env.iter().map(|(n, _)| n.as_os_str())))
    }
    fn git_redirect_into(&self, pid: u32, in_repo: &dyn Fn(&Path) -> bool) -> Option<bool> {
        let mut table = self.0.lock().unwrap();
        let probe = table.get(&pid)?.probe;
        match probe {
            Probe::Read => {
                let p = &table[&pid];
                Some(git_redirect_into(&p.args, &p.env, &p.cwd, in_repo))
            }
            Probe::ExitsWhileRead => {
                table.remove(&pid);
                None
            }
        }
    }
    fn git_call(&self, pid: u32) -> Option<GitCall> {
        let table = self.0.lock().unwrap();
        let p = table.get(&pid)?;
        (p.probe == Probe::Read).then(|| git_call(&p.args))
    }
}

/// A repo whose engine persisted everything at mark 1, with `table` as its processes.
fn rig_with(table: &Arc<Table>) -> Rig {
    let f = gitraptor_testkit::Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    // The other folder is a repo of its own, another workspace.
    let r = Rig {
        common: gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap(),
        wt: f.repo.clone(),
        f,
        marks: Arc::new(RepoMarks::default()),
        procs: Arc::new(Procs::default()),
    };
    r.publish(1);
    r.marks.set_procs(Arc::clone(table) as Arc<dyn ProcLister>);
    r
}

/// A `git` elsewhere that redirects into another repo leaves this one calm.
#[test]
fn a_git_redirected_into_another_workspace_keeps_the_calm() {
    let table = Arc::new(Table::default());
    let r = rig_with(&table);
    let other = r.f.other_repo.clone();
    table.add(
        90,
        &r.f.home,
        &["-C", other.to_str().unwrap(), "status"],
        &[],
        Probe::Read,
    );
    assert_eq!(r.settle_now(), Some(1), "git -C into another repo");
    table.kill(90);
    table.add(
        91,
        &r.f.home,
        &["status"],
        &[("GIT_DIR", &other.join(".git"))],
        Probe::Read,
    );
    assert_eq!(r.settle_now(), Some(1), "GIT_DIR of another repo");
    table.kill(91);
    table.add(
        92,
        &r.f.home,
        &["-C", "missing-folder", "status"],
        &[],
        Probe::Read,
    );
    assert_eq!(r.settle_now(), Some(1), "a target that does not exist");
}

/// A `git` elsewhere that redirects into this repo holds its calm: `-C` the worktree, or a
/// relative `GIT_DIR` from its own folder.
#[test]
fn a_git_redirected_into_this_repo_counts() {
    let table = Arc::new(Table::default());
    let r = rig_with(&table);
    table.add(
        93,
        &r.f.home,
        &["-C", r.wt.to_str().unwrap(), "status"],
        &[],
        Probe::Read,
    );
    assert_eq!(r.settle_now(), None, "git -C into this worktree");
    table.kill(93);
    table.add(
        94,
        &r.f.root,
        &["status"],
        &[("GIT_DIR", Path::new("repo/.git"))],
        Probe::Read,
    );
    assert_eq!(r.settle_now(), None, "a relative GIT_DIR into this repo");
    table.kill(94);
    table.add(
        95,
        &r.f.root,
        &["-C", "repo", "--git-dir=.git", "status"],
        &[],
        Probe::Read,
    );
    assert_eq!(r.settle_now(), None, "-C then a relative --git-dir");
}

/// A `git` that exits while it is read does not count.
#[test]
fn an_unreadable_git_that_exited_does_not_count() {
    let table = Arc::new(Table::default());
    let r = rig_with(&table);
    table.add(97, &r.f.home, &["status"], &[], Probe::ExitsWhileRead);
    assert_eq!(r.settle_now(), Some(1));
}
