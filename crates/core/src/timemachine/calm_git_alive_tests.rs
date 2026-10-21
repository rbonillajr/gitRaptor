//! No calm while a `git` runs in the worktree (fix/tm-restore-calm-mark, case 3): a running `git`
//! may have changed the content without having written its reflog yet (`reset --hard`: index,
//! then `ORIG_HEAD`, then `HEAD`). And only complete reflog lines are published (D16).
//!
//! Every test runs on a testkit fixture (temporary repo, home and profile), never this repo or
//! the real profile (NFR-01). The processes come from a synthetic table, changed at fixed points,
//! so nothing waits on time. The table is shared with the other tests of the case.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

use gitraptor_api::messages::GitEventKind;
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;

use super::*;
use crate::daemon::{LogLimits, Logger};
use crate::detect::{GitCall, ProcEntry, ProcLister, git_call};
use crate::profile::ProfileDirs;
use crate::timemachine::continuous::{CaptureDeps, Failure, observe};
use crate::timemachine::protected::TmRepos;
use crate::timemachine::store::CaptureError;
use crate::watch::{RefsView, classify};

pub(crate) const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
pub(crate) const GIT: &str = "/usr/bin/git";

/// A synthetic process table: what each process is, where it runs, and its command line, as the
/// system lister would report them.
#[derive(Default)]
pub(crate) struct Procs {
    procs: Mutex<Vec<ProcEntry>>,
    cwds: Mutex<HashMap<u32, PathBuf>>,
    redirects: Mutex<HashMap<u32, Option<bool>>>,
    argv: Mutex<HashMap<u32, Vec<OsString>>>,
    /// Calls to `list_bare`: one per pass that looked at processes.
    pub(crate) bare_calls: AtomicUsize,
}

impl Procs {
    /// A process `exe` with parent `ppid`, working in `cwd` (`None`: unreadable) with `argv`
    /// after the program name.
    pub(crate) fn add(&self, pid: u32, ppid: u32, exe: &str, cwd: Option<&Path>, argv: &[&str]) {
        self.procs.lock().unwrap().push(ProcEntry {
            pid,
            ppid,
            start_us: u64::from(pid),
            exe: Some(PathBuf::from(exe)),
        });
        if let Some(cwd) = cwd {
            self.cwds.lock().unwrap().insert(pid, cwd.to_path_buf());
        }
        self.argv
            .lock()
            .unwrap()
            .insert(pid, argv.iter().map(OsString::from).collect());
    }

    pub(crate) fn kill(&self, pid: u32) {
        self.procs.lock().unwrap().retain(|p| p.pid != pid);
        self.cwds.lock().unwrap().remove(&pid);
        self.argv.lock().unwrap().remove(&pid);
        self.redirects.lock().unwrap().remove(&pid);
    }

    /// What `git_redirect` answers for `pid` (`Some(false)` unless set).
    pub(crate) fn redirect(&self, pid: u32, answer: Option<bool>) {
        self.redirects.lock().unwrap().insert(pid, answer);
    }
}

impl ProcLister for Procs {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        Some(self.procs.lock().unwrap().clone())
    }
    fn list_bare(&self) -> Option<Vec<ProcEntry>> {
        self.bare_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut table = self.list()?;
        for e in &mut table {
            e.exe = None;
        }
        Some(table)
    }
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        let procs = self.procs.lock().unwrap();
        let now = procs.iter().find(|p| p.pid == entry.pid)?;
        (now.start_us == entry.start_us)
            .then(|| now.exe.clone())
            .flatten()
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.cwds.lock().unwrap().get(&pid).cloned()
    }
    fn git_redirect(&self, pid: u32) -> Option<bool> {
        self.redirects
            .lock()
            .unwrap()
            .get(&pid)
            .copied()
            .unwrap_or(Some(false))
    }
    fn git_call(&self, pid: u32) -> Option<GitCall> {
        self.argv.lock().unwrap().get(&pid).map(|a| git_call(a))
    }
}

/// The engine as the daemon links it, over a real `RepoMarks`.
pub(crate) struct Marks(pub(crate) Arc<RepoMarks>);

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
}

/// A repo whose engine persisted everything on disk at mark 1, with a process table.
pub(crate) struct Rig {
    pub(crate) f: Fixture,
    pub(crate) common: PathBuf,
    pub(crate) wt: PathBuf,
    pub(crate) marks: Arc<RepoMarks>,
    pub(crate) procs: Arc<Procs>,
}

pub(crate) fn rig() -> Rig {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    let r = Rig {
        wt: f.repo.clone(),
        f,
        common,
        marks: Arc::new(RepoMarks::default()),
        procs: Arc::new(Procs::default()),
    };
    r.publish(1);
    r.marks
        .set_procs(Arc::clone(&r.procs) as Arc<dyn ProcLister>);
    r
}

impl Rig {
    /// The engine persisted a batch at `seq` that read the disk as it is now.
    pub(crate) fn publish(&self, seq: i64) {
        let reader = RepoReader::open(&self.common, &ReaderOptions::default()).unwrap();
        let branches = crate::watch::read_local_branches(&reader).unwrap();
        self.marks.set_persisted(REPO, seq, &self.common, branches);
        let view = RefsView::read(&self.common).unwrap();
        self.marks.set_head_logs(REPO, &view.head_logs());
        let git_dir = git_dir_of(&self.wt).unwrap();
        self.marks.set_heads(
            REPO,
            &[(
                self.wt.clone(),
                std::fs::read(git_dir.join("HEAD")).unwrap(),
            )],
        );
    }

    pub(crate) fn settle_now(&self) -> Option<i64> {
        self.marks
            .settle(REPO, std::slice::from_ref(&self.wt), Duration::ZERO)
    }

    pub(crate) fn head(&self) -> String {
        self.f.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }
}

/// C19: a `git` working in the worktree holds its calm; its end gives it back. A `git` whose
/// folder cannot be read counts (fail closed); the daemon's own children and a `git` of another
/// repo do not.
#[test]
fn a_worktree_with_a_git_still_running_is_not_calm() {
    let r = rig();
    assert_eq!(r.settle_now(), Some(1), "calm before any git");
    r.procs.add(4242, 1, GIT, Some(&r.wt), &["status"]);
    assert_eq!(r.settle_now(), None, "a git runs in the worktree");
    r.procs.kill(4242);
    assert_eq!(r.settle_now(), Some(1), "it ended");

    r.procs
        .add(4243, 1, GIT, Some(&r.common.join("refs")), &["status"]);
    assert_eq!(r.settle_now(), None, "a git runs in the repo's Git folder");
    r.procs.kill(4243);

    r.procs.add(4244, 1, GIT, None, &["status"]);
    assert_eq!(r.settle_now(), None, "a git whose folder cannot be read");
    r.procs.kill(4244);

    r.procs
        .add(4245, std::process::id(), GIT, Some(&r.wt), &["status"]);
    assert_eq!(r.settle_now(), Some(1), "the daemon's own child");
    r.procs.kill(4245);

    r.procs
        .add(4246, 1, GIT, Some(&r.f.other_repo), &["status"]);
    assert_eq!(r.settle_now(), Some(1), "a git of another repo");
}

/// C20: a `git` that starts between the capture's mark and its read discards the capture.
#[test]
fn confirm_mark_rejects_a_capture_while_a_git_is_alive() {
    let r = rig();
    let dirs = ProfileDirs::under_root(&r.f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    let (oplog, _) = crate::timemachine::oplog::Oplog::open(&dirs, REPO, 1).unwrap();
    repos.insert(REPO, &r.common, oplog);
    let (procs, wt) = (Arc::clone(&r.procs), r.wt.clone());
    let deps = CaptureDeps {
        repos,
        engine: Arc::new(Marks(Arc::clone(&r.marks))),
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
        after_mark: Some(Arc::new(move || {
            procs.add(4300, 1, GIT, Some(&wt), &["reset"]);
        })),
    };
    let out = observe(
        &deps,
        REPO,
        std::slice::from_ref(&r.wt),
        None,
        None,
        Duration::ZERO,
        false,
    );
    assert!(
        matches!(out, Err(Failure::Capture(CaptureError::Discarded))),
        "a capture read while a git runs is discarded: {out:?}"
    );
}

/// C21: a reflog line without its `\n` is not published and names no event; once complete it is
/// the `Reset` and the whole length. A complete line without an event (a checkout of the same
/// branch) still reaches the calm.
#[test]
fn a_half_written_reflog_line_is_not_published() {
    use std::io::Write as _;
    let r = rig();
    let log = r.common.join("logs/HEAD");
    let append = |text: &str| {
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    };
    let len = |view: &RefsView| view.worktrees[&r.wt].head_log_len;
    let kinds = |old: &RefsView, new: &RefsView| -> Vec<GitEventKind> {
        classify(&r.common, old, new, (0, 0))
            .into_iter()
            .map(|e| e.kind)
            .collect()
    };
    let head = r.head();
    let v0 = RefsView::read(&r.common).unwrap();

    append(&format!(
        "{head} {head} A U Thor <a@example.com> 1728000000 +0000\treset: moving to HEAD"
    ));
    let v1 = RefsView::read(&r.common).unwrap();
    assert_eq!(len(&v1), len(&v0), "the half line is not published");
    assert!(
        !kinds(&v0, &v1).contains(&GitEventKind::Reset),
        "the half line names no event"
    );
    assert_eq!(
        r.settle_now(),
        Some(1),
        "the engine compares complete lines too"
    );

    append("\n");
    let v2 = RefsView::read(&r.common).unwrap();
    assert_eq!(len(&v2), std::fs::metadata(&log).unwrap().len());
    assert!(kinds(&v1, &v2).contains(&GitEventKind::Reset));
    r.publish(2);

    append(&format!(
        "{head} {head} A U Thor <a@example.com> 1728000001 +0000\tcheckout: moving from main to main\n"
    ));
    let v3 = RefsView::read(&r.common).unwrap();
    assert!(!kinds(&v2, &v3).contains(&GitEventKind::Reset));
    r.publish(2);
    assert_eq!(
        r.settle_now(),
        Some(2),
        "a line without an event reaches the calm"
    );
}
