//! The reserved-command audit admits the end of a confirmation of the team baseline
//! (`guard.confirm`, DS-US-GRD-014 D10): a profile from the schema right before it migrates with
//! every `guard.uninstall` row and its id, `guard.confirm` rows can be written, `changed` stays
//! `guard.confirm`'s own and the audit stays append-only (NFR-01, SEC-03). Temporary profile.

mod common;

use common::*;
use gitraptor_core::profile::{AuditRow, INDEX_FILE};

fn row(at_ms: i64, operation: &str, outcome: &str) -> AuditRow {
    AuditRow {
        at_ms,
        operation: operation.into(),
        repo_id: Some("r1".into()),
        outcome: outcome.into(),
        reason: Some("risk-accepted; action a1".into()),
        client: r#"{"pid":7,"start_us":11}"#.into(),
        chain: format!(r#"{{"pid":7,"start_us":{at_ms}}}"#),
    }
}

/// The audit as the migration of the uninstall outcomes left it (index `user_version` 5).
const PREVIOUS_AUDIT: &str = r"
CREATE TABLE reserved_audit (
    id        INTEGER PRIMARY KEY,
    at_ms     INTEGER NOT NULL,
    operation TEXT NOT NULL,
    repo_id   TEXT,
    outcome   TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected', 'not-implemented',
                  'applied', 'cancelled', 'failed', 'expired')),
    reason    TEXT,
    client    TEXT NOT NULL,
    chain     TEXT NOT NULL,
    CHECK (outcome IN ('accepted', 'rejected', 'not-implemented')
           OR operation = 'guard.uninstall')
) STRICT;
CREATE TRIGGER reserved_audit_no_update BEFORE UPDATE ON reserved_audit
    BEGIN SELECT RAISE(ABORT, 'the audit is append-only'); END;
CREATE TRIGGER reserved_audit_no_delete BEFORE DELETE ON reserved_audit
    BEGIN SELECT RAISE(ABORT, 'the audit is append-only'); END;
";

#[test]
fn a_profile_from_before_the_confirmation_outcomes_migrates_every_row() {
    let tp = TempProfile::new();
    drop(tp.open());
    let rows = vec![
        row(10, "guard.uninstall", "accepted"),
        row(20, "guard.uninstall", "rejected"),
        row(30, "guard.uninstall", "applied"),
    ];
    {
        let conn = rusqlite::Connection::open(tp.dirs().data.join(INDEX_FILE)).unwrap();
        conn.execute_batch(&format!(
            "DROP TABLE reserved_audit; {PREVIOUS_AUDIT} PRAGMA user_version = 5;"
        ))
        .unwrap();
        for (id, r) in [2_i64, 4, 7].iter().zip(&rows) {
            conn.execute(
                "INSERT INTO reserved_audit
                     (id, at_ms, operation, repo_id, outcome, reason, client, chain)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    id,
                    r.at_ms,
                    r.operation,
                    r.repo_id,
                    r.outcome,
                    r.reason,
                    r.client,
                    r.chain
                ],
            )
            .unwrap();
        }
        // Before the migration a confirmation's end is refused.
        assert!(
            conn.execute(
                "INSERT INTO reserved_audit (at_ms, operation, outcome, client, chain)
                 VALUES (1, 'guard.confirm', 'applied', '{}', '{}')",
                [],
            )
            .is_err()
        );
    }

    let mut profile = tp.open();
    let migrated = profile.audit(0, 100).unwrap();
    let expected: Vec<(i64, AuditRow)> = [2, 4, 7].into_iter().zip(rows).collect();
    assert_eq!(migrated, expected);
    for (at, outcome) in ["applied", "changed", "cancelled", "expired", "failed"]
        .iter()
        .enumerate()
    {
        profile
            .append_audit(&row(40 + at as i64, "guard.confirm", outcome))
            .unwrap_or_else(|e| panic!("guard.confirm {outcome}: {e}"));
    }
    // `changed` is the confirmation's own; the uninstall keeps its outcomes.
    assert!(
        profile
            .append_audit(&row(50, "guard.uninstall", "changed"))
            .is_err()
    );
    assert!(
        profile
            .append_audit(&row(51, "guard.uninstall", "expired"))
            .is_ok()
    );
    assert!(
        profile
            .append_audit(&row(52, "daemon.stop", "changed"))
            .is_err()
    );
    drop(profile);

    let conn = rusqlite::Connection::open(tp.dirs().data.join(INDEX_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(version > 5, "not migrated: user_version {version}");
    for statement in [
        "UPDATE reserved_audit SET outcome = 'accepted' WHERE id = 2",
        "DELETE FROM reserved_audit WHERE id = 2",
    ] {
        let err = conn.execute(statement, []).unwrap_err();
        assert!(
            err.to_string().contains("append-only"),
            "{statement}: {err}"
        );
    }
    drop(conn);
    assert_eq!(tp.open().audit(0, 100).unwrap().len(), 9);
}
