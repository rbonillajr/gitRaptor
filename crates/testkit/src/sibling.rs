//! Binaries of another workspace package, next to the binary under test.
//!
//! An integration test only gets `CARGO_BIN_EXE_*` for its own package's binaries. When it also
//! needs one from another package (`raptor-mcp` for the `raptor` tests), the binary must be in
//! the same target directory and profile, or the test runs a stale one or none at all. `cargo
//! test --workspace` builds it there already; `nx` runs `cargo test -p <package>` with
//! `--target-dir dist/target/<project>` while exporting a different `CARGO_TARGET_DIR`, so a bare
//! nested `cargo build` would put it somewhere else. Here the target directory, the profile and
//! the target triple are read from the path of the reference binary instead.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// One nested build at a time within a test binary; cargo's own lock covers other processes.
static BUILD: Mutex<()> = Mutex::new(());

/// `bin` of `package`, next to `next_to` (a `CARGO_BIN_EXE_*` path); built there first, with
/// the same target directory, profile and target triple, if it is not there yet.
pub fn sibling_bin(next_to: &Path, package: &str, bin: &str) -> PathBuf {
    let exe = next_to.with_file_name(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    let _build = BUILD.lock().unwrap_or_else(|e| e.into_inner());
    if exe.exists() {
        return exe;
    }
    let profile_dir = next_to
        .parent()
        .expect("a binary inside a profile directory");
    let profile = match profile_dir.file_name().and_then(|n| n.to_str()) {
        Some("debug") => "dev",
        Some(name) => name,
        None => panic!("no profile directory above {}", next_to.display()),
    };
    let above = profile_dir
        .parent()
        .expect("a profile directory inside a target directory");
    // Cargo tags the root of every target directory; without the tag, `above` is the
    // `<target>/<triple>` of a cross build.
    let mut args: Vec<OsString> = ["build", "-q", "-p", package, "--bin", bin, "--profile"]
        .map(OsString::from)
        .into();
    args.push(profile.into());
    let target_dir = if above.join("CACHEDIR.TAG").exists() {
        above
    } else {
        args.push("--target".into());
        args.push(above.file_name().expect("a target triple").into());
        above.parent().expect("a target directory above the triple")
    };
    args.push("--target-dir".into());
    args.push(target_dir.into());
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo).args(&args).status().unwrap();
    assert!(status.success(), "cargo {args:?} failed");
    assert!(
        exe.exists(),
        "cargo {args:?} did not leave {}",
        exe.display()
    );
    exe
}
