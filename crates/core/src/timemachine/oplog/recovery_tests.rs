//! The pre-migration copy is put back atomically, and also when only the copy is left.
//! Temporary profiles only (NFR-01).

use std::fs;

use rusqlite::Connection;

use super::{COPY_SUFFIX, OPLOG_FILE, Oplog, repo_dir};
use crate::profile::ProfileDirs;

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";

#[test]
fn an_oplog_missing_with_its_copy_present_is_restored_on_open() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    drop(oplog);
    let path = repo_dir(&dirs, REPO).unwrap().join(OPLOG_FILE);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE marker (x INTEGER);")
        .unwrap();
    // The state an interrupted restore leaves: the copy, and no oplog.
    let copy = path.with_file_name(format!("{OPLOG_FILE}{COPY_SUFFIX}"));
    fs::rename(&path, &copy).unwrap();

    let (oplog, _) = Oplog::open(&dirs, REPO, 2).unwrap();
    drop(oplog);

    assert!(!copy.exists(), "the copy was moved back");
    let seen: i64 = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'marker'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seen, 1, "the restored file is the copy");
}
