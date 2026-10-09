//! Contract tests of `git_redirects`: which global options and environment
//! names make a `git` write somewhere other than its own folder.

use std::ffi::{OsStr, OsString};

use super::git_redirects;

fn args(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

fn redirects(line: &str) -> bool {
    git_redirects(&args(line), std::iter::empty::<&OsStr>())
}

fn redirects_with_env(line: &str, names: &[&str]) -> bool {
    git_redirects(&args(line), names.iter().map(OsStr::new))
}

#[test]
fn dash_c_upper_before_the_subcommand_redirects() {
    assert!(redirects("-C /wt/other status"));
    assert!(redirects("-C . -C /wt/other commit -m x"));
}

#[test]
fn work_tree_redirects_with_or_without_equals() {
    assert!(redirects("--work-tree=/wt/other commit -m x"));
    assert!(redirects("--work-tree /wt/other commit -m x"));
}

#[test]
fn git_dir_redirects_with_or_without_equals() {
    assert!(redirects("--git-dir=/r/.git/worktrees/x commit -m x"));
    assert!(redirects("--git-dir /r/.git/worktrees/x commit -m x"));
}

#[test]
fn a_redirect_after_another_global_option_counts() {
    assert!(redirects("-c core.x=1 -C /wt/other status"));
    assert!(redirects("--no-pager --git-dir=/r/.git log"));
}

#[test]
fn the_environment_names_redirect() {
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"] {
        assert!(
            redirects_with_env("commit -m x", &["HOME", name, "PATH"]),
            "{name}"
        );
    }
}

#[test]
fn options_of_the_subcommand_do_not_redirect() {
    // `-C` of `log` (copy detection), not the global one.
    assert!(!redirects("log -C"));
    assert!(!redirects("commit -C HEAD"));
    assert!(!redirects("status --git-dir-like"));
    assert!(!redirects("rebase --work-tree=/x"));
}

#[test]
fn a_config_value_is_skipped_and_does_not_redirect() {
    assert!(!redirects("-c core.x=1 status"));
    assert!(!redirects("-c k=v status"));
    assert!(!redirects("--namespace=ns status"));
}

#[test]
fn plain_git_and_unrelated_environment_do_not_redirect() {
    assert!(!redirects(""));
    assert!(!redirects("status"));
    assert!(!redirects_with_env(
        "status",
        &["HOME", "PATH", "GIT_AUTHOR_NAME", "GIT_DIRX", "MY_GIT_DIR"]
    ));
}
