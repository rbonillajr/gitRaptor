//! `changed_paths` on a hostile tree (H-01, US-TMC-006): a tree nested thousands of levels deep
//! must be refused, never walked into a stack overflow that would take the daemon down. Built by
//! Git itself in a temporary repo (NFR-01).

mod common;

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::Fixture;
use gitraptor_git::{ReadError, ReaderOptions, RepoReader};

#[test]
fn a_very_deep_tree_is_unavailable_and_does_not_crash() {
    let f = Fixture::with_commit();
    let old = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    // A commit whose tree nests 6000 directories, made by `git fast-import` in one process.
    let path = format!("{}leaf", "d/".repeat(6000));
    let stream = format!(
        "commit refs/heads/deep\ncommitter t <t@example.com> 0 +0000\ndata 4\ndeep\n\
         M 100644 inline {path}\ndata 2\nx\n\n"
    );
    let mut child = Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(&f.repo)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let deep = f.git(&["rev-parse", "refs/heads/deep"]);

    let reader = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    assert!(matches!(
        reader.changed_paths(Some(&old), deep.trim(), 20, deadline),
        Err(ReadError::Unavailable(_))
    ));
}
