//! What S3 reads of a `git`'s command line (ADR-GRP-012, amendments 2026-10-28): whether it
//! cannot make a Git event (`harmless`, for a foreign `git`) and whether it can move a ref or
//! `HEAD` (`writes`, for a `git` of a session).

use std::ffi::OsString;

use super::git_call;

fn call(line: &str) -> super::GitCall {
    let args: Vec<OsString> = line.split_whitespace().map(OsString::from).collect();
    git_call(&args)
}

#[test]
fn a_git_that_cannot_make_an_event_is_harmless() {
    for line in [
        "log --oneline",
        "-c core.pager=x --paginate log",
        "--no-optional-locks status --porcelain",
        "status",
        "fsmonitor--daemon run",
        "cat-file --batch",
        "rev-parse --show-toplevel",
        "config --get remote.origin.url",
        "for-each-ref",
    ] {
        let c = call(line);
        assert!(c.harmless, "{line}");
        assert!(!c.writes, "{line}");
    }
}

#[test]
fn a_git_that_may_write_or_is_unknown_is_not_harmless() {
    for line in [
        "commit -m x",
        "config user.name x",
        "diff --output=/tmp/x",
        "fetch origin",
        "gc --auto",
        "maintenance run --auto",
        // An alias, and the sanitized form of one: `stat.us` may run anything.
        "st",
        "stat.us",
        "Status",
        "",
    ] {
        assert!(!call(line).harmless, "{line}");
    }
}

#[test]
fn only_a_ref_writer_writes() {
    assert!(call("-c core.hooksPath=/h commit -q -m x").writes);
    assert!(call("reset --hard HEAD~1").writes);
    assert!(!call("fetch origin").writes);
    assert!(!call("ci -m x").writes);
    // The calm's reading is unchanged: `status` may refresh the index.
    assert!(!call("status").read_only);
}
