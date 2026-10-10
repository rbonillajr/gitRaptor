//! The temporaries a killed `replace_files` leaves in the guardrails folder: only
//! `<listed file>.gitraptor.tmp-<16 hex>`, regular files, never through a link, and only in the
//! folder the journal recorded. Temporary folders only, never a real repo (NFR-01).

mod common;

use std::path::Path;

use gitraptor_git::guard_write::{FOLDER, FileId, GuardWriteError, GuardWriter, NewFile};

const REAL_TEMP: &str = "hooks/pre-push.gitraptor.tmp-0123456789abcdef";

fn files() -> Vec<NewFile<'static>> {
    vec![
        NewFile {
            path: "hooks/pre-push",
            bytes: b"stub",
            executable: true,
        },
        NewFile {
            path: "dispatch.conf",
            bytes: b"template\t2\n",
            executable: false,
        },
    ]
}

fn plant(folder: &Path, rela: &str) {
    std::fs::write(folder.join(rela), b"leftover").unwrap();
}

#[test]
fn remove_file_temporaries_removes_only_temporaries_of_listed_files() {
    let f = common::Fixture::new();
    let invoker = f.invoker();
    let writer = GuardWriter::new(&f.git, &invoker);
    let git_dir = f.repo.join(".git");
    let expected = writer.write_folder(&git_dir, &files()).unwrap();
    let folder = git_dir.join(FOLDER);
    let listed = ["hooks/pre-push", "dispatch.conf"];

    // What must go, what must stay.
    plant(&folder, REAL_TEMP);
    plant(&folder, "dispatch.conf.gitraptor.tmp-fedcba9876543210");
    let stay = [
        "hooks/pre-push.gitraptor.tmp-xyz",
        "hooks/pre-push.gitraptor.tmp-0123456789abcde",
        "hooks/pre-push.gitraptor.tmp-0123456789abcdeff",
        "hooks/otro.gitraptor.tmp-0123456789abcdef",
        "hooks/pre-push.other-0123456789abcdef",
    ];
    for name in stay {
        plant(&folder, name);
    }
    #[cfg(unix)]
    let outside = {
        // A link with the temporary's name must neither be followed nor removed.
        let outside = f.tmp.path().join("outside.txt");
        std::fs::write(&outside, b"keep").unwrap();
        std::os::unix::fs::symlink(
            &outside,
            folder.join("hooks/pre-push.gitraptor.tmp-aaaaaaaaaaaaaaaa"),
        )
        .unwrap();
        outside
    };

    // A folder that is not the one the journal recorded: refused, nothing removed.
    let other = FileId {
        dev: expected.dev,
        ino: expected.ino.wrapping_add(1),
    };
    let refused = writer.remove_file_temporaries(&git_dir, other, &listed);
    assert!(
        matches!(refused, Err(GuardWriteError::Changed(_))),
        "{refused:?}"
    );
    assert!(
        folder.join(REAL_TEMP).exists(),
        "refused call removed a file"
    );

    writer
        .remove_file_temporaries(&git_dir, expected, &listed)
        .unwrap();
    assert!(!folder.join(REAL_TEMP).exists(), "the temporary stays");
    assert!(
        !folder
            .join("dispatch.conf.gitraptor.tmp-fedcba9876543210")
            .exists(),
        "the temporary of dispatch.conf stays"
    );
    for name in stay {
        assert!(folder.join(name).exists(), "{name} was removed");
    }
    assert!(folder.join("hooks/pre-push").exists());
    assert!(folder.join("dispatch.conf").exists());
    #[cfg(unix)]
    {
        let link = folder.join("hooks/pre-push.gitraptor.tmp-aaaaaaaaaaaaaaaa");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link was removed"
        );
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
    }

    // Without the folder there is nothing to do.
    let none = f.tmp.path().join("no-such-common");
    std::fs::create_dir_all(&none).unwrap();
    writer
        .remove_file_temporaries(&none, expected, &listed)
        .unwrap();
}
