//! The Guardrails keys of a repo store (ADR-GRD-001 § 1 and § 4, ADR-GRP-006 § 4): the install
//! journal (the authoritative integrity reference, H-04), the developer's answer to the
//! permission (BR-AUTH-002) and the last refused install attempt. `store_meta` keys, no
//! migration; only the daemon writes them.

use gitraptor_policy::team::Confirmed;
use rusqlite::{OptionalExtension, params};

use super::Result;
use super::store::RepoStore;

const JOURNAL_KEY: &str = "guardrails.journal";
const PERMISSION_KEY: &str = "guardrails.permission";
const REFUSAL_KEY: &str = "guardrails.last_refusal";

/// The Guardrails keys of one repo, as stored (JSON and plain text).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardKeys {
    pub journal: Option<String>,
    pub permission: Option<String>,
    pub last_refusal: Option<String>,
}

impl RepoStore {
    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Every Guardrails key of the repo.
    pub fn guard_keys(&self) -> Result<GuardKeys> {
        Ok(GuardKeys {
            journal: self.meta(JOURNAL_KEY)?,
            permission: self.meta(PERMISSION_KEY)?,
            last_refusal: self.meta(REFUSAL_KEY)?,
        })
    }

    /// Writes some Guardrails keys (`Some(None)` deletes one) and, at the same time, the
    /// confirmed base branch and floor: one IMMEDIATE transaction, so an install is never
    /// confirmed without its base branch (ADR-GRD-001 § 4 paso 5, ADR-GRD-004 § 3.5).
    pub fn set_guard_keys(
        &mut self,
        journal: Option<Option<&str>>,
        permission: Option<&str>,
        last_refusal: Option<Option<&str>>,
        confirmed: Option<&Confirmed>,
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let put = |key: &str, value: Option<&str>| -> Result<()> {
            match value {
                Some(value) => tx.execute(
                    "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )?,
                None => tx.execute("DELETE FROM store_meta WHERE key = ?1", params![key])?,
            };
            Ok(())
        };
        if let Some(journal) = journal {
            put(JOURNAL_KEY, journal)?;
        }
        if let Some(permission) = permission {
            put(PERMISSION_KEY, Some(permission))?;
        }
        if let Some(refusal) = last_refusal {
            put(REFUSAL_KEY, refusal)?;
        }
        if let Some(confirmed) = confirmed {
            for (key, value) in super::team_baseline::rows(confirmed)? {
                put(key, Some(&value))?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
