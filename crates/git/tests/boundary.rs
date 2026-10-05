//! TS-GRP-002: every read leaves the repository byte-identical, `.git` included (BR-CONS-001,
//! ADR-GRP-009 § 2). Fingerprints include files and directories with their mtime, so a lock
//! created and deleted is detected.

mod common;

use common::{Fixture, assert_unchanged, busy_repo, read_everything};
use gitraptor_git::{InProgress, ReaderOptions, RefName, RepoReader};

#[test]
fn every_read_leaves_repo_byte_identical() {
    let f = busy_repo();
    let before = f.fingerprint();
    read_everything(&f, &f.repo);
    assert_unchanged(&before, &f.fingerprint(), "reads");
}

#[test]
fn linked_worktrees_are_read_without_writes() {
    let f = busy_repo();
    let wt = f.root().join("wt-feature");
    f.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "feature"]);
    std::fs::write(wt.join("c.txt"), "gamma dirty\n").unwrap();
    let before = f.fingerprint();

    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    let linked = r.worktrees().unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].path, wt);
    drop(r);
    read_everything(&f, &wt);
    let r = RepoReader::open(&wt, &ReaderOptions::default()).unwrap();
    assert_eq!(r.head().unwrap().branch.as_deref(), Some("feature"));
    assert_eq!(r.status().unwrap().unstaged.len(), 1);
    drop(r);

    assert_unchanged(&before, &f.fingerprint(), "worktree reads");
}

#[test]
fn fsmonitor_enabled_repo_is_untouched() {
    let f = busy_repo();
    f.git(&["config", "core.fsmonitor", "true"]);
    let before = f.fingerprint();
    read_everything(&f, &f.repo);
    assert_unchanged(&before, &f.fingerprint(), "reads with core.fsmonitor=true");
    assert!(
        !f.repo.join(".git/fsmonitor--daemon.ipc").exists()
            && !f.repo.join(".git/fsmonitor--daemon").exists(),
        "fsmonitor daemon started"
    );
}

#[test]
fn untracked_cache_split_index_dirty_stat_not_rewritten() {
    let f = busy_repo();
    f.git(&["config", "core.untrackedCache", "true"]);
    f.git(&["config", "core.splitIndex", "true"]);
    f.git(&["update-index", "--untracked-cache", "--split-index"]);
    // Leave the index with a stale stat for a tracked file and new untracked content.
    f.dirty_stat("a.txt");
    f.dirty_stat("b.txt");
    f.write("new-untracked.txt", "n\n");
    let before = f.fingerprint();
    read_everything(&f, &f.repo);
    let after = f.fingerprint();
    assert_unchanged(
        &before,
        &after,
        "reads with untracked cache and split index",
    );
    assert!(!f.repo.join(".git/index.lock").exists());
}

#[test]
fn status_reports_what_git_reports() {
    let f = busy_repo();
    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    let s = r.status().unwrap();
    let paths = |v: &[gitraptor_git::Change]| v.iter().map(|c| c.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths(&s.staged), ["staged.txt"]);
    // b.txt only has a dirty stat: same content, not reported.
    assert_eq!(paths(&s.unstaged), ["a.txt"]);
    assert_eq!(s.untracked, ["untracked.txt"]);
    assert!(r.is_ignored("target", true).unwrap());
    assert!(!r.is_ignored("a.txt", false).unwrap());
    assert_eq!(r.head().unwrap().branch.as_deref(), Some("main"));
    let (ahead, behind) = r
        .ahead_behind(
            &RefName::new("main").unwrap(),
            &RefName::new("feature").unwrap(),
            1000,
        )
        .unwrap();
    assert_eq!(
        (ahead, behind),
        (
            gitraptor_git::Count::Exact(1),
            gitraptor_git::Count::Exact(1)
        )
    );
    let (ahead, _) = r
        .ahead_behind(
            &RefName::new("main").unwrap(),
            &RefName::new("feature").unwrap(),
            0,
        )
        .unwrap();
    assert_eq!(ahead, gitraptor_git::Count::AtLeast(0));

    // The same walk by commit id, as for a detached `HEAD` (US-GRP-012).
    let tip = |name: &str| {
        r.resolve_ref(&RefName::new(name).unwrap())
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        r.ahead_behind_commits(&tip("main"), &tip("feature"), 1000)
            .unwrap(),
        (
            gitraptor_git::Count::Exact(1),
            gitraptor_git::Count::Exact(1)
        )
    );
    assert!(matches!(
        r.ahead_behind_commits("not-an-id", &tip("main"), 1000),
        Err(gitraptor_git::ReadError::InvalidInput(_))
    ));
}

#[test]
fn merge_in_progress_is_detected_without_writes() {
    let f = Fixture::with_commit();
    f.git(&["checkout", "-q", "-b", "other"]);
    f.write("a.txt", "other\n");
    f.git(&["commit", "-q", "-am", "other"]);
    f.git(&["checkout", "-q", "main"]);
    f.write("a.txt", "main\n");
    f.git(&["commit", "-q", "-am", "main"]);
    let out = f
        .git_command(&f.repo, &["merge", "other"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the merge must conflict");
    let before = f.fingerprint();

    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    assert_eq!(r.in_progress(), Some(InProgress::Merge));
    let s = r.status().unwrap();
    assert!(
        s.unstaged
            .iter()
            .any(|c| c.path == "a.txt" && c.kind == gitraptor_git::ChangeKind::Conflicted)
    );
    drop(r);
    assert_unchanged(&before, &f.fingerprint(), "reads during a merge");
}

#[test]
fn detached_and_unborn_heads() {
    let f = Fixture::new();
    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    assert!(r.head().unwrap().unborn);
    drop(r);
    let f = Fixture::with_commit();
    f.git(&["checkout", "-q", "--detach"]);
    let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    let head = r.head().unwrap();
    assert!(head.detached && head.commit.is_some() && head.branch.is_none());
}

#[test]
fn concurrent_agent_git_add_never_fails() {
    let f = busy_repo();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let repo = f.repo.clone();
    let reader = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut reads = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let r = RepoReader::open(&repo, &ReaderOptions::default()).unwrap();
                r.status().unwrap();
                r.index_entry_count().unwrap();
                reads += 1;
            }
            reads
        })
    };
    for i in 0..40 {
        f.write(&format!("agent-{i}.txt"), &format!("{i}\n"));
        let out = f
            .git_command(&f.repo, &["add", &format!("agent-{i}.txt")])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git add failed while reading: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(reader.join().unwrap() > 0);
}

/// Control run: plain `git status` refreshes the stat cache and rewrites `.git/index`, and the
/// fingerprint must see it. Otherwise the tests above would prove nothing.
#[test]
fn fingerprint_detects_plain_git_status_index_refresh() {
    let f = busy_repo();
    let before = f.fingerprint();
    f.git(&["status", "--porcelain"]);
    let after = f.fingerprint();
    assert_ne!(before, after, "the fingerprint missed the index refresh");
    assert_ne!(
        before.get(std::path::Path::new("repo/.git/index")),
        after.get(std::path::Path::new("repo/.git/index"))
    );
}
