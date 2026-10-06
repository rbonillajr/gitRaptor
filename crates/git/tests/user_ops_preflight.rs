//! TS-CKP-002: the preflight read of the executor (ADR-CKP-002 § 1 and § 5, M-05) and the
//! invocation of user operations against real temporary repos (never this repo, NFR-01).

mod common;

use std::path::{Path, PathBuf};

use common::Fixture;
use gitraptor_git::preflight::preflight;
use gitraptor_git::user_ops::{FALLBACK_NO_EDITOR, RepoTarget, SessionEnv, UserGitCommand, UserOp};
use gitraptor_git::{InProgress, Oid, RefName};

/// The rejecting editor. On Windows [`FALLBACK_NO_EDITOR`] is not absolute and the launch is
/// refused (XP-19); a ref update never opens an editor, so any absolute path serves there.
fn no_editor(f: &Fixture) -> PathBuf {
    if cfg!(windows) {
        f.root().join("raptor-no-editor.exe")
    } else {
        PathBuf::from(FALLBACK_NO_EDITOR)
    }
}

fn head(f: &Fixture, dir: &Path) -> String {
    f.git_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// The fixture repo plus a linked worktree on branch `feat`.
fn with_linked() -> (Fixture, PathBuf) {
    let f = Fixture::with_commit();
    let wt = f.root().join("repo-feat");
    f.git(&["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()]);
    (f, wt)
}

#[test]
fn a_clean_worktree_passes_and_names_its_repo() {
    let (f, wt) = with_linked();
    let main = preflight(&f.repo).unwrap();
    assert!(!main.linked && main.gitdir_linked_back);
    assert_eq!(main.head.branch.as_deref(), Some("main"));
    assert!(main.git_locks.is_empty() && !main.grafts && main.in_progress.is_none());
    assert_eq!(main.branches_elsewhere, ["feat"]);

    let linked = preflight(&wt).unwrap();
    assert!(linked.linked && linked.gitdir_linked_back && !linked.locked);
    assert_eq!(linked.common_dir, main.common_dir);
    assert_eq!(linked.branches_elsewhere, ["main"]);
    assert!(linked.root_id.is_some() && linked.dot_git_id.is_some());
}

/// The Git preconditions the TUI does not see (ADR-CKP-002 § 1): locks are reported and never
/// removed, a locked worktree, grafts, an operation in progress and a detached HEAD.
#[test]
fn git_preconditions_are_read_without_touching_anything() {
    let (f, wt) = with_linked();
    let lock = f.repo.join(".git").join("index.lock");
    std::fs::write(&lock, "").unwrap();
    assert_eq!(preflight(&f.repo).unwrap().git_locks, ["index.lock"]);
    assert!(lock.exists(), "a Git lock is never removed");
    std::fs::remove_file(&lock).unwrap();

    f.git(&["worktree", "lock", wt.to_str().unwrap()]);
    assert!(preflight(&wt).unwrap().locked);
    f.git(&["worktree", "unlock", wt.to_str().unwrap()]);

    let info = f.repo.join(".git").join("info");
    std::fs::create_dir_all(&info).unwrap();
    std::fs::write(info.join("grafts"), "").unwrap();
    assert!(preflight(&wt).unwrap().grafts);
    std::fs::remove_file(info.join("grafts")).unwrap();

    let oid = head(&f, &f.repo);
    std::fs::write(f.repo.join(".git").join("MERGE_HEAD"), format!("{oid}\n")).unwrap();
    assert_eq!(
        preflight(&f.repo).unwrap().in_progress,
        Some(InProgress::Merge)
    );
    std::fs::remove_file(f.repo.join(".git").join("MERGE_HEAD")).unwrap();

    f.git_in(&wt, &["checkout", "-q", "--detach"]);
    assert!(preflight(&wt).unwrap().head.detached);
}

/// M-05 (Validación 23): a `.git` of a linked worktree replaced, or a broken `gitdir` back link,
/// shows in the facts the executor compares under the lock.
#[test]
fn a_replaced_dot_git_changes_the_identity() {
    let (_f, wt) = with_linked();
    let before = preflight(&wt).unwrap();
    let dot_git = wt.join(".git");
    let text = std::fs::read_to_string(&dot_git).unwrap();
    // Substituted the way an attacker would: a new file renamed over the old one. Both exist at
    // once, so the file system cannot hand the new one the old inode (ext4 reuses a freed one).
    let replacement = wt.join(".git.new");
    std::fs::write(&replacement, &text).unwrap();
    std::fs::rename(&replacement, &dot_git).unwrap();
    let after = preflight(&wt).unwrap();
    assert_ne!(before.dot_git_id, after.dot_git_id);
    assert!(after.gitdir_linked_back);

    std::fs::write(after.git_dir.join("gitdir"), "/elsewhere/.git\n").unwrap();
    assert!(!preflight(&wt).unwrap().gitdir_linked_back);
}

/// ADR-CKP-002 § 5 (Validación 4): the expected value goes to Git; a ref moved by an external
/// process makes the whole update fail and leaves the ref where the other process put it.
#[test]
fn a_ref_update_with_a_stale_old_value_fails_whole() {
    let f = Fixture::with_commit();
    let first = head(&f, &f.repo);
    f.write("c.txt", "gamma\n");
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "second"]);
    let second = head(&f, &f.repo);
    f.git(&["branch", "base", &first]);
    // An external process moves `base` after the plan read it at `first`.
    f.git(&["branch", "-f", "base", &second]);

    let target = RepoTarget {
        git_dir: f.repo.join(".git"),
        work_tree: None,
    };
    let op = UserOp::UpdateRef {
        name: RefName::new("refs/heads/base").unwrap(),
        new: Oid::from_hex(&first).unwrap(),
        old: Oid::from_hex(&first).unwrap(),
    };
    let cmd = UserGitCommand::new(
        &f.git,
        &target,
        &op,
        None,
        &SessionEnv::default(),
        &no_editor(&f),
        Some(&f.home),
    )
    .unwrap();
    let (mut child, out) = cmd.spawn().unwrap();
    let status = child.wait().unwrap();
    let (_, stderr) = out.collect();
    assert!(!status.success(), "{}", String::from_utf8_lossy(&stderr));
    let now = f.git(&["rev-parse", "refs/heads/base"]);
    assert_eq!(now.trim(), second);
}

/// ADR-CKP-002 § 6 (Validación 8 and 25): a hook runs without a terminal, with stdin at EOF and
/// only the standard descriptors; an editor request ends in the rejecting editor, never hangs.
#[cfg(target_os = "macos")]
#[test]
fn hooks_run_without_a_terminal_and_with_only_fds_0_to_2() {
    let f = Fixture::with_commit();
    let report = f.root().join("hook-report.txt");
    let hook = f.repo.join(".git").join("hooks").join("pre-commit");
    common::script(
        &hook,
        &format!(
            "{{ for n in 3 4 5 6 7 8 9; do if {{ true >&$n; }} 2>/dev/null; then printf 'fd%s ' $n; fi; done; echo; \
             if : </dev/tty 2>/dev/null; then echo tty; else echo no-tty; fi; \
             cat; echo eof; }} > '{}' 2>&1\nexit 1\n",
            report.display()
        ),
    );
    f.write("a.txt", "changed\n");
    f.git(&["add", "a.txt"]);
    let target = RepoTarget {
        git_dir: f.repo.join(".git"),
        work_tree: Some(f.repo.clone()),
    };
    let cmd = UserGitCommand::new(
        &f.git,
        &target,
        &UserOp::CommitStaged,
        Some("message that hooks must not read"),
        &SessionEnv::default(),
        Path::new(FALLBACK_NO_EDITOR),
        Some(&f.home),
    )
    .unwrap();
    let (mut child, out) = cmd.spawn().unwrap();
    let status = child.wait().unwrap();
    let _ = out.collect();
    assert!(!status.success(), "the hook rejects the commit");
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.contains("no-tty"), "{text}");
    assert!(!text.contains("message that hooks"), "{text}");
    assert!(text.contains("eof"), "{text}");
    let fds = text.lines().next().unwrap().trim();
    assert!(fds.is_empty(), "inherited descriptors: {fds}");
}
