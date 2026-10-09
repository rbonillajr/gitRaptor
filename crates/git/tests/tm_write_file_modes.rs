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
fn restored_files_keep_their_modes_whatever_the_umask() {
    // 077 would narrow a 0644 to 0600; 000 would leave 0666 / 0777 if nothing set the mode.
    for umask in [0o077, 0o000] {
        let tmp = tempfile::tempdir().unwrap();
        let root = RootDir::open(tmp.path()).unwrap();
        let previous = rustix::process::umask(rustix::fs::Mode::from_raw_mode(umask));

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
        assert_eq!(
            mode_of(&tmp.path().join("plain.txt")),
            0o644,
            "umask {umask:o}"
        );
        assert_eq!(
            mode_of(&tmp.path().join("sub/run.sh")),
            0o755,
            "umask {umask:o}"
        );
    }
}
