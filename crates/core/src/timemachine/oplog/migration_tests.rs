//! Migration 3 of the oplog (the manual snapshot columns) on an oplog written by the previous
//! version: append-only triggers back, a failed migration leaves the file as it was, and running
//! it twice changes nothing. Dedicated file wired from `oplog/mod.rs` by one line: the
//! criteria's tests must not live in a production file.
//!
//! Every test works in a temporary profile, never in a real repo or profile (NFR-01). The
//! "previous version" oplog is built here from the first two migrations and rows chained with
//! the encoding of format 2, so it does not depend on what the current code writes.

use std::fs;
use std::path::PathBuf;

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use super::chain::{self, Tip};
use super::schema::OPLOG_MIGRATIONS;
use super::{OPLOG_FILE, Oplog, OplogStatus, repo_dir};
use crate::profile::{ProfileDirs, fsperm};

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";
/// Version of the oplog this change introduces.
const MIGRATION: i64 = 3;
/// Format of the hashed rows of the previous version.
const OLD_FORMAT: i64 = 2;

fn profile() -> (tempfile::TempDir, ProfileDirs) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    (tmp, dirs)
}

fn raw(dirs: &ProfileDirs) -> Connection {
    Connection::open(db_path(dirs)).unwrap()
}

fn db_path(dirs: &ProfileDirs) -> PathBuf {
    repo_dir(dirs, REPO).unwrap().join(OPLOG_FILE)
}

fn version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// The hash of a snapshot row as format 2 encodes it (the fixed `SELECT` of the previous
/// version): previous hash, format, kind, sequence, batch and each column, tagged by type.
fn snapshot_hash(prev: &[u8], seq: i64, batch: i64, id: &str, worktree: &str, at: i64) -> [u8; 32] {
    fn bytes(h: &mut Sha256, b: &[u8]) {
        h.update((b.len() as u64).to_be_bytes());
        h.update(b);
    }
    fn text(h: &mut Sha256, t: &str) {
        h.update([3]);
        bytes(h, t.as_bytes());
    }
    let mut h = Sha256::new();
    h.update(prev);
    h.update(OLD_FORMAT.to_be_bytes());
    bytes(&mut h, b"snapshot");
    h.update(seq.to_be_bytes());
    h.update(batch.to_be_bytes());
    h.update(9u64.to_be_bytes());
    text(&mut h, id); // snapshot_id
    h.update([1]);
    h.update(seq.to_be_bytes()); // seq
    text(&mut h, "guaranteed-prior"); // level
    text(&mut h, &format!("[\"{worktree}\"]")); // worktrees
    text(&mut h, &format!("refs/tm/snap/{id}")); // store_ref
    h.update([1]);
    h.update(1i64.to_be_bytes()); // engine_mark
    h.update([0]); // cause_operation
    h.update([0]); // cause_event_seq
    h.update([1]);
    h.update(at.to_be_bytes()); // recorded_ms
    h.finalize().into()
}

/// An oplog as the previous version left it: schema version 2, `rows` chained snapshot rows
/// and the head file. Returns the ids of the rows.
fn previous_version_oplog(dirs: &ProfileDirs, rows: i64) -> Vec<String> {
    let dir = repo_dir(dirs, REPO).unwrap();
    fsperm::ensure_private_dir(&dirs.data.join(super::TM_DIR)).unwrap();
    fsperm::ensure_private_dir(&dir).unwrap();
    let conn = Connection::open(dir.join(OPLOG_FILE)).unwrap();
    for migration in &OPLOG_MIGRATIONS[..2] {
        conn.execute_batch(migration).unwrap();
    }
    conn.pragma_update(None, "user_version", 2).unwrap();
    let mut prev = chain::genesis(REPO).to_vec();
    let mut ids = Vec::new();
    let mut tip = None;
    for seq in 1..=rows {
        let id = format!("00000000-0000-4000-8000-{seq:012}");
        let at = 1_000 + seq;
        conn.execute(
            "INSERT INTO snapshots (snapshot_id, seq, level, worktrees, store_ref, engine_mark,
                 cause_operation, cause_event_seq, recorded_ms)
             VALUES (?1, ?2, 'guaranteed-prior', ?3, ?4, 1, NULL, NULL, ?5)",
            params![
                id,
                seq,
                "[\"/repo/main\"]",
                format!("refs/tm/snap/{id}"),
                at
            ],
        )
        .unwrap();
        let hash = snapshot_hash(&prev, seq, seq, &id, "/repo/main", at);
        conn.execute(
            "INSERT INTO chain (seq, kind, format, batch, prev_hash, hash)
             VALUES (?1, 'snapshot', ?2, ?3, ?4, ?5)",
            params![seq, OLD_FORMAT, seq, &prev[..], &hash[..]],
        )
        .unwrap();
        prev = hash.to_vec();
        tip = Some(Tip {
            seq,
            batch: seq,
            hash,
        });
        ids.push(id);
    }
    drop(conn);
    if let Some(tip) = tip {
        chain::write_head(&dir.join(super::HEAD_FILE), &tip).unwrap();
    }
    ids
}

fn snapshot_ids(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT snapshot_id FROM snapshots ORDER BY seq")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn triggers_of_snapshots(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'snapshots' ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn count(conn: &Connection, table: &str) -> i64 {
    let sql = ["SELECT COUNT(*) FROM ", table].concat();
    conn.query_row(&sql, [], |r| r.get(0)).unwrap()
}

/// Sanity of the fixture itself: what the previous version wrote verifies.
#[test]
fn the_previous_version_oplog_of_these_tests_verifies() {
    let (_tmp, dirs) = profile();
    previous_version_oplog(&dirs, 3);
    let conn = raw(&dirs);
    assert_eq!(version(&conn), 2);
    assert_eq!(chain::verify(&conn, REPO).unwrap(), vec![]);
}

#[test]
fn the_append_only_triggers_exist_after_migration_3() {
    let (_tmp, dirs) = profile();
    let ids = previous_version_oplog(&dirs, 2);

    let (log, opened) = Oplog::open(&dirs, REPO, 5_000).unwrap();
    assert_eq!(opened.status, OplogStatus::Existing);
    assert_eq!(opened.new_breaks, vec![], "migrating is not a break");
    assert_eq!(log.verify_chain().unwrap(), vec![]);
    drop(log);

    let conn = raw(&dirs);
    assert_eq!(
        version(&conn),
        MIGRATION,
        "the oplog went through migration 3"
    );
    let has_label = conn.prepare("SELECT label FROM snapshots LIMIT 0").is_ok();
    assert!(has_label, "the snapshots table has the manual columns");
    assert_eq!(snapshot_ids(&conn), ids, "every row survived");
    assert_eq!(
        triggers_of_snapshots(&conn),
        ["snapshots_no_delete", "snapshots_no_update"],
        "the rebuilt table got its append-only triggers back"
    );
    let update = conn.execute("UPDATE snapshots SET store_ref = 'x'", []);
    assert!(
        update.unwrap_err().to_string().contains("append-only"),
        "UPDATE on snapshots must fail"
    );
    let delete = conn.execute("DELETE FROM snapshots", []);
    assert!(
        delete.unwrap_err().to_string().contains("append-only"),
        "DELETE on snapshots must fail"
    );
    assert_eq!(snapshot_ids(&conn), ids, "nothing was touched");
}

#[test]
fn a_failed_migration_3_restores_the_oplog_and_the_chain_verifies() {
    let (_tmp, dirs) = profile();
    let ids = previous_version_oplog(&dirs, 3);
    // A table in the way of the rebuilt one: the migration fails after it has begun.
    raw(&dirs)
        .execute_batch("CREATE TABLE snapshots_v3 (in_the_way INTEGER);")
        .unwrap();
    let before = fs::read(db_path(&dirs)).unwrap().len();

    let failed = Oplog::open(&dirs, REPO, 5_000);
    assert!(
        failed.is_err(),
        "migration 3 must fail on the sabotaged oplog"
    );

    // The oplog is as the previous version left it: same version, same rows, same triggers,
    // and the chain verifies.
    let conn = raw(&dirs);
    assert_eq!(version(&conn), 2, "the version did not move");
    assert_eq!(snapshot_ids(&conn), ids);
    assert_eq!(
        triggers_of_snapshots(&conn),
        ["snapshots_no_delete", "snapshots_no_update"]
    );
    assert!(
        conn.prepare("SELECT label FROM snapshots LIMIT 0").is_err(),
        "no manual column was left behind"
    );
    assert_eq!(chain::verify(&conn, REPO).unwrap(), vec![]);
    assert_eq!(count(&conn, "chain"), 3);
    // Nothing grew: no declared break, no journal row.
    assert_eq!(count(&conn, "journal"), 0);
    assert!(fs::read(db_path(&dirs)).unwrap().len() >= before);
}

#[test]
fn running_migration_3_twice_changes_nothing_the_second_time() {
    let (_tmp, dirs) = profile();
    let ids = previous_version_oplog(&dirs, 3);

    let (first, opened) = Oplog::open(&dirs, REPO, 5_000).unwrap();
    assert_eq!(opened.new_breaks, vec![]);
    let tip = first.last_seq().unwrap();
    drop(first);
    let conn = raw(&dirs);
    assert_eq!(version(&conn), MIGRATION, "the first open migrated");
    let rows_after_first = (
        count(&conn, "snapshots"),
        count(&conn, "chain"),
        count(&conn, "journal"),
    );
    drop(conn);

    let (second, opened) = Oplog::open(&dirs, REPO, 6_000).unwrap();
    assert_eq!(opened.status, OplogStatus::Existing);
    assert_eq!(
        opened.new_breaks,
        vec![],
        "the second open found nothing to repair"
    );
    assert_eq!(second.verify_chain().unwrap(), vec![]);
    assert_eq!(
        second.last_seq().unwrap(),
        tip,
        "the second open appended nothing"
    );
    drop(second);

    let conn = raw(&dirs);
    assert_eq!(version(&conn), MIGRATION, "still version 3, not 4");
    assert_eq!(snapshot_ids(&conn), ids);
    assert_eq!(
        (
            count(&conn, "snapshots"),
            count(&conn, "chain"),
            count(&conn, "journal")
        ),
        rows_after_first
    );
    assert_eq!(chain::verify(&conn, REPO).unwrap(), vec![]);
}
