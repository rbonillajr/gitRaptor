//! The reserved-command audit keeps the outcomes of an announced uninstall (US-GRD-003 E6,
//! ADR-GRD-007 § 2): applied, cancelled, failed and expired rows are written, not lost, and a
//! profile from before them migrates with every row and both append-only triggers intact
//! (NFR-01, SEC-03).

mod common;

use common::*;
use gitraptor_core::profile::{AuditRow, INDEX_FILE};

fn row(at_ms: i64, outcome: &str, reason: Option<&str>, repo_id: Option<&str>) -> AuditRow {
    AuditRow {
        at_ms,
        operation: "guard.uninstall".into(),
        repo_id: repo_id.map(str::to_owned),
        outcome: outcome.into(),
        reason: reason.map(str::to_owned),
        client: r#"{"pid":7,"start_us":11}"#.into(),
        chain: format!(r#"{{"pid":7,"start_us":{at_ms}}}"#),
    }
}

/// The schema of the audit before the uninstall outcomes (index `user_version` 4), as migration
/// 2 created it.
const PREVIOUS_AUDIT: &str = r"
CREATE TABLE reserved_audit (
    id        INTEGER PRIMARY KEY,
    at_ms     INTEGER NOT NULL,
    operation TEXT NOT NULL,
    repo_id   TEXT,
    outcome   TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected', 'not-implemented')),
    reason    TEXT,
    client    TEXT NOT NULL,
    chain     TEXT NOT NULL
) STRICT;
CREATE TRIGGER reserved_audit_no_update BEFORE UPDATE ON reserved_audit
    BEGIN SELECT RAISE(ABORT, 'the audit is append-only'); END;
CREATE TRIGGER reserved_audit_no_delete BEFORE DELETE ON reserved_audit
    BEGIN SELECT RAISE(ABORT, 'the audit is append-only'); END;
";

#[test]
fn every_uninstall_outcome_is_audited() {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let outcomes = ["applied", "cancelled", "failed", "expired"];
    for (at, outcome) in outcomes.iter().enumerate() {
        profile
            .append_audit(&row(at as i64, outcome, Some("risk-accepted"), Some("r1")))
            .unwrap_or_else(|e| panic!("the {outcome} row was lost: {e}"));
    }
    drop(profile);
    let rows = tp.open().audit(0, 10).unwrap();
    let kept: Vec<&str> = rows.iter().map(|(_, r)| r.outcome.as_str()).collect();
    assert_eq!(kept, outcomes);
}

#[test]
fn an_unknown_outcome_is_still_refused() {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    assert!(
        profile
            .append_audit(&row(1, "approved", None, None))
            .is_err()
    );
    // The outcomes of an announced uninstall belong to `guard.uninstall` only.
    let mut stop = row(2, "applied", None, None);
    stop.operation = "daemon.stop".into();
    assert!(profile.append_audit(&stop).is_err());
    assert!(profile.audit(0, 10).unwrap().is_empty());
}

#[test]
fn a_profile_from_before_the_outcomes_migrates_every_row() {
    let tp = TempProfile::new();
    drop(tp.open());
    let rows = vec![
        row(10, "accepted", None, Some("r1")),
        row(20, "rejected", Some("agent-ancestry"), None),
        row(30, "not-implemented", None, Some("r2")),
        row(40, "accepted", Some("a reason, with 'quotes'"), None),
    ];
    // Back to the previous schema, with rows and a gap in the ids (ids are kept, not renumbered).
    {
        let conn = rusqlite::Connection::open(tp.dirs().data.join(INDEX_FILE)).unwrap();
        conn.execute_batch(&format!(
            "DROP TABLE reserved_audit; {PREVIOUS_AUDIT} PRAGMA user_version = 4;"
        ))
        .unwrap();
        for (id, r) in [3_i64, 5, 6, 9].iter().zip(&rows) {
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
    }

    let mut profile = tp.open();
    let migrated = profile.audit(0, 100).unwrap();
    let expected: Vec<(i64, AuditRow)> = [3, 5, 6, 9].into_iter().zip(rows).collect();
    assert_eq!(migrated, expected);
    // New rows follow the old ones.
    let id = profile
        .append_audit(&row(50, "expired", Some("risk-accepted"), Some("r1")))
        .unwrap();
    assert_eq!(id, 10);
    drop(profile);

    // Still append-only.
    let conn = rusqlite::Connection::open(tp.dirs().data.join(INDEX_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(version > 4, "not migrated: user_version {version}");
    for statement in [
        "UPDATE reserved_audit SET outcome = 'accepted' WHERE id = 3",
        "DELETE FROM reserved_audit WHERE id = 3",
    ] {
        let err = conn.execute(statement, []).unwrap_err();
        assert!(
            err.to_string().contains("append-only"),
            "{statement}: {err}"
        );
    }
    let leftovers: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'reserved_audit%'
                 AND name NOT IN ('reserved_audit', 'reserved_audit_no_update',
                                  'reserved_audit_no_delete')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leftovers, 0);
    drop(conn);
    // Opening again migrates nothing and keeps every row.
    assert_eq!(tp.open().audit(0, 100).unwrap().len(), 5);
}
