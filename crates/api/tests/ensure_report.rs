//! DSK002-REPORT (TS-DSK-002 T001): the report of `raptor daemon ensure` is one schema shared by
//! the CLI and the desktop app (`gitraptor_api::client::ensure::EnsureReport`, D4), and the rule
//! of an installed `raptor` (`gitraptor_api::client::installed`, SEC-14) exists once.
//!
//! Temporary folders only (NFR-01). The accepted `raptor` lives under `CARGO_TARGET_TMPDIR`
//! because on macOS `tempfile::tempdir()` falls under `/var/folders`, inside `FIXED_TEMP`.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use gitraptor_api::client::ensure::EnsureReport;
use gitraptor_api::client::installed::is_installed_raptor;

const FIXTURES: &[&str] = &[
    "ready.json",
    "ready-started.json",
    "not-running.json",
    "daemon-older.json",
    "app-older.json",
    "missing-capabilities.json",
    "launch-failed.json",
];

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ensure")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()))
}

/// A regular file `name` with `mode` in `dir`, created with its parents.
fn executable(dir: &Path, name: &str, mode: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

/// A folder outside `FIXED_TEMP` (the target folder of this build).
fn outside_temp() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

#[test]
fn dsk002_report_every_fixture_round_trips_unchanged() {
    for name in FIXTURES {
        let text = fixture(name);
        let original: serde_json::Value = serde_json::from_str(&text).unwrap();
        let report: EnsureReport = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!("DSK002-REPORT: fixture {name} does not deserialize to EnsureReport: {e}")
        });
        let back = serde_json::to_value(&report).unwrap();
        assert_eq!(
            back, original,
            "DSK002-REPORT: fixture {name} does not serialize back identical (every key present)"
        );
    }
}

#[test]
fn dsk002_report_an_unknown_field_is_rejected() {
    let mut value: serde_json::Value = serde_json::from_str(&fixture("ready.json")).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unexpected".into(), serde_json::json!(1));
    let parsed = serde_json::from_value::<EnsureReport>(value);
    assert!(
        parsed.is_err(),
        "DSK002-REPORT: EnsureReport accepted an unknown field (deny_unknown_fields missing)"
    );
}

#[test]
fn dsk002_report_an_own_0755_raptor_outside_temp_is_installed() {
    let tmp = outside_temp();
    let raptor = executable(&tmp.path().join("bin"), "raptor", 0o755);
    assert!(
        is_installed_raptor(&raptor),
        "DSK002-REPORT: an own 0755 `raptor` outside FIXED_TEMP was not taken as installed: {}",
        raptor.display()
    );
}

#[test]
fn dsk002_report_a_file_with_another_name_is_not_installed() {
    let tmp = outside_temp();
    let other = executable(&tmp.path().join("bin"), "raptor-mcp", 0o755);
    assert!(
        !is_installed_raptor(&other),
        "DSK002-REPORT: a file not named `raptor` was taken as installed"
    );
}

#[test]
fn dsk002_report_a_group_writable_raptor_is_not_installed() {
    let tmp = outside_temp();
    let raptor = executable(&tmp.path().join("bin"), "raptor", 0o775);
    assert!(
        !is_installed_raptor(&raptor),
        "DSK002-REPORT: a 0775 `raptor` (writable by the group) was taken as installed"
    );
}

#[test]
fn dsk002_report_a_symlink_named_raptor_is_not_installed() {
    let tmp = outside_temp();
    let target = executable(&tmp.path().join("real"), "raptor", 0o755);
    let link_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&link_dir).unwrap();
    let link = link_dir.join("raptor");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(
        is_installed_raptor(&target),
        "DSK002-REPORT: precondition: the symlink target is an installed `raptor`"
    );
    assert!(
        !is_installed_raptor(&link),
        "DSK002-REPORT: a symbolic link named `raptor` was taken as installed"
    );
}

#[test]
fn dsk002_report_a_raptor_under_node_modules_is_not_installed() {
    let tmp = outside_temp();
    let raptor = executable(
        &tmp.path().join("project/node_modules/.bin"),
        "raptor",
        0o755,
    );
    assert!(
        !is_installed_raptor(&raptor),
        "DSK002-REPORT: a real 0755 `raptor` under node_modules was taken as installed"
    );
}

#[test]
fn dsk002_report_a_raptor_under_npx_is_not_installed() {
    let tmp = outside_temp();
    let raptor = executable(&tmp.path().join("cache/_npx/1a2b3c/bin"), "raptor", 0o755);
    assert!(
        !is_installed_raptor(&raptor),
        "DSK002-REPORT: a real 0755 `raptor` under _npx was taken as installed"
    );
}

#[test]
fn dsk002_report_a_raptor_under_tmp_is_not_installed() {
    let tmp = tempfile::tempdir_in("/private/tmp")
        .or_else(|_| tempfile::tempdir_in("/tmp"))
        .unwrap();
    let raptor = executable(&tmp.path().join("bin"), "raptor", 0o755);
    assert!(
        !is_installed_raptor(&raptor),
        "DSK002-REPORT: a real 0755 `raptor` under /private/tmp was taken as installed"
    );
}
