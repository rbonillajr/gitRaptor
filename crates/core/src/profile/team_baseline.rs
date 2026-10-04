//! Confirmed base branch and floor of a repository, kept in its store (ADR-GRD-004 § 3 and § 4,
//! ADR-GRP-006 § 4). Two keys of `store_meta`, no migration.
//!
//! Only the developer's confirmation writes them (US-GRD-001 at install time, US-GRD-014 and
//! US-GRD-007 with a reserved command); no read ever confirms anything. A value that does not
//! validate reads as "not confirmed", which protects more, never less.

use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use rusqlite::{OptionalExtension, params};

use super::Result;
use super::store::RepoStore;

const BASE_KEY: &str = "guardrails.confirmed_base_branch";
const FLOOR_KEY: &str = "guardrails.confirmed_floor";
const FLOOR_ABSENT: &str = "absent";
const FLOOR_BLOB: &str = "blob:";

impl RepoStore {
    /// What the developer confirmed, or `None` if nothing was confirmed yet (`base-unconfirmed`).
    pub fn confirmed_team_baseline(&self) -> Result<Option<Confirmed>> {
        let get = |key: &str| -> Result<Option<String>> {
            Ok(self
                .conn
                .query_row(
                    "SELECT value FROM store_meta WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()?)
        };
        let (Some(base), Some(floor)) = (get(BASE_KEY)?, get(FLOOR_KEY)?) else {
            return Ok(None);
        };
        let Ok(base_branch) = RefName::new(&base) else {
            return Ok(None);
        };
        let floor = match floor.strip_prefix(FLOOR_BLOB) {
            None if floor == FLOOR_ABSENT => ConfirmedFloor::Absent,
            Some(id) if is_object_id(id) => ConfirmedFloor::Blob(id.to_owned()),
            _ => return Ok(None),
        };
        Ok(Some(Confirmed { base_branch, floor }))
    }

    /// Record a confirmation: both keys in one transaction.
    pub fn set_confirmed_team_baseline(&mut self, confirmed: &Confirmed) -> Result<()> {
        let floor = match &confirmed.floor {
            ConfirmedFloor::Absent => FLOOR_ABSENT.to_owned(),
            ConfirmedFloor::Blob(id) if is_object_id(id) => format!("{FLOOR_BLOB}{id}"),
            ConfirmedFloor::Blob(_) => {
                return Err(super::ProfileError::InvalidWrite(
                    "confirmed floor is not an object id".into(),
                ));
            }
        };
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for (key, value) in [
            (BASE_KEY, confirmed.base_branch.as_str()),
            (FLOOR_KEY, floor.as_str()),
        ] {
            tx.execute(
                "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// A SHA-1 or SHA-256 object id in lowercase hex.
fn is_object_id(id: &str) -> bool {
    matches!(id.len(), 40 | 64) && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
