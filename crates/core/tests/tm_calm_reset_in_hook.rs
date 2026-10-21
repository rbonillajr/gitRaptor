//! A capture while a `git reset --hard` is paused in its hook is not taken
//! (fix/tm-restore-calm-mark, case 3, D15). `reset --hard` reverts the files, then commits
//! `ORIG_HEAD` and only then `HEAD`, whose reflog line is what the engine's calm compares. Its
//! `reference-transaction` hook stops the `git` right after the first commit: the content is
//! already reverted and the `HEAD` reflog is still the published one.
//!
//! A testkit fixture (temporary repo and home, Git without system config) and the real process
//! table; never this repo or the real profile (NFR-01). The hook writes a marker and blocks reading
//! a FIFO: the test waits for the marker and releases the FIFO, nothing sleeps. The `git` runs as
//! a grandchild of the test, because the engine leaves out its process's own children.
#![cfg(unix)]

use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitraptor_core::daemon::{LogLimits, Logger};
use gitraptor_core::detect::{ProcLister, SystemProcLister};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::continuous::{CaptureDeps, Failure, observe};
use gitraptor_core::timemachine::engine::{EngineLink, RawGitEvent, RepoMarks, git_dir_of};
use gitraptor_core::timemachine::oplog::Oplog;
use gitraptor_core::timemachine::protected::TmRepos;
use gitraptor_core::watch::{RefsView, read_local_branches};
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;

const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const DEADLINE: Duration = Duration::from_secs(20);

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

/// The engine persisted a batch at `seq` that read the disk as it is now.
fn publish(marks: &RepoMarks, common: &Path, wt: &Path, seq: i64) {
    let reader = RepoReader::open(common, &ReaderOptions::default()).unwrap();
    marks.set_persisted(REPO, seq, common, read_local_branches(&reader).unwrap());
    marks.set_head_logs(REPO, &RefsView::read(common).unwrap().head_logs());
    let git_dir = git_dir_of(wt).unwrap();
    marks.set_heads(
        REPO,
        &[(
            wt.to_path_buf(),
            std::fs::read(git_dir.join("HEAD")).unwrap(),
        )],
    );
}

/// Lets the paused hook go once, also when the test fails before it does: a writer that opens
/// without waiting, so a hook that never started does not hang the test either.
struct Release(PathBuf);

impl Release {
    fn now(&self) {
        if let Ok(mut fifo) = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(&self.0)
        {
            let _ = fifo.write_all(b"\n");
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}

#[test]
fn a_capture_during_a_reset_paused_in_its_hook_is_not_taken() {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    f.write("a.txt", "agent work\n");
    let wt = f.repo.clone();
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    let marks = Arc::new(RepoMarks::default());
    publish(&marks, &common, &wt, 1);
    marks.set_procs(Arc::new(SystemProcLister) as Arc<dyn ProcLister>);

    // The hook: on the first `committed`, a marker, then a blocking read of the FIFO.
    let hooks = f.root.join("hooks");
    std::fs::create_dir(&hooks).unwrap();
    let fifo = f.root.join("release.fifo");
    let marker = f.root.join("paused");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let hook = hooks.join("reference-transaction");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\n[ \"$1\" = committed ] || exit 0\n[ -e '{m}' ] && exit 0\n: > '{m}'\nread _ < '{fifo}'\nexit 0\n",
            m = marker.display(),
            fifo = fifo.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let release = Release(fifo.clone());

    // `sh` forks the `git` and waits for it: the `git` is not a child of this process.
    let mut reset = Command::new("/bin/sh")
        .args([
            "-c",
            "\"$0\" -c core.hooksPath=\"$1\" reset -q --hard; exit $?",
        ])
        .arg(&f.git)
        .arg(&hooks)
        .current_dir(&wt)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &f.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !marker.exists() {
        assert!(start.elapsed() < DEADLINE, "the hook never paused");
        std::thread::sleep(Duration::from_millis(10));
    }

    // Paused: the files are reverted and the `HEAD` reflog is the one the engine published.
    assert_eq!(std::fs::read(wt.join("a.txt")).unwrap(), b"alpha\n");
    assert_eq!(
        marks.settle(REPO, std::slice::from_ref(&wt), Duration::ZERO),
        None,
        "a git still runs in the worktree"
    );
    let dirs = ProfileDirs::under_root(&f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    repos.insert(REPO, &common, Oplog::open(&dirs, REPO, 1).unwrap().0);
    let deps = CaptureDeps {
        repos,
        engine: Arc::new(Marks(Arc::clone(&marks))),
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
        after_mark: None,
    };
    let out = observe(
        &deps,
        REPO,
        std::slice::from_ref(&wt),
        None,
        None,
        Duration::ZERO,
        false,
    );
    assert!(
        matches!(out, Err(Failure::NotCalm)),
        "no capture while the reset runs: {out:?}"
    );

    // The reset ends and the engine persists it (the Reset, event 2).
    release.now();
    assert!(reset.wait().unwrap().success());
    publish(&marks, &common, &wt, 2);
    let mark = marks.settle(REPO, std::slice::from_ref(&wt), Duration::ZERO);
    assert!(
        mark.is_some_and(|m| m >= 2),
        "calm again, past the Reset: {mark:?}"
    );
}
