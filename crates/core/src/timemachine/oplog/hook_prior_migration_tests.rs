//! The oplog migration that lets a `hook-prior` row carry its requester (Brief US-TMC-005, D6).
//!
//! The migration is found by its marker, never by its index: another branch may add a migration
//! too, and whoever merges second renumbers (appends after the other). These tests hold for any
//! position: the whole sequence applies on a fresh database, and the migration applies on a
//! database left by the migrations before it, keeping every row as it was.

use rusqlite::{Connection, params};

use super::schema::OPLOG_MIGRATIONS;

/// The first line of the migration's SQL.
const MARKER: &str = "-- migration: hook-prior requester";

fn migration_index() -> usize {
    OPLOG_MIGRATIONS
        .iter()
        .position(|m| m.contains(MARKER))
        .expect("no hook-prior migration: its SQL must contain the marker")
}

fn apply(conn: &Connection, migrations: &[&str]) {
    for m in migrations {
        conn.execute_batch(m).unwrap();
    }
}

/// One `snapshots` row; `None` columns are `NULL`.
#[allow(clippy::too_many_arguments)]
fn insert(
    conn: &Connection,
    seq: i64,
    level: &str,
    label: Option<&str>,
    requester: Option<&str>,
    session: Option<&str>,
    key: Option<&str>,
    channel: Option<&str>,
) -> rusqlite::Result<usize> {
    let id = format!("00000000-0000-4000-8000-{seq:012}");
    conn.execute(
        "INSERT INTO snapshots (snapshot_id, seq, level, worktrees, store_ref, engine_mark,
             cause_operation, cause_event_seq, recorded_ms,
             label, requester, requester_session, worktree_key, channel)
         VALUES (?1, ?2, ?3, '[\"main\"]', ?4, 1, NULL, NULL, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            seq,
            level,
            format!("refs/tm/snapshots/{id}"),
            1_000 + seq,
            label,
            requester,
            session,
            key,
            channel
        ],
    )
}

const AGENT: &str = r#"{"agent":{"name":"claude-code","origin":"detected","session_id":"s1"}}"#;
const UNATTRIBUTED: &str = r#""unattributed""#;

#[test]
fn hook_prior_migration_fresh_database_accepts_hook_prior_rows_and_keeps_the_other_rules() {
    let conn = Connection::open_in_memory().unwrap();
    apply(&conn, OPLOG_MIGRATIONS);
    assert!(OPLOG_MIGRATIONS.iter().any(|m| m.contains(MARKER)));

    // A hook prior of an agent and of an unattributed requester (shared quota bucket).
    let ok = |r: rusqlite::Result<usize>, what: &str| assert!(r.is_ok(), "{what}: {r:?}");
    let refused = |r: rusqlite::Result<usize>, what: &str| assert!(r.is_err(), "{what}");
    ok(
        insert(
            &conn,
            1,
            "hook-prior",
            None,
            Some(AGENT),
            Some("s1"),
            Some("k"),
            Some("hook"),
        ),
        "an agent's hook prior",
    );
    ok(
        insert(
            &conn,
            2,
            "hook-prior",
            None,
            Some(UNATTRIBUTED),
            None,
            Some("k"),
            Some("hook"),
        ),
        "an unattributed hook prior",
    );
    // A hook prior always says who asked, for which worktree, through the hook channel.
    refused(
        insert(
            &conn,
            3,
            "hook-prior",
            None,
            None,
            None,
            Some("k"),
            Some("hook"),
        ),
        "a hook prior without requester",
    );
    refused(
        insert(
            &conn,
            4,
            "hook-prior",
            None,
            Some(AGENT),
            Some("s1"),
            None,
            Some("hook"),
        ),
        "a hook prior without worktree key",
    );
    refused(
        insert(
            &conn,
            5,
            "hook-prior",
            None,
            Some(AGENT),
            Some("s1"),
            Some("k"),
            Some("mcp"),
        ),
        "a hook prior through another channel",
    );
    refused(
        insert(
            &conn,
            6,
            "hook-prior",
            Some("l"),
            Some(AGENT),
            Some("s1"),
            Some("k"),
            Some("hook"),
        ),
        "a hook prior with a label",
    );
    // The manual rules hold as before.
    ok(
        insert(
            &conn,
            7,
            "manual",
            Some("l"),
            Some(AGENT),
            Some("s1"),
            Some("k"),
            Some("mcp"),
        ),
        "a manual snapshot",
    );
    refused(
        insert(
            &conn,
            8,
            "manual",
            Some("l"),
            Some(UNATTRIBUTED),
            None,
            Some("k"),
            Some("mcp"),
        ),
        "a manual snapshot without session",
    );
    // Other levels carry none of it.
    ok(
        insert(&conn, 9, "observation", None, None, None, None, None),
        "an observation",
    );
    refused(
        insert(
            &conn,
            10,
            "guaranteed-prior",
            None,
            Some(AGENT),
            Some("s1"),
            Some("k"),
            Some("cli"),
        ),
        "a guaranteed prior with a requester",
    );
    // The quota of hook priors reads through its own indexes.
    let indexes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND tbl_name = 'snapshots' AND sql LIKE '%hook-prior%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(indexes >= 2, "hook-prior indexes: {indexes}");
    // Still append-only.
    assert!(conn.execute("UPDATE snapshots SET seq = seq", []).is_err());
    assert!(conn.execute("DELETE FROM snapshots", []).is_err());
}

#[test]
fn hook_prior_migration_applies_cleanly_from_the_previous_version() {
    let at = migration_index();
    let conn = Connection::open_in_memory().unwrap();
    apply(&conn, &OPLOG_MIGRATIONS[..at]);
    insert(&conn, 1, "guaranteed-prior", None, None, None, None, None).unwrap();
    insert(&conn, 2, "observation", None, None, None, None, None).unwrap();
    insert(
        &conn,
        3,
        "manual",
        Some("l"),
        Some(AGENT),
        Some("s1"),
        Some("k"),
        Some("mcp"),
    )
    .unwrap();
    let dump = |conn: &Connection| -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT snapshot_id, seq, level, worktrees, store_ref, engine_mark,
                     cause_operation, cause_event_seq, recorded_ms, label, requester,
                     requester_session, worktree_key, channel
                 FROM snapshots ORDER BY seq",
            )
            .unwrap();
        stmt.query_map([], |r| {
            let cols: Vec<String> = (0..14)
                .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                .collect();
            Ok(cols.join("|"))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    };
    let before = dump(&conn);

    apply(&conn, &OPLOG_MIGRATIONS[at..]);
    // The columns are the same and in the same order, so the chain's hashes still verify.
    assert_eq!(dump(&conn), before);
    insert(
        &conn,
        4,
        "hook-prior",
        None,
        Some(AGENT),
        Some("s1"),
        Some("k"),
        Some("hook"),
    )
    .unwrap();
}
