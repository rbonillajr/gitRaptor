//! Resolution of the system Git on Windows (SEC-10, TD-GRP-001): owner and DACL of the
//! executable and its folders, checked on the real machine. Hostile entries are added with
//! `icacls` to temporary folders only.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gitraptor_git::Invoker;
use gitraptor_git::resolve::{self, Rejection, Resolution, ResolveConfig, check_executable};

const PROGRAM_FILES_GIT: &str = r"C:\Program Files\Git\cmd\git.exe";
const USERS: &str = "*S-1-5-32-545";

fn icacls(path: &Path, args: &[&str]) {
    let status = Command::new("icacls")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "icacls {path:?} {args:?}: {status:?}"
    );
}

/// A fake `git.exe` (never launched) in `<tmp>\<dir>`.
fn fake(tmp: &Path, dir: &str) -> PathBuf {
    let d = tmp.join(dir);
    std::fs::create_dir_all(&d).unwrap();
    let git = d.join("git.exe");
    std::fs::write(&git, b"MZ").unwrap();
    git
}

#[test]
fn program_files_git_is_accepted_and_resolves() {
    if !Path::new(PROGRAM_FILES_GIT).exists() {
        eprintln!("skipped: no Git in {PROGRAM_FILES_GIT}");
        return;
    }
    let canonical = check_executable(Path::new(PROGRAM_FILES_GIT)).expect("accepted");
    let config = ResolveConfig {
        configured_path: Some(PROGRAM_FILES_GIT.into()),
        path_env: None,
        known_locations: vec![],
        shim_paths: vec![],
        toolchain_gits: vec![],
    };
    match resolve::resolve(&config, &Invoker::default()) {
        Resolution::Found { git, .. } => assert_eq!(git.path, canonical),
        other => panic!("not found: {other:?}"),
    }
}

#[test]
fn fresh_private_folder_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let git = fake(tmp.path(), "ok");
    check_executable(&git).expect("accepted");
}

#[test]
fn git_writable_by_users_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let git = fake(tmp.path(), "hostile");
    icacls(&git, &["/grant", &format!("{USERS}:(M)")]);
    assert_eq!(check_executable(&git), Err(Rejection::WritableByOthers));
}

#[test]
fn folder_where_users_can_add_files_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let git = fake(tmp.path(), "planting");
    icacls(git.parent().unwrap(), &["/grant", &format!("{USERS}:(WD)")]);
    assert_eq!(check_executable(&git), Err(Rejection::WritableByOthers));
}

#[test]
fn ancestor_where_everyone_can_delete_children_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let git = fake(tmp.path(), r"a\b");
    let ancestor = tmp.path().join("a");
    // Adding subfolders above is harmless (as `C:\` grants it); deleting children is not.
    icacls(&ancestor, &["/grant", "*S-1-1-0:(AD)"]);
    check_executable(&git).expect("add-subfolder above is accepted");
    icacls(&ancestor, &["/grant", "*S-1-1-0:(DC)"]);
    assert_eq!(check_executable(&git), Err(Rejection::WritableByOthers));
}

#[test]
fn git_owned_by_users_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let git = fake(tmp.path(), "owner");
    icacls(&git, &["/setowner", USERS]);
    assert_eq!(check_executable(&git), Err(Rejection::UntrustedOwner));
}

#[test]
fn junction_resolves_to_its_verified_target() {
    let tmp = tempfile::tempdir().unwrap();
    let git = fake(tmp.path(), "real");
    let link = tmp.path().join("link");
    let out = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(tmp.path().join("real"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    // The path that runs is the canonical target, which is what gets verified.
    let canonical = check_executable(&link.join("git.exe")).expect("accepted");
    assert_eq!(canonical, git.canonicalize().unwrap());
    // A path that goes through the junction is never accepted as such.
    assert!(matches!(
        gitraptor_winsys::acl::verify_trusted_executable(&link.join("git.exe")),
        Err(gitraptor_winsys::acl::AclError::ReparsePoint)
    ));
}
