//! An audit row as the contract exposes it (`audit.list` and `reserved.audit`).

use gitraptor_api::messages::{AuditEntry, AuditOutcome, ClientIdentity, RefusalReason};

use crate::profile::AuditRow;

/// The reason text of the end of an announced action: the risk it accepted, by version of
/// ADR-GRD-007 § 2. The contract carries no reason for it.
pub(crate) const RISK_ACCEPTED_PREFIX: &str = "risk-accepted";

/// What the audit keeps of the requester of an announced action.
#[derive(serde::Deserialize)]
struct Process {
    pid: u32,
    start_us: u64,
}

fn from_text<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

/// The entry of a row, or `None` when the row cannot be read: a corrupt row never reaches a
/// client.
pub(crate) fn audit_entry(id: i64, row: &AuditRow) -> Option<AuditEntry> {
    let outcome: AuditOutcome = from_text(&row.outcome)?;
    let reason = match &row.reason {
        None => None,
        Some(r) if outcome.needs_capability() && r.starts_with(RISK_ACCEPTED_PREFIX) => None,
        Some(r) => Some(from_text::<RefusalReason>(r)?),
    };
    let (client, client_partial) = match serde_json::from_str::<ClientIdentity>(&row.client) {
        Ok(client) => (client, false),
        // The end of an announced action keeps only the process: say so instead of
        // inventing what was not recorded.
        Err(_) if outcome.needs_capability() => {
            let p: Process = serde_json::from_str(&row.client).ok()?;
            let client = ClientIdentity {
                pid: p.pid,
                start_us: p.start_us,
                exe: None,
                agent_ancestor: false,
                daemon_descendant: false,
                controlling_terminal: false,
                chain_truncated: false,
            };
            (client, true)
        }
        Err(_) => return None,
    };
    Some(AuditEntry {
        id,
        at_ms: row.at_ms,
        operation: row.operation.clone(),
        repo_id: row.repo_id.clone(),
        outcome,
        reason,
        client,
        client_partial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(outcome: &str, reason: Option<&str>, client: &str) -> AuditRow {
        AuditRow {
            at_ms: 5,
            operation: "guard.uninstall".into(),
            repo_id: Some("r1".into()),
            outcome: outcome.into(),
            reason: reason.map(str::to_owned),
            client: client.into(),
            chain: "{}".into(),
        }
    }

    #[test]
    fn the_end_of_an_announced_action_is_an_entry_without_a_reason_and_with_a_partial_client() {
        for outcome in ["applied", "cancelled", "failed", "expired"] {
            let r = row(
                outcome,
                Some(
                    "risk-accepted: ADR-GRD-007 § 2 (2026-10-04); uncovered: planted-code; action a1",
                ),
                r#"{"pid":7,"start_us":11}"#,
            );
            let e = audit_entry(3, &r).unwrap_or_else(|| panic!("{outcome} was hidden"));
            assert!(e.outcome.needs_capability());
            assert_eq!((e.reason, e.client_partial, e.client.pid), (None, true, 7));
        }
    }

    #[test]
    fn a_row_that_cannot_be_read_is_skipped() {
        assert!(audit_entry(1, &row("approved", None, "{}")).is_none());
        assert!(audit_entry(1, &row("applied", None, "not json")).is_none());
        // A legacy outcome does not get the partial client: it keeps being hidden.
        assert!(audit_entry(1, &row("accepted", None, r#"{"pid":1,"start_us":2}"#)).is_none());
        assert!(audit_entry(1, &row("rejected", Some("made-up"), "{}")).is_none());
    }
}
