//! The commits a ref update brings, counted without reading trees: how many are new and how many
//! of them close a merge (the facts the permission classifier reads). Read-only, on temporary
//! repos (NFR-01).

mod common;

use common::Fixture;
use gitraptor_git::{Hide, NewCommits, PathLimits, ReaderOptions, RepoReader};

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

const MAIN: &[&str] = &["refs/heads/main"];

fn read(f: &Fixture, old: Option<&str>, new: &str, hide: Hide, limits: &PathLimits) -> NewCommits {
    reader(f)
        .fresh_commits(old, new, MAIN, hide, limits)
        .unwrap()
}

fn counts(found: &NewCommits) -> (usize, usize) {
    assert!(!found.unverifiable, "{found:?}");
    (found.commits, found.merges)
}

#[test]
fn a_commit_is_one_new_commit_and_several_are_counted() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("a.txt", "one\n");
    let c1 = commit(&f, "one");
    let found = read(
        &f,
        Some(&c0),
        &c1,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (1, 0));
    f.write("a.txt", "two\n");
    commit(&f, "two");
    f.write("a.txt", "three\n");
    let c3 = commit(&f, "three");
    let found = read(
        &f,
        Some(&c0),
        &c3,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (3, 0));
}

#[test]
fn a_merge_commit_counts_as_a_commit_and_as_a_merge() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.git(&["switch", "-q", "-c", "side"]);
    f.write("s.txt", "s\n");
    commit(&f, "side");
    f.git(&["switch", "-q", "main"]);
    f.write("m.txt", "m\n");
    let m1 = commit(&f, "main");
    f.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
    let merged = rev(&f, "HEAD");
    // `side` holds its commit: only the merge commit is new.
    let found = read(
        &f,
        Some(&m1),
        &merged,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (1, 1));
    // Without `side` the commit it carried is new too.
    f.git(&["branch", "-D", "side"]);
    let found = read(
        &f,
        Some(&m1),
        &merged,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (2, 1));
    let _ = c0;
}

#[test]
fn an_amend_brings_one_new_commit() {
    let f = Fixture::with_commit();
    f.write("a.txt", "one\n");
    let c1 = commit(&f, "one");
    f.git(&["commit", "-q", "--amend", "-m", "one, reworded"]);
    let amended = rev(&f, "HEAD");
    assert_ne!(c1, amended);
    let found = read(
        &f,
        Some(&c1),
        &amended,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (1, 0));
}

#[test]
fn a_fast_forward_to_an_existing_commit_brings_nothing() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("a.txt", "one\n");
    let c1 = commit(&f, "one");
    // Another branch already holds `c1`: moving main there creates no commit.
    f.git(&["branch", "ahead", &c1]);
    f.git(&["reset", "-q", "--hard", &c0]);
    let found = read(
        &f,
        Some(&c0),
        &c1,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (0, 0));
    // A detached commit nothing else reaches is new, and the cheap first look cannot tell.
    f.git(&["branch", "-D", "ahead"]);
    let found = read(
        &f,
        Some(&c0),
        &c1,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(counts(&found), (1, 0));
    let cheap = read(&f, Some(&c0), &c1, Hide::OldOnly, &PathLimits::default());
    assert_eq!(counts(&cheap), (1, 0));
}

#[test]
fn a_creation_counts_what_no_other_branch_holds() {
    let f = Fixture::with_commit();
    f.write("a.txt", "one\n");
    let c1 = commit(&f, "one");
    // `main` is the ref being created (no old value): the root commit and `one` are new.
    let found = read(&f, None, &c1, Hide::OtherBranches, &PathLimits::default());
    assert_eq!(counts(&found), (2, 0));
    // Another branch holds the root: only `one` is new.
    f.git(&["branch", "root", "HEAD~1"]);
    let found = read(&f, None, &c1, Hide::OtherBranches, &PathLimits::default());
    assert_eq!(counts(&found), (1, 0));
}

#[test]
fn the_bounds_are_fail_closed() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    for n in 0..3 {
        f.write(&format!("f{n}.txt"), "x\n");
        commit(&f, &format!("c{n}"));
    }
    let c3 = rev(&f, "HEAD");
    let tight = |limits: PathLimits| read(&f, Some(&c0), &c3, Hide::OtherBranches, &limits);
    let default = PathLimits::default();
    assert!(!tight(default).unverifiable);
    assert!(
        tight(PathLimits {
            commits: 2,
            ..default
        })
        .unverifiable
    );
    assert!(
        tight(PathLimits {
            visited: 2,
            ..default
        })
        .unverifiable
    );
    // More tips than hidden: what the rest hold counts as new, never as nothing.
    f.git(&["branch", "other", &c3]);
    let found = tight(PathLimits { tips: 0, ..default });
    assert!(!found.unverifiable);
    assert_eq!(found.commits, 3);
    assert_eq!(tight(default).commits, 0);
}

#[test]
fn an_object_missing_from_the_repo_cannot_be_verified() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    let missing = "1".repeat(40);
    let found = reader(&f).fresh_commits(
        Some(&c0),
        &missing,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert!(
        found.is_err() || found.unwrap().unverifiable,
        "a missing tip is never read as 'nothing new'"
    );
}
