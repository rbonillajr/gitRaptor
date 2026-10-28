//! SEC-11 (ADR-GRP-010 § 2): every home folder is refused as the root of a linked worktree,
//! whatever form its path is written in. On Windows Git often sets `HOME` to another folder
//! than the profile (`USERPROFILE`), and both must be refused.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gitraptor_core::observe::{canonical, home_folders_from, linked_is_trusted_in};

/// A bare common directory with one linked worktree `wt1` rooted at `root`, its `.git` pointing
/// back as it should.
fn linked(base: &Path, root: &Path) -> PathBuf {
    let common = base.join("shop.git");
    let admin = common.join("worktrees").join("wt1");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    common
}

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
    move |name| {
        vars.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| OsString::from(v))
    }
}

#[test]
fn the_home_folders_are_home_and_on_windows_the_profile_too() {
    let both = home_folders_from(env(&[("HOME", "/h"), ("USERPROFILE", "/p")]));
    let expected: Vec<PathBuf> = if cfg!(windows) {
        vec!["/h".into(), "/p".into()]
    } else {
        vec!["/h".into()]
    };
    assert_eq!(both, expected);

    // Without `HOME` (the daemon started on demand on Windows has none), the profile is still
    // refused there; an empty value is no folder.
    let profile_only = home_folders_from(env(&[("HOME", ""), ("USERPROFILE", "/p")]));
    let expected: Vec<PathBuf> = if cfg!(windows) {
        vec!["/p".into()]
    } else {
        vec![]
    };
    assert_eq!(profile_only, expected);
}

/// `HOME=D:\home` and the root at `USERPROFILE`: the second folder counts as much as the first.
#[test]
fn a_root_at_any_home_folder_is_untrusted() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let (profile, git_home) = (base.join("profile"), base.join("git-home"));
    std::fs::create_dir_all(&git_home).unwrap();
    let common = linked(&base, &profile);

    assert!(linked_is_trusted_in(
        &common,
        "wt1",
        &profile,
        std::slice::from_ref(&git_home)
    ));
    assert!(!linked_is_trusted_in(
        &common,
        "wt1",
        &profile,
        &[git_home, profile.clone()]
    ));
}

/// A root written in another form than the home folder (on Windows, the verbatim `\\?\C:\…`
/// of `std::fs::canonicalize` against the drive form) is still the home folder.
#[test]
fn a_home_written_another_way_is_still_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let home = base.join("home");
    let common = linked(&base, &home);
    let other_form = std::fs::canonicalize(&home).unwrap();

    assert!(!linked_is_trusted_in(
        &common,
        "wt1",
        &other_form,
        std::slice::from_ref(&home)
    ));
    assert!(!linked_is_trusted_in(&common, "wt1", &home, &[other_form]));
}

/// An ancestor of the repo is refused whatever form either path is written in.
#[test]
fn an_ancestor_of_the_repo_written_another_way_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let common = linked(&base, &base.join("wt"));
    // The repo's own parent claims to be the worktree.
    std::fs::write(
        base.join(".git"),
        format!(
            "gitdir: {}\n",
            common.join("worktrees").join("wt1").display()
        ),
    )
    .unwrap();
    let other_form = std::fs::canonicalize(&base).unwrap();

    assert!(!linked_is_trusted_in(&common, "wt1", &base, &[]));
    assert!(!linked_is_trusted_in(&common, "wt1", &other_form, &[]));
    let common_other_form = std::fs::canonicalize(&common).unwrap();
    assert!(!linked_is_trusted_in(&common_other_form, "wt1", &base, &[]));
}
