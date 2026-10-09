//! Contract tests of `parse_procargs2_git_redirect` over synthetic
//! `KERN_PROCARGS2` areas: argc, the executable path, NUL padding, the argv
//! and then the `KEY=VALUE` strings of the environment.

use super::parse_procargs2_git_redirect;

/// An area with `args` as the argv (`git` first) and `env` after it.
fn area(args: &[&str], env: &[&str]) -> Vec<u8> {
    let argc = i32::try_from(args.len()).unwrap();
    let mut v = argc.to_ne_bytes().to_vec();
    v.extend_from_slice(b"/usr/bin/git\0\0\0\0");
    for s in args.iter().chain(env) {
        v.extend_from_slice(s.as_bytes());
        v.push(0);
    }
    v
}

fn raw(argc: i32, body: &[u8]) -> Vec<u8> {
    let mut v = argc.to_ne_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

const ENV: &[&str] = &["HOME=/Users/u", "PATH=/usr/bin"];

#[test]
fn a_global_dash_c_upper_redirects() {
    let a = area(&["git", "-C", "/wt/other", "status"], ENV);
    assert_eq!(parse_procargs2_git_redirect(&a), Some(true));
}

#[test]
fn git_dir_and_work_tree_redirect_in_both_forms() {
    for args in [
        &["git", "--git-dir=/r/.git/worktrees/x", "commit"][..],
        &["git", "--git-dir", "/r/.git/worktrees/x", "commit"][..],
        &["git", "--work-tree=/wt/other", "commit"][..],
        &["git", "--work-tree", "/wt/other", "commit"][..],
        &["git", "-c", "k=v", "-C", "/wt/other", "status"][..],
    ] {
        assert_eq!(
            parse_procargs2_git_redirect(&area(args, ENV)),
            Some(true),
            "{args:?}"
        );
    }
}

#[test]
fn the_environment_names_redirect() {
    for var in [
        "GIT_WORK_TREE=/wt/other",
        "GIT_DIR=/r/.git",
        "GIT_COMMON_DIR=/r/.git",
    ] {
        let a = area(&["git", "status"], &["HOME=/Users/u", var, "PATH=/usr/bin"]);
        assert_eq!(parse_procargs2_git_redirect(&a), Some(true), "{var}");
    }
    // The environment may end in NUL padding.
    let mut a = area(&["git", "status"], &["GIT_WORK_TREE=/wt/other"]);
    a.extend_from_slice(b"\0\0\0");
    assert_eq!(parse_procargs2_git_redirect(&a), Some(true));
}

#[test]
fn options_after_the_subcommand_and_config_values_do_not_redirect() {
    for args in [
        &["git", "-c", "k=v", "status"][..],
        &["git", "-c", "core.x=1", "status"][..],
        &["git", "log", "-C"][..],
        &["git", "commit", "-C", "HEAD"][..],
        &["git", "status"][..],
        &["git"][..],
    ] {
        assert_eq!(
            parse_procargs2_git_redirect(&area(args, ENV)),
            Some(false),
            "{args:?}"
        );
    }
}

/// Only the names count: a value that mentions a variable, a longer name or
/// an empty environment is no redirect.
#[test]
fn the_environment_is_compared_by_name_only() {
    let a = area(
        &["git", "status"],
        &[
            "FOO=GIT_DIR=/x",
            "GIT_DIRX=1",
            "MY_GIT_DIR=1",
            "GIT_AUTHOR_NAME=u",
        ],
    );
    assert_eq!(parse_procargs2_git_redirect(&a), Some(false));
    assert_eq!(
        parse_procargs2_git_redirect(&area(&["git", "status"], &[])),
        Some(false)
    );
}

#[test]
fn a_malformed_area_is_unreadable() {
    assert_eq!(parse_procargs2_git_redirect(&[]), None);
    assert_eq!(parse_procargs2_git_redirect(&[1, 0]), None);
    assert_eq!(parse_procargs2_git_redirect(&raw(0, b"/x\0git\0")), None);
    assert_eq!(parse_procargs2_git_redirect(&raw(-1, b"/x\0git\0")), None);
    // Fewer strings than argc.
    assert_eq!(
        parse_procargs2_git_redirect(&raw(3, b"/x\0\0git\0status")),
        None
    );
    // No terminated executable path.
    assert_eq!(parse_procargs2_git_redirect(&raw(1, b"/x")), None);
}
