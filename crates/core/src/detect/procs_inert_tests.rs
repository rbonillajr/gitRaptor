//! Contract tests of TD-GRP-007 (written by the criterion-tester): `git_call` reads the
//! daemon's inert argv (GRP007-CALL).

use super::*;

fn os(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// The real argv of the daemon's `git` whose subcommand starts with `first_word`.
fn daemon_argv(first_word: &str) -> Vec<OsString> {
    gitraptor_git::daemon_git_argvs()
        .into_iter()
        .find(|(word, _)| *word == first_word)
        .unwrap_or_else(|| panic!("daemon_git_argvs lists `{first_word}`"))
        .1
}

#[test]
fn inert_is_set_for_the_real_argv_of_an_inert_daemon_git() {
    for first_word in [
        "update-index",
        "pack-objects",
        "index-pack",
        "repack",
        "prune",
        "rev-parse",
        "for-each-ref",
        "rev-list",
        "merge-base",
        "log",
        "version",
    ] {
        let argv = daemon_argv(first_word);
        assert!(
            git_call(&argv).inert,
            "GRP007-CALL: git_call must mark the daemon's real `{first_word}` argv inert: {argv:?}"
        );
    }
}

#[test]
fn inert_is_not_set_for_update_ref_of_the_daemon() {
    let argv = daemon_argv("update-ref");
    assert!(
        !git_call(&argv).inert,
        "GRP007-CALL: the daemon's update-ref argv must never be inert"
    );
    // The same call also proves the positive side of the parser on a neighbour.
    assert!(
        git_call(&daemon_argv("pack-objects")).inert,
        "GRP007-CALL: the daemon's pack-objects argv must be inert"
    );
}

#[test]
fn inert_is_not_set_without_the_fixed_options() {
    assert!(
        git_call(&daemon_argv("update-index")).inert,
        "GRP007-CALL: the real update-index argv must be inert (baseline of this case)"
    );
    for (label, argv) in [
        (
            "git update-index -z --index-info",
            os(&["update-index", "-z", "--index-info"]),
        ),
        ("git status", os(&["status"])),
        ("git log", os(&["log", "-1"])),
        ("git rev-parse", os(&["rev-parse", "HEAD"])),
    ] {
        assert!(
            !git_call(&argv).inert,
            "GRP007-CALL: `{label}` has no fixed options and must not be inert"
        );
    }
}
