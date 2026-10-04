//! SEC-06 / H4: `GITRAPTOR_PROFILE_DIR` redirects the profile only in builds
//! with `debug_assertions`; a release build ignores it. The variable is set
//! on a child process (never on this one) to keep tests independent.
//! Run `cargo test --release` to exercise the release branch.

use std::process::Command;

use gitraptor_core::profile::{PROFILE_DIR_ENV, ProfileDirs};

const CHILD_ENV: &str = "GITRAPTOR_TEST_PRINT_DIRS";

#[test]
fn print_dirs_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    // Only resolves paths; nothing is created.
    let dirs = ProfileDirs::resolve().unwrap();
    println!("DATA={}", dirs.data.display());
}

#[test]
fn profile_override_only_in_debug_builds() {
    let root = tempfile::tempdir().unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "print_dirs_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .env(PROFILE_DIR_ENV, root.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let data = stdout
        .lines()
        .find_map(|l| l.split_once("DATA=").map(|(_, rest)| rest))
        .expect("child printed its data folder");
    let overridden = std::path::Path::new(data).starts_with(root.path());
    if cfg!(debug_assertions) {
        assert!(overridden, "debug build must honor the override: {data}");
    } else {
        assert!(
            !overridden,
            "release build must ignore the override: {data}"
        );
    }
}
