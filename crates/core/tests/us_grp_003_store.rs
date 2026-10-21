// US-GRP-003, ADR-GRP-006 § 4: the per-repo store keeps every session and
// attribution record when `sessions` is rebuilt to accept the cause
// `worktree-removed`. Temporary profile and repo only (NFR-01).

mod common;

use std::path::Path;

use common::*;
use gitraptor_core::profile::{Author, EndCause, Origin, RecordKind, WriteOp};
use rusqlite::Connection;

const OLD_SESSIONS: &str = "
CREATE TABLE sessions_old (
    session_id     TEXT PRIMARY KEY,
    worktree_id    INTEGER NOT NULL REFERENCES worktrees(id),
    agent_kind     TEXT NOT NULL CHECK (agent_kind IN ('claude-code', 'other')),
    agent_name     TEXT,
    initial_origin TEXT NOT NULL CHECK (initial_origin IN ('detected', 'registered')),
    detection_key  TEXT,
    started_ms     INTEGER NOT NULL,
    ended_ms       INTEGER,
    end_cause      TEXT CHECK (end_cause IN
                       ('process-gone', 'ended-during-gap', 'registration-withdrawn'))
) STRICT;
INSERT INTO sessions_old SELECT * FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_old RENAME TO sessions;
CREATE INDEX sessions_by_worktree ON sessions(worktree_id);
";

/// Puts the store back in the shape the version before this story left it:
/// when the current schema already accepts `worktree-removed`, `sessions` is
/// rebuilt without it and `user_version` goes one back. When it does not
/// accept it yet, the store is already that old version.
fn make_it_the_previous_version(store: &Path) {
    let conn = Connection::open(store).unwrap();
    let ddl: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'sessions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if !ddl.contains("worktree-removed") {
        return;
    }
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute_batch(&format!(
        "BEGIN; {OLD_SESSIONS} PRAGMA user_version = {}; COMMIT;",
        version - 1
    ))
    .unwrap();
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn the_migration_keeps_every_session_and_accepts_worktree_removed() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let path = profile.store_path(&entry.repo_id);

    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    let mut ops = vec![WriteOp::UpsertWorktree {
        path: repo.clone(),
        admin_name: None,
        seen_ms: 1,
    }];
    for (id, cause, ended) in [
        ("s1", EndCause::ProcessGone, Some(9)),
        ("s2", EndCause::EndedDuringGap, None),
        ("s3", EndCause::RegistrationWithdrawn, Some(11)),
    ] {
        ops.push(WriteOp::StartSession {
            session_id: id.into(),
            worktree: repo.clone(),
            agent: agent(),
            origin: Origin::Registered,
            detection_key: None,
            started_ms: 2,
        });
        ops.push(WriteOp::AppendAttribution {
            session_id: id.into(),
            kind: RecordKind::Register,
            agent: agent(),
            author: Author::Developer,
            recorded_ms: 3,
        });
        ops.push(WriteOp::EndSession {
            session_id: id.into(),
            ended_ms: ended,
            cause,
        });
    }
    store.write_batch(&ops).unwrap();
    drop(store);
    drop(profile);

    make_it_the_previous_version(&path);
    let before = Connection::open(&path).unwrap();
    let sessions = count(&before, "sessions");
    let records = count(&before, "attribution_records");
    assert_eq!((sessions, records), (3, 3));
    drop(before);

    // The next open applies whatever migration is pending.
    let profile = tp.open();
    let (store, _) = profile.open_store(&entry.repo_id).unwrap();
    drop(store);

    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "sessions"), sessions, "no session was lost");
    assert_eq!(count(&conn, "attribution_records"), records);
    let violation: Option<String> = conn
        .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
        .ok();
    assert_eq!(violation, None, "foreign_key_check must be empty");
    for (id, cause) in [
        ("s1", "process-gone"),
        ("s2", "ended-during-gap"),
        ("s3", "registration-withdrawn"),
    ] {
        let stored: String = conn
            .query_row(
                "SELECT end_cause FROM sessions WHERE session_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, cause);
    }
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    conn.execute(
        "INSERT INTO sessions (session_id, worktree_id, agent_kind, initial_origin,
             started_ms, ended_ms, end_cause)
         VALUES ('s4', (SELECT id FROM worktrees LIMIT 1), 'other', 'registered', 5, NULL,
             'worktree-removed')",
        [],
    )
    .expect("the migrated store accepts a session ended by worktree-removed");
    assert_eq!(count(&conn, "sessions"), sessions + 1);
}
