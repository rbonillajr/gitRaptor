//! The shape of a ref update the second line evaluates (DS-US-GRD-018 § 5.3): one new commit
//! (commit, merge, amend, root), and nothing else. Read-only, on temporary repos (NFR-01).

mod common;

use common::Fixture;
use gitraptor_git::{CommitShape, ReaderOptions, RepoReader};

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

/// A commit object on top of `parents`, reached by no ref (what `commit --no-verify` leaves
/// before the ref moves).
fn dangling(f: &Fixture, message: &str, parents: &[&str]) -> String {
    let tree = rev(f, "HEAD^{tree}");
    let mut args = vec!["commit-tree", tree.as_str(), "-m", message];
    for p in parents {
        args.extend(["-p", p]);
    }
    f.git(&args).trim().to_owned()
}

fn one(message: &str) -> CommitShape {
    CommitShape::One {
        message: Some(format!("{message}\n").into_bytes()),
    }
}

const MAIN: &[&str] = &["refs/heads/main"];

#[test]
fn a_new_commit_a_merge_and_an_amend_are_one_commit() {
    let f = Fixture::with_commit();
    let r = reader(&f);
    let head = rev(&f, "HEAD");
    let new = dangling(&f, "feat: x", &[&head]);
    assert_eq!(
        r.commit_shape(Some(&head), &new, MAIN, 1024).unwrap(),
        one("feat: x")
    );

    // Amend: the parents of the old tip.
    f.write("b.txt", "b\n");
    f.git(&["add", "b.txt"]);
    f.git(&["commit", "-qm", "second"]);
    let tip = rev(&f, "HEAD");
    let amended = dangling(&f, "amended", &[&head]);
    assert_eq!(
        r.commit_shape(Some(&tip), &amended, MAIN, 1024).unwrap(),
        one("amended")
    );

    // Merge: first parent is the old tip.
    let merge = dangling(&f, "merge", &[&tip, &head]);
    assert_eq!(
        r.commit_shape(Some(&tip), &merge, MAIN, 1024).unwrap(),
        one("merge")
    );

    // A root commit on a new ref.
    let root = dangling(&f, "root", &[]);
    assert_eq!(
        r.commit_shape(None, &root, &["refs/heads/orphan"], 1024)
            .unwrap(),
        one("root")
    );
}

#[test]
fn a_fast_forward_several_commits_and_no_change_are_not_one_commit() {
    let f = Fixture::with_commit();
    let base = rev(&f, "HEAD");
    f.git(&["switch", "-qc", "side"]);
    f.write("s.txt", "s\n");
    f.git(&["add", "s.txt"]);
    f.git(&["commit", "-qm", "side one"]);
    let side1 = rev(&f, "HEAD");
    f.write("s.txt", "t\n");
    f.git(&["commit", "-qam", "side two"]);
    let side2 = rev(&f, "HEAD");
    f.git(&["switch", "-q", "main"]);
    let r = reader(&f);

    // Fast-forward of main to a commit another ref already has (the tip or below it).
    assert_eq!(
        r.commit_shape(Some(&base), &side1, MAIN, 1024).unwrap(),
        CommitShape::Other
    );
    f.git(&["branch", "-qf", "side", &side2]);
    assert_eq!(
        r.commit_shape(Some(&side1), &side2, MAIN, 1024).unwrap(),
        CommitShape::Other
    );
    // Two new commits at once.
    let a = dangling(&f, "a", &[&base]);
    let b = dangling(&f, "b", &[&a]);
    assert_eq!(
        r.commit_shape(Some(&base), &b, MAIN, 1024).unwrap(),
        CommitShape::Other
    );
    // No change; a branch created on an existing commit.
    assert_eq!(
        r.commit_shape(Some(&base), &base, MAIN, 1024).unwrap(),
        CommitShape::Other
    );
    assert_eq!(
        r.commit_shape(None, &side1, &["refs/heads/x"], 1024)
            .unwrap(),
        CommitShape::Other
    );
}

#[test]
fn a_message_over_the_limit_is_not_read() {
    let f = Fixture::with_commit();
    let head = rev(&f, "HEAD");
    let new = dangling(&f, &"a".repeat(100), &[&head]);
    assert_eq!(
        reader(&f)
            .commit_shape(Some(&head), &new, MAIN, 10)
            .unwrap(),
        CommitShape::One { message: None }
    );
}
