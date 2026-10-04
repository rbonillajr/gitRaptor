//! SEC-06: profile folders 0700 and files 0600 (including -wal/-shm and
//! quarantined files); an open or foreign folder stops the engine.
#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::*;
use gitraptor_core::profile::{Profile, ProfileError};

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn dirs_and_sqlite_files_are_private() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let (mut profile, _) = Profile::open(tp.dirs()).unwrap();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    store.write_batch(&sample_batch(&repo, "s1", 3)).unwrap();

    for dir in tp.dirs().owned_dirs() {
        assert_eq!(mode(dir), 0o700, "{}", dir.display());
    }
    let files = files_under(&tp.dirs().data);
    let names: Vec<String> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // While the connections are open, SQLite keeps -wal and -shm next to
    // both databases: they must be private too.
    for expected in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing {expected} in {names:?}"
        );
    }
    assert!(
        names
            .iter()
            .any(|n| n.ends_with(".sqlite-wal") && n != "index.sqlite-wal")
    );
    for file in &files {
        assert_eq!(mode(file), 0o600, "{}", file.display());
    }
    drop(store);
    drop(profile);
}

#[test]
fn preexisting_0755_dir_is_rejected_and_left_alone() {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    fs::create_dir_all(&dirs.data).unwrap();
    fs::set_permissions(
        dirs.owned_dirs()[0].as_path(),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::set_permissions(&dirs.data, fs::Permissions::from_mode(0o755)).unwrap();

    let err = Profile::open(dirs.clone())
        .err()
        .expect("must refuse to open");
    assert!(
        matches!(err, ProfileError::InsecureDir { ref path, .. } if *path == dirs.data),
        "{err}"
    );
    assert_eq!(
        mode(&dirs.data),
        0o755,
        "the folder is not fixed with chmod"
    );
    assert!(
        !dirs.data.join("index.sqlite").exists(),
        "nothing is written"
    );
}

#[test]
fn symlinked_profile_dir_is_rejected() {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let elsewhere = tp.root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(dirs.owned_dirs()[0].as_path()).unwrap();
    fs::set_permissions(
        dirs.owned_dirs()[0].as_path(),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::os::unix::fs::symlink(&elsewhere, &dirs.data).unwrap();

    assert!(matches!(
        Profile::open(dirs),
        Err(ProfileError::InsecureDir { .. })
    ));
}

#[test]
fn quarantined_files_are_private() {
    let tp = TempProfile::new();
    let (profile, _) = Profile::open(tp.dirs()).unwrap();
    drop(profile);
    let index = tp.dirs().data.join("index.sqlite");
    fs::write(&index, b"not a database at all, definitely corrupt").unwrap();
    fs::set_permissions(&index, fs::Permissions::from_mode(0o644)).unwrap();

    let (_, report) = Profile::open(tp.dirs()).unwrap();
    let quarantined = report.quarantined_index.expect("index quarantined");
    assert_eq!(mode(&quarantined), 0o600);
    assert_eq!(mode(&tp.dirs().quarantine_dir()), 0o700);
}
