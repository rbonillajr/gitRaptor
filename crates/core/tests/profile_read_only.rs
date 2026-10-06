//! US-GRP-017, escenario 4: with the engine stopped, the CLI reads the
//! repos of the index read-only and writes nothing to the profile.

mod common;

use common::*;
use gitraptor_core::profile::{INDEX_FILE, read_only_repos};

#[test]
fn the_index_is_read_without_writing_the_profile() {
    let tp = TempProfile::new();
    let repo = init_repo(tp.root.path(), "demo", true);
    let id = {
        let mut profile = tp.open();
        profile
            .add_repo(&common_dir(&repo), None, 1)
            .unwrap()
            .0
            .repo_id
    };
    let mut before = files_under(tp.root.path());
    before.sort();

    let repos = read_only_repos(&tp.dirs().data.join(INDEX_FILE)).unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].0, id);
    assert_eq!(repos[0].1, common_dir(&repo));

    let mut after = files_under(tp.root.path());
    after.sort();
    assert_eq!(before, after, "no index file appeared or went away");
}

#[test]
fn a_missing_or_foreign_index_gives_nothing() {
    let tp = TempProfile::new();
    let index = tp.dirs().data.join(INDEX_FILE);
    assert_eq!(read_only_repos(&index), None);
    assert!(!index.exists(), "the index is never created");
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    std::fs::write(&index, b"not a database").unwrap();
    assert_eq!(read_only_repos(&index), None);
}

/// With a `-wal` file the index may hold commits only there: no answer
/// rather than a stale one, and still nothing written.
#[test]
fn an_index_with_a_wal_is_not_read() {
    let tp = TempProfile::new();
    // `?` is not a valid file name character on Windows.
    let odd = if cfg!(windows) {
        "demo  #&%"
    } else {
        "demo  #?%"
    };
    let repo = init_repo(tp.root.path(), odd, true);
    let mut profile = tp.open();
    profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let index = tp.dirs().data.join(INDEX_FILE);
    let wal = tp.dirs().data.join(format!("{INDEX_FILE}-wal"));
    assert!(wal.exists(), "the engine keeps its WAL while open");
    assert_eq!(read_only_repos(&index), None);
    drop(profile);
    assert!(!wal.exists());
    let repos = read_only_repos(&index).unwrap();
    assert_eq!(repos[0].1, common_dir(&repo), "an odd path is still read");
}
