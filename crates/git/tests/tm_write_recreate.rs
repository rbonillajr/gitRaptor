//! Recreating a deleted linked worktree never follows a link (SEC-TMC-04, H-01 of the security
//! review of US-TMC-009): neither `.git/worktrees` nor the target folder, not even when someone
//! swaps it for a link between the check and the write. A failure halfway leaves no
//! administrative entry behind. An integration test because it plants junctions with `mklink` on
//! Windows, and the crate's sources may launch no process outside `invoke.rs` (`static_check.rs`).

mod common;

use std::path::{Path, PathBuf};

use common::Fixture;
use gitraptor_git::tm_write::WriteError;
use gitraptor_git::tm_write::recreate::{Stage, recreate, recreate_with};
use gitraptor_git::tm_write::worktree::{HeadValue, WriteWorktree};

/// A repository with a linked worktree `w` on branch `feature`, then removed: what `undo` and
/// `restore` find when they recreate it.
struct Removed {
    f: Fixture,
    wt: PathBuf,
    head: HeadValue,
    profile: tempfile::TempDir,
}

impl Removed {
    fn new() -> Self {
        let f = Fixture::with_commit();
        let wt = f.repo.parent().unwrap().join("w");
        f.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ]);
        let head = HeadValue::parse(&std::fs::read(f.repo.join(".git/worktrees/w/HEAD")).unwrap())
            .unwrap();
        f.git(&["worktree", "remove", "--force", wt.to_str().unwrap()]);
        assert!(!wt.exists());
        let _ = std::fs::remove_dir(f.repo.join(".git/worktrees"));
        Self {
            f,
            wt,
            head,
            profile: tempfile::tempdir().unwrap(),
        }
    }

    fn main(&self) -> WriteWorktree {
        WriteWorktree::open(&self.f.repo).unwrap()
    }

    fn admin(&self) -> PathBuf {
        self.f.repo.join(".git/worktrees/w")
    }

    fn recreate(&self, hook: &dyn Fn(Stage)) -> Result<WriteWorktree, WriteError> {
        recreate_with(
            &self.main(),
            "w",
            &self.wt,
            &self.head,
            self.profile.path(),
            hook,
        )
    }
}

/// A symbolic link on Unix; a junction on Windows, which needs no privilege.
fn link(at: &Path, to: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(to, at).unwrap();
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(at)
            .arg(to)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn refused(result: &Result<WriteWorktree, WriteError>, why: &str) {
    assert!(
        matches!(result, Err(WriteError::InvalidInput(m)) if m.contains(why)),
        "{result:?}"
    );
}

#[test]
fn a_removed_worktree_is_recreated_where_it_was() {
    let r = Removed::new();
    let wt = recreate(&r.main(), "w", &r.wt, &r.head, r.profile.path()).unwrap();
    assert_eq!(wt.root(), r.wt);
    assert!(wt.is_linked());
    assert_eq!(entries(&r.admin()), ["HEAD", "commondir", "gitdir"]);
    assert_eq!(
        std::fs::read_to_string(r.wt.join(".git")).unwrap(),
        format!("gitdir: {}\n", r.admin().display())
    );
    let list = r.f.git(&["worktree", "list", "--porcelain"]);
    assert!(list.contains("branch refs/heads/feature"), "{list}");
    // Into an empty folder that is already there, too.
    let r = Removed::new();
    std::fs::create_dir(&r.wt).unwrap();
    r.recreate(&|_| {}).unwrap();
    assert!(r.wt.join(".git").is_file());
}

#[test]
fn a_worktrees_folder_that_is_a_link_is_refused() {
    let r = Removed::new();
    let elsewhere = tempfile::tempdir().unwrap();
    link(&r.f.repo.join(".git/worktrees"), elsewhere.path());
    let result = r.recreate(&|_| {});
    refused(&result, "worktrees folder");
    assert!(
        entries(elsewhere.path()).is_empty(),
        "written through the link"
    );
    assert!(r.wt.symlink_metadata().is_err() || entries(&r.wt).is_empty());
}

#[test]
fn a_worktrees_entry_that_is_a_file_is_refused() {
    let r = Removed::new();
    std::fs::write(r.f.repo.join(".git/worktrees"), b"").unwrap();
    refused(&r.recreate(&|_| {}), "worktrees folder");
}

#[test]
fn a_target_swapped_for_a_link_after_the_check_writes_nothing_outside() {
    let r = Removed::new();
    let home = tempfile::tempdir().unwrap();
    let result = r.recreate(&|stage| {
        if stage == Stage::TargetChecked {
            link(&r.wt, home.path());
        }
    });
    refused(&result, "link");
    assert!(entries(home.path()).is_empty(), "a .git landed outside");
    assert!(r.admin().symlink_metadata().is_err(), "admin entry left");
}

#[test]
fn a_target_moved_away_before_the_commit_point_writes_nothing_outside() {
    let r = Removed::new();
    let home = tempfile::tempdir().unwrap();
    let moved = r.wt.with_file_name("w-moved");
    let swapped = std::cell::Cell::new(false);
    let result = r.recreate(&|stage| {
        // On Windows the target is held open without `FILE_SHARE_DELETE`: the move fails, and
        // the recreation goes on where it was checked.
        if stage == Stage::BeforeCommit && std::fs::rename(&r.wt, &moved).is_ok() {
            link(&r.wt, home.path());
            swapped.set(true);
        }
    });
    assert!(entries(home.path()).is_empty(), "a .git landed outside");
    assert!(
        !(cfg!(windows) && swapped.get()),
        "a pinned folder was moved"
    );
    if swapped.get() {
        refused(&result, "moved");
        assert!(r.admin().symlink_metadata().is_err(), "admin entry left");
        assert!(entries(&moved).is_empty(), "our .git left behind");
    } else {
        result.unwrap();
    }
}

#[test]
fn a_failure_halfway_removes_only_the_administrative_entry() {
    let r = Removed::new();
    let result = r.recreate(&|stage| {
        if stage == Stage::BeforeCommit {
            std::fs::write(r.wt.join(".git"), b"someone else's\n").unwrap();
        }
    });
    assert!(result.is_err(), "{result:?}");
    assert!(r.admin().symlink_metadata().is_err(), "admin entry left");
    assert!(r.f.repo.join(".git/worktrees").is_dir());
    assert_eq!(
        std::fs::read(r.wt.join(".git")).unwrap(),
        b"someone else's\n",
        "someone else's file touched"
    );
}

#[test]
fn an_existing_administrative_entry_is_left_alone() {
    let r = Removed::new();
    std::fs::create_dir_all(r.admin()).unwrap();
    std::fs::write(r.admin().join("HEAD"), b"x").unwrap();
    refused(&r.recreate(&|_| {}), "already exists");
    assert_eq!(entries(&r.admin()), ["HEAD"]);
    assert!(r.wt.symlink_metadata().is_err() || entries(&r.wt).is_empty());
}
