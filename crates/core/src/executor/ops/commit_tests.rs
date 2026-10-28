//! The commit plan reads the worktree through the gate's Git directory.

use std::path::Path;
use std::process::Command;

use super::*;

/// `git` in `cwd` with no user, global or system configuration.
fn git(home: &Path, cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A repository at `dir` with `file` staged.
fn staged_repo(home: &Path, dir: &Path, file: &str) {
    std::fs::create_dir_all(dir).expect("dir");
    git(home, dir, &["init", "-q"]);
    std::fs::write(dir.join(file), "x\n").expect("file");
    git(home, dir, &["add", file]);
}

fn facts(root: &Path, git_dir: &Path) -> RepoFacts {
    RepoFacts {
        root: root.to_path_buf(),
        git_dir: git_dir.to_path_buf(),
        common_dir: git_dir.to_path_buf(),
        linked: false,
        root_id: None,
        dot_git_id: None,
        gitdir_linked_back: false,
        head_branch: Some("main".into()),
        head_commit: None,
        detached: false,
        in_progress: None,
        git_locks: Vec::new(),
        locked: false,
        grafts: false,
        branches_elsewhere: Vec::new(),
    }
}

#[test]
fn the_view_comes_from_the_gates_git_dir_when_dot_git_points_elsewhere() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let root = tmp.path().join("wt");
    let other = tmp.path().join("other");
    staged_repo(&home, &root, "mine.txt");
    staged_repo(&home, &other, "theirs.txt");
    // The gate opened this Git directory; then the worktree's `.git` is swapped for a file
    // that points to another repository (A11).
    let gate = tmp.path().join("gate");
    std::fs::create_dir_all(&gate).expect("gate");
    let git_dir = gate.join(".git");
    std::fs::rename(root.join(".git"), &git_dir).expect("move .git");
    std::fs::write(
        root.join(".git"),
        format!("gitdir: {}\n", other.join(".git").display()),
    )
    .expect("swap .git");

    let view = read_view(&facts(&root, &git_dir)).expect("view");
    assert_eq!(view.staged, vec!["mine.txt".to_owned()]);
    // Re-resolving the worktree's `.git` would have read the other repository.
    let swapped = RepoReader::open(&root, &ReaderOptions::default()).expect("open");
    let swapped = WorktreeView::read(&root, swapped).expect("swapped view");
    assert_eq!(swapped.staged, vec!["theirs.txt".to_owned()]);
}
