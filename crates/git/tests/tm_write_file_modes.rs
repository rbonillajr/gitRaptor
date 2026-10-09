//! Files written by the applier get the mode the snapshot records, whatever the umask of the
//! process (NFR-01: exact state after an undo). Its own test binary: the umask is process-wide.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use gitraptor_git::tm_write::files::{Content, Expected, Outcome, RootDir};

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

#[test]
fn restored_files_keep_their_modes_under_a_restrictive_umask() {
    let tmp = tempfile::tempdir().unwrap();
    let root = RootDir::open(tmp.path()).unwrap();
    let previous = rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o077));

    let plain = Content::File {
        bytes: b"plain\n",
        executable: false,
    };
    let script = Content::File {
        bytes: b"#!/bin/sh\n",
        executable: true,
    };
    let a = root.replace(b"plain.txt", &plain, &Expected::Absent, &|_| {});
    let b = root.replace(b"sub/run.sh", &script, &Expected::Absent, &|_| {});
    rustix::process::umask(previous);

    assert_eq!(a.unwrap(), Outcome::Written);
    assert_eq!(b.unwrap(), Outcome::Written);
    assert_eq!(mode_of(&tmp.path().join("plain.txt")), 0o644);
    assert_eq!(mode_of(&tmp.path().join("sub/run.sh")), 0o755);
}
