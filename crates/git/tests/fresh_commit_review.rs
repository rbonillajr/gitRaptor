//! Regression tests for the review fixes of DS-US-GRD-009 § 10 (rename limit, basename pass,
//! extra parents already in the first parent's history). `git diff --numstat -M` is the oracle.
//! Read-only, on temporary repos (NFR-01).
//!
//! Not covered here: the "more than 100 000 paths are never read as complete" bound. Building a
//! 100 000-path commit is not cheap enough for this suite.

mod common;

use common::Fixture;
use gitraptor_git::{Hide, LineLimits, NewCommitChanges, PathLimits, ReaderOptions, RepoReader};

const MAIN: &[&str] = &["refs/heads/main"];
const FILES: usize = 1_100;

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(
        &f.repo,
        &ReaderOptions {
            ignore_ambient_config: true,
            ..ReaderOptions::default()
        },
    )
    .unwrap()
}

fn rev(f: &Fixture, what: &str) -> String {
    f.git(&["rev-parse", what]).trim().to_owned()
}

fn commit(f: &Fixture, message: &str) -> String {
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", message]);
    rev(f, "HEAD")
}

fn read(f: &Fixture, old: &str, new: &str, hide: Hide) -> NewCommitChanges {
    reader(f)
        .fresh_commit_changes(
            Some(old),
            new,
            MAIN,
            hide,
            &PathLimits::default(),
            Some(&LineLimits::default()),
        )
        .unwrap()
}

/// What `git diff --numstat -M` says of `parent..commit`, summed (binary entries count 0).
fn oracle_total(f: &Fixture, parent: &str, commit: &str) -> u32 {
    let out = f.git(&["diff", "--numstat", "-M", parent, commit]);
    out.lines()
        .map(|l| {
            let mut parts = l.split('\t');
            let n = |s: Option<&str>| s.and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
            n(parts.next()) + n(parts.next())
        })
        .sum()
}

/// What the reader reports for `oid`: the sum of its per-file lines.
fn reported_total(found: &NewCommitChanges, oid: &str) -> u32 {
    assert!(!found.unverifiable, "{found:?}");
    let c = found
        .commits
        .iter()
        .find(|c| c.oid == oid)
        .unwrap_or_else(|| panic!("commit {oid} is not among the new ones: {found:?}"));
    assert!(!c.partial, "nothing is cut on this corpus: {c:?}");
    c.files
        .as_deref()
        .map_or(0, |files| files.iter().map(|f| f.1).sum())
}

/// `n` files of 20 distinct lines under `dir`, named by `name(i)`; `edited` changes line 7.
fn write_files(f: &Fixture, dir: &str, name: impl Fn(usize) -> String, edited: bool) {
    for i in 0..FILES {
        let body: String = (1..=20)
            .map(|j| {
                if edited && j == 7 {
                    format!("edited {i}\n")
                } else {
                    format!("file {i} line {j}\n")
                }
            })
            .collect();
        f.write(&format!("{dir}/{}", name(i)), &body);
    }
}

#[test]
fn more_than_1000_renames_with_distinct_basenames_are_not_detected() {
    let f = Fixture::with_commit();
    write_files(&f, "src", |i| format!("old{i}.txt"), false);
    let before = commit(&f, "feat: many files");
    f.git(&["rm", "-rq", "src"]);
    write_files(&f, "dst", |i| format!("new{i}.txt"), true);
    let after = commit(&f, "refactor: rename everything");

    let git_total = oracle_total(&f, &before, &after);
    assert!(
        git_total > FILES as u32 * 20,
        "premise: this Git does not pair {FILES} renames of different basenames ({git_total})"
    );
    let found = read(&f, &before, &after, Hide::OtherBranches);
    assert_eq!(
        reported_total(&found, &after),
        git_total,
        "past the rename limit every file counts whole, as git counts it"
    );
}

#[test]
fn a_unique_basename_rename_is_detected_beyond_the_limit() {
    let f = Fixture::with_commit();
    write_files(&f, "src", |i| format!("keep{i}.txt"), false);
    let before = commit(&f, "feat: many files");
    f.git(&["rm", "-rq", "src"]);
    write_files(&f, "dst", |i| format!("keep{i}.txt"), true);
    let after = commit(&f, "refactor: move the directory");

    let git_total = oracle_total(&f, &before, &after);
    assert_eq!(
        git_total,
        FILES as u32 * 2,
        "premise: git pairs them by basename (one line out, one in each)"
    );
    let found = read(&f, &before, &after, Hide::OtherBranches);
    assert_eq!(
        reported_total(&found, &after),
        git_total,
        "the basename pass has no limit: renames are detected, not counted whole"
    );
}

#[test]
fn a_commit_whose_extra_parent_is_in_first_parent_history_counts_as_a_normal_commit() {
    let f = Fixture::with_commit();
    f.write("one.txt", "1\n");
    commit(&f, "feat: one");
    let head = rev(&f, "HEAD");
    f.write(
        "big.txt",
        &(1..=30).map(|i| format!("big {i}\n")).collect::<String>(),
    );
    f.git(&["add", "-A"]);
    let tree = f.git(&["write-tree"]).trim().to_owned();
    let odd = f
        .git(&[
            "commit-tree",
            &tree,
            "-p",
            "HEAD",
            "-p",
            "HEAD~1",
            "-m",
            "feat: odd parents",
        ])
        .trim()
        .to_owned();

    let found = read(&f, &head, &odd, Hide::OldOnly);
    assert_eq!(
        reported_total(&found, &odd),
        30,
        "an extra parent already in the first parent's history is not a merge: its lines count"
    );

    // Control: a real merge still counts 0.
    f.git(&["switch", "-q", "-c", "side", &head]);
    f.write("side.txt", "s\n");
    commit(&f, "feat: side");
    f.git(&["switch", "-q", "main"]);
    f.git(&["reset", "-q", "--hard", &head]);
    f.write("main.txt", "m\n");
    commit(&f, "feat: main");
    let before = rev(&f, "HEAD~1");
    f.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge branch 'side'",
        "side",
    ]);
    let merge = rev(&f, "HEAD");
    let found = read(&f, &before, &merge, Hide::OldOnly);
    assert_eq!(
        reported_total(&found, &merge),
        0,
        "a real merge counts no lines"
    );
}
