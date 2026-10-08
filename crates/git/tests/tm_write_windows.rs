//! Windows working-tree writes of the applier never follow a junction (DS-TS-TMC-003, Enmienda
//! 2026-10-08, W2–W3). An integration test because it plants the junction with `mklink`, and the
//! crate's sources may launch no process outside `invoke.rs` (`static_check.rs`).
#![cfg(windows)]

use gitraptor_git::tm_write::files::{Content, Expected, Kind, Outcome, RootDir, blob_id};

fn content(bytes: &[u8]) -> Content<'_> {
    Content::File {
        bytes,
        executable: false,
    }
}

fn present(bytes: &[u8]) -> Expected {
    Expected::Present {
        kind: Kind::File,
        id: blob_id(bytes),
    }
}

#[test]
fn links_and_junctions_are_never_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("a"), b"outside").unwrap();
    // A junction needs no privilege, unlike a symbolic link.
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(tmp.path().join("j"))
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(status.status.success(), "{status:?}");
    let root = RootDir::open(tmp.path()).unwrap();
    // On the way: blocked, nothing written outside.
    let out = root
        .replace(b"j/a", &content(b"new"), &present(b"outside"), &|_| {})
        .unwrap();
    assert!(matches!(out, Outcome::Blocked(_)), "{out:?}");
    assert!(matches!(
        root.remove(b"j/a", Kind::File, blob_id(b"outside"))
            .unwrap(),
        Outcome::Blocked(_)
    ));
    // At the path itself: someone else's entry, kept as it is.
    let out = root
        .replace(b"j", &content(b"new"), &present(b"x"), &|_| {})
        .unwrap();
    assert_eq!(out, Outcome::Overlap { kept_at: None });
    assert!(!root.remove_dir_if_empty(b"j").unwrap());
    assert_eq!(root.current(b"j").unwrap(), None);
    assert_eq!(std::fs::read(outside.path().join("a")).unwrap(), b"outside");
    assert!(tmp.path().join("j").symlink_metadata().is_ok());
}
