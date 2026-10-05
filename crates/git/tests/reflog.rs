//! Reflogs, remote-tracking branches and upstreams (US-GRP-002): what the observer reads to
//! name a Git event. Read-only, on temporary repos (NFR-01).

mod common;

use common::Fixture;
use gitraptor_git::{ReaderOptions, RepoReader};

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
}

fn head(f: &Fixture) -> String {
    f.git(&["rev-parse", "HEAD"]).trim().to_owned()
}

#[test]
fn every_entry_since_the_previous_tip_is_read_newest_first() {
    let f = Fixture::with_commit();
    let before = head(&f);
    f.write("a.txt", "one\n");
    f.git(&["commit", "-qam", "one"]);
    f.write("a.txt", "two\n");
    f.git(&["commit", "-qam", "two"]);

    let entries = reader(&f)
        .reflog_since("refs/heads/main", Some(&before), 50)
        .unwrap();
    let messages: Vec<_> = entries.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages, ["commit: two", "commit: one"]);
    assert_eq!(entries[1].old, before);
    assert_eq!(entries[0].new, head(&f));

    // A new ref: only its newest entry. No reflog: nothing.
    let newest = reader(&f)
        .reflog_since("refs/heads/main", None, 50)
        .unwrap();
    assert_eq!(newest.len(), 1);
    assert!(
        reader(&f)
            .reflog_since("refs/heads/missing", None, 50)
            .unwrap()
            .is_empty()
    );
    assert!(reader(&f).reflog_since("main", None, 50).is_err());
}

#[test]
fn head_reflog_names_the_checkout() {
    let f = Fixture::with_commit();
    f.git(&["switch", "-q", "-c", "feat"]);
    f.git(&["switch", "-q", "main"]);
    let last = reader(&f).reflog_last("HEAD").unwrap().unwrap();
    assert_eq!(last.message, "checkout: moving from feat to main");
}

#[test]
fn remote_branches_and_upstreams_are_read() {
    let f = Fixture::with_commit();
    let remote = f.root().join("remote.git");
    f.git(&["init", "-q", "--bare", remote.to_str().unwrap()]);
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    f.git(&["push", "-q", "-u", "origin", "main"]);

    let r = reader(&f);
    let remotes = r.remote_branches().unwrap();
    assert_eq!(remotes.len(), 1);
    assert_eq!(remotes[0].name, "origin/main");
    assert_eq!(remotes[0].commit, head(&f));
    assert_eq!(r.branch_upstream("main").as_deref(), Some("origin/main"));
    assert_eq!(r.branch_upstream("feat"), None);
    let pushed = r
        .reflog_since("refs/remotes/origin/main", None, 50)
        .unwrap();
    assert_eq!(pushed[0].message, "update by push");
}
