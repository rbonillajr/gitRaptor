//! Unit tests of the worktree diff reader. Child module of `diff`, mounted there as `mod tests`.

use std::time::Duration;

use gitraptor_testkit::Fixture;

use super::*;
use crate::ReaderOptions;

fn fixture() -> Fixture {
    Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path())
}

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
}

fn limits(deadline: Instant) -> DiffLimits {
    DiffLimits {
        max_files: 1_000,
        max_lines_per_file: 2_000,
        max_blob_bytes: 8 * 1024 * 1024,
        max_bytes: 768 * 1024,
        deadline,
    }
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn never() -> bool {
    false
}

/// `feat` branched from `main` with one commit; the repo is on `feat`.
fn feature_repo() -> Fixture {
    let f = fixture();
    f.git(&["checkout", "-q", "-b", "feat"]);
    f.write("feat.txt", "one\ntwo\n");
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "feat"]);
    f
}

fn main_ref() -> RefName {
    RefName::new("refs/heads/main").unwrap()
}

#[test]
fn committed_past_the_deadline_is_too_slow_never_an_empty_section() {
    let f = feature_repo();
    let base = main_ref();
    let req = DiffRequest {
        base: Some(&base),
        base_confirmed: true,
    };
    // The deadline is already here: listing the changed paths cannot finish.
    let diff = reader(&f)
        .worktree_diff(&req, &limits(Instant::now()), &never)
        .unwrap();
    assert_eq!(diff.committed, Err(CommittedGapKind::TooSlow));
}

#[test]
fn committed_lists_the_branch_against_the_merge_base() {
    let f = feature_repo();
    let base = main_ref();
    let req = DiffRequest {
        base: Some(&base),
        base_confirmed: true,
    };
    let diff = reader(&f)
        .worktree_diff(&req, &limits(later()), &never)
        .unwrap();
    let section = diff.committed.unwrap();
    assert_eq!(section.files_total, 1);
    assert_eq!(section.cut, None);
    let file = &section.files[0];
    assert_eq!(
        (file.path.as_str(), file.change),
        ("feat.txt", GitChange::Added)
    );
    assert_eq!((file.added, file.removed), (2, 0));
    assert!(diff.uncommitted.files.is_empty());
}

#[test]
fn committed_gaps_say_why() {
    let f = feature_repo();
    let r = reader(&f);
    let base = main_ref();
    let missing = RefName::new("refs/heads/nope").unwrap();
    let gap = |base: Option<&RefName>, confirmed| {
        let req = DiffRequest {
            base,
            base_confirmed: confirmed,
        };
        r.worktree_diff(&req, &limits(later()), &never)
            .unwrap()
            .committed
    };
    assert_eq!(gap(None, true), Err(CommittedGapKind::NoBase));
    assert_eq!(
        gap(Some(&base), false),
        Err(CommittedGapKind::BaseUnconfirmed)
    );
    assert_eq!(
        gap(Some(&missing), true),
        Err(CommittedGapKind::BaseMissing)
    );
}

#[test]
fn time_cut_keeps_the_paths_as_omitted_and_the_total() {
    let f = fixture();
    f.write("a.txt", "alpha edited\n");
    f.write("new.txt", "new\n");
    let req = DiffRequest {
        base: None,
        base_confirmed: false,
    };
    let diff = reader(&f)
        .worktree_diff(&req, &limits(Instant::now()), &never)
        .unwrap();
    let section = diff.uncommitted;
    assert_eq!(section.cut, Some(CutKind::Time));
    assert_eq!(section.files_total, 2);
    let names: Vec<_> = section.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, ["a.txt", "new.txt"]);
    assert!(section.files.iter().all(|f| f.body == GitBody::Omitted));
}

#[test]
fn the_stop_signal_is_asked_between_files() {
    let f = fixture();
    f.write("a.txt", "alpha edited\n");
    let req = DiffRequest {
        base: None,
        base_confirmed: false,
    };
    let diff = reader(&f)
        .worktree_diff(&req, &limits(later()), &|| true)
        .unwrap();
    assert!(
        diff.uncommitted
            .files
            .iter()
            .all(|f| f.body == GitBody::Omitted)
    );
}

#[test]
fn uncommitted_has_staged_unstaged_and_untracked_sorted_by_path() {
    let f = fixture();
    f.write(".gitignore", "*.log\n");
    f.write("a.txt", "alpha edited\n");
    f.write("staged.txt", "staged\n");
    f.git(&["add", "staged.txt"]);
    f.write("new.txt", "new\n");
    f.write("noise.log", "ignored\n");
    let req = DiffRequest {
        base: None,
        base_confirmed: false,
    };
    let diff = reader(&f)
        .worktree_diff(&req, &limits(later()), &never)
        .unwrap();
    let section = diff.uncommitted;
    let names: Vec<_> = section.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, [".gitignore", "a.txt", "new.txt", "staged.txt"]);
    assert_eq!(section.files_total, 4);
    let a = &section.files[1];
    assert_eq!(a.change, GitChange::Modified);
    assert_eq!((a.added, a.removed), (1, 1));
}

#[test]
fn the_file_cap_cuts_with_files_and_keeps_the_total() {
    let f = fixture();
    for i in 0..5 {
        f.write(&format!("n{i}.txt"), "x\n");
    }
    let req = DiffRequest {
        base: None,
        base_confirmed: false,
    };
    let mut caps = limits(later());
    caps.max_files = 3;
    let diff = reader(&f).worktree_diff(&req, &caps, &never).unwrap();
    assert_eq!(diff.uncommitted.cut, Some(CutKind::Files));
    assert_eq!(diff.uncommitted.files.len(), 3);
    assert_eq!(diff.uncommitted.files_total, 5);
}

#[test]
fn the_byte_budget_cuts_with_bytes_and_omits_the_rest() {
    let f = fixture();
    for i in 0..4 {
        f.write(&format!("n{i}.txt"), &"0123456789\n".repeat(100));
    }
    let req = DiffRequest {
        base: None,
        base_confirmed: false,
    };
    let mut caps = limits(later());
    caps.max_bytes = 4_000;
    let diff = reader(&f).worktree_diff(&req, &caps, &never).unwrap();
    let section = diff.uncommitted;
    assert_eq!(section.cut, Some(CutKind::Bytes));
    assert_eq!(section.files_total, 4);
    assert_eq!(section.files.len(), 4);
    assert!(
        section
            .files
            .iter()
            .any(|f| matches!(f.body, GitBody::Omitted))
    );
}

#[test]
fn text_diff_gives_hunks_with_three_lines_of_context() {
    let old: String = (1..=20).map(|i| format!("l{i}\n")).collect();
    let new = old.replace("l10\n", "ten\n");
    let (hunks, added, removed, truncated) = text_diff(old.as_bytes(), new.as_bytes(), 100);
    assert_eq!((added, removed, truncated), (1, 1, false));
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    assert_eq!(
        (h.old_start, h.old_len, h.new_start, h.new_len),
        (7, 7, 7, 7)
    );
    assert_eq!(h.lines.len(), 8);
    assert_eq!(h.lines[3].kind, GitLineKind::Removed);
    assert_eq!(h.lines[3].text, "l10");
    assert_eq!(h.lines[4].kind, GitLineKind::Added);
}

#[test]
fn text_diff_caps_the_lines_and_says_so() {
    let new: String = (0..50).map(|i| format!("line {i}\n")).collect();
    let (hunks, added, _, truncated) = text_diff(b"", new.as_bytes(), 10);
    assert!(truncated);
    assert_eq!(added, 50, "the counts are the real ones");
    assert_eq!(hunks[0].lines.len(), 10);
}

#[test]
fn a_nul_in_the_first_8000_bytes_is_binary_and_one_after_is_not() {
    let mut inside = vec![b'a'; 7_999];
    inside.push(0);
    let mut beyond = vec![b'a'; 8_000];
    beyond.push(0);
    assert!(is_binary(&inside));
    assert!(!is_binary(&beyond));
}

#[cfg(unix)]
#[test]
fn the_working_tree_is_read_without_following_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("wt");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(root.join("real")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("f.txt"), "secret\n").unwrap();
    std::fs::write(root.join("real/f.txt"), "inside\n").unwrap();
    std::os::unix::fs::symlink(outside.join("f.txt"), root.join("file-link")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("dir-link")).unwrap();
    let read = |comps: &[&str]| read_worktree(&root, comps, 1024);
    assert!(matches!(read(&["real", "f.txt"]), Side::Blob(b) if b == b"inside\n"));
    assert!(matches!(read(&["file-link"]), Side::Link));
    assert!(matches!(read(&["dir-link", "f.txt"]), Side::Link));
    assert!(matches!(read(&["missing"]), Side::Absent));
    assert!(matches!(read(&["real", "f.txt", "deeper"]), Side::Absent));
    assert!(matches!(read(&["real"]), Side::Submodule));
    assert!(matches!(
        read_worktree(&root, &["real", "f.txt"], 3),
        Side::TooLarge
    ));
}
