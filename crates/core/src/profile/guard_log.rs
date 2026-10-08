//! The Guardrails decision log of a repo store (US-GRD-005, ADR-GRD-006 § 1 to § 3 and § 6):
//! aggregation of identical occurrences, the insert cap with rows over it, retention and the
//! query with its KPI. Only the daemon loop writes it; the clock is the caller's.

use gitraptor_api::guard::{
    GuardLogEntry, GuardLogResult, GuardLogSummary, LOG_RETENTION_DAYS, LogDetail, LogLayer,
    LogOrigin, UnloggedPeriod,
};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::Result;
use super::store::RepoStore;
use crate::guardrails::log::{LogEntry, OverflowRow};

/// Window from an entry's first occurrence in which identical ones aggregate into it
/// (ASSUMPTION of § 2). Fixed, not sliding: a retry every minute does not hide in one row.
pub const AGGREGATION_WINDOW_MS: i64 = 60_000;
/// New rows a minute per repo before occurrences go to the rows over the cap (§ 2).
pub const NEW_ROWS_PER_MINUTE: i64 = 100;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;

/// Gap causes that mean the engine was not deciding: the hooks were alone and nothing was
/// logged (the degraded-mode spool is not built yet).
const ENGINE_DOWN: &str = "'machine-off', 'daemon-down', 'daemon-down-during-session', \
                           'daemon-stopped', 'profile-lost', 'store-corrupt'";

/// The code a value travels with (`"denial"`, `"claude-code"`): its JSON string.
fn code<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn conversion(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
}

fn from_code<T: DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(conversion)
}

fn from_json<T: DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(conversion)
}

fn retention_floor(now_ms: i64) -> i64 {
    now_ms - LOG_RETENTION_DAYS * DAY_MS
}

impl RepoStore {
    /// Logs one occurrence at `entry.at_ms`: aggregated into the same entry within the window,
    /// a new row while under the cap, or counted in the row over the cap of its kind, operation
    /// and rule. The count is never lost.
    pub fn record_guard_decision(&mut self, entry: &LogEntry) -> Result<()> {
        let now = entry.at_ms;
        let key = entry.agg_key();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let same: Option<i64> = tx
            .query_row(
                "SELECT id FROM guardrails_decisions
                 WHERE agg_key = ?1 AND detail = 'full' AND at_ms >= ?2
                 ORDER BY at_ms DESC LIMIT 1",
                params![key, now - AGGREGATION_WINDOW_MS],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = same {
            tx.execute(
                "UPDATE guardrails_decisions SET count = count + 1, last_ms = MAX(last_ms, ?2)
                 WHERE id = ?1",
                params![id, now],
            )?;
            tx.commit()?;
            return Ok(());
        }
        let recent: i64 = tx.query_row(
            "SELECT COUNT(*) FROM guardrails_decisions WHERE detail = 'full' AND at_ms > ?1",
            params![now - MINUTE_MS],
            |row| row.get(0),
        )?;
        if recent >= NEW_ROWS_PER_MINUTE {
            add_over_cap(&tx, &entry.overflow(), 1, now, now)?;
        } else {
            tx.execute(
                "INSERT INTO guardrails_decisions (at_ms, utc_offset_s, last_ms, count, worktree,
                     branch, actor, operation, kind, detail, effect, applied_effect, reasons,
                     layer, decision_id, origin, authorship, agg_key)
                 VALUES (?1, ?2, ?1, 1, ?3, ?4, ?5, ?6, ?7, 'full', ?8, ?9, ?10, ?11, ?12, ?13,
                     ?14, ?15)",
                params![
                    now,
                    entry.utc_offset_s,
                    entry.worktree,
                    entry.branch,
                    entry.actor.as_ref().map(code),
                    json(&entry.operation),
                    code(&entry.kind),
                    code(&entry.effect),
                    code(&entry.applied_effect),
                    json(&entry.reasons),
                    code(&LogLayer::Hooks),
                    entry.decision_id,
                    code(&LogOrigin::Daemon),
                    entry.authorship.as_ref().map(json),
                    key,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Adds occurrences that were never queued (the connection's overflow, D4) to the row over
    /// the cap of their kind, operation and rule.
    pub fn record_guard_overflow(&mut self, row: &OverflowRow) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        add_over_cap(&tx, row, row.count, row.first_ms, row.last_ms)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes the entries whose last occurrence is older than the retention (BR-TIME-002).
    pub fn purge_guard_log(&mut self, now_ms: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM guardrails_decisions WHERE last_ms < ?1",
            params![retention_floor(now_ms)],
        )?)
    }

    /// The entries of the period, most recent first, its KPI and the periods nothing was
    /// logged. The period never starts before the retention, purged or not.
    pub fn guard_log(&self, since_ms: i64, limit: u32, now_ms: i64) -> Result<GuardLogResult> {
        let since = since_ms.max(retention_floor(now_ms));
        let entries = self
            .conn
            .prepare(
                "SELECT at_ms, utc_offset_s, last_ms, count, worktree, branch, actor, operation,
                     kind, detail, effect, applied_effect, reasons, decision_id, authorship
                 FROM guardrails_decisions WHERE last_ms >= ?1
                 ORDER BY last_ms DESC, id DESC LIMIT ?2",
            )?
            .query_map(params![since, limit], entry_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let summary = self.conn.query_row(
            "SELECT
                 COALESCE(SUM(CASE WHEN kind = 'denial' AND origin = 'daemon'
                     THEN count END), 0),
                 COALESCE(SUM(CASE WHEN kind = 'notice' THEN count END), 0),
                 COALESCE(SUM(CASE WHEN detail = 'rate-limited' THEN count END), 0)
             FROM guardrails_decisions WHERE last_ms >= ?1",
            params![since],
            |row| {
                Ok(GuardLogSummary {
                    blocked: row.get::<_, i64>(0)?.max(0) as u64,
                    notices: row.get::<_, i64>(1)?.max(0) as u64,
                    rate_limited: row.get::<_, i64>(2)?.max(0) as u64,
                })
            },
        )?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT started_ms, ended_ms FROM gaps
             WHERE cause IN ({ENGINE_DOWN}) AND started_ms <= ?2
                 AND (ended_ms IS NULL OR ended_ms >= ?1)
             ORDER BY started_ms"
        ))?;
        let unlogged_periods = stmt
            .query_map(params![since, now_ms], |row| {
                Ok(UnloggedPeriod {
                    from_ms: row.get(0)?,
                    to_ms: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(GuardLogResult {
            since_ms: since,
            summary,
            entries,
            unlogged_periods,
        })
    }
}

/// Adds `count` occurrences to the row over the cap of `row`'s key in the current minute, or
/// opens it.
fn add_over_cap(
    tx: &rusqlite::Transaction<'_>,
    row: &OverflowRow,
    count: u64,
    first_ms: i64,
    last_ms: i64,
) -> Result<()> {
    let key = row.agg_key();
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    let open: Option<i64> = tx
        .query_row(
            "SELECT id FROM guardrails_decisions
             WHERE agg_key = ?1 AND detail = 'rate-limited' AND at_ms > ?2
             ORDER BY at_ms DESC LIMIT 1",
            params![key, last_ms - MINUTE_MS],
            |r| r.get(0),
        )
        .optional()?;
    match open {
        Some(id) => {
            tx.execute(
                "UPDATE guardrails_decisions SET count = count + ?2, last_ms = MAX(last_ms, ?3)
                 WHERE id = ?1",
                params![id, count, last_ms],
            )?;
        }
        None => {
            tx.execute(
                "INSERT INTO guardrails_decisions (at_ms, utc_offset_s, last_ms, count,
                     operation, kind, detail, effect, applied_effect, reasons, layer,
                     decision_id, origin, agg_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'rate-limited', ?7, ?8, ?9, ?10, '', ?11, ?12)",
                params![
                    first_ms,
                    row.utc_offset_s,
                    last_ms,
                    count,
                    json(&row.operation),
                    code(&row.kind),
                    code(&row.effect),
                    code(&row.applied_effect),
                    json(&row.reason.iter().collect::<Vec<_>>()),
                    code(&LogLayer::Hooks),
                    code(&LogOrigin::Daemon),
                    key,
                ],
            )?;
        }
    }
    Ok(())
}

fn entry_row(row: &Row<'_>) -> rusqlite::Result<GuardLogEntry> {
    let text = |i: usize| row.get::<_, Option<String>>(i);
    Ok(GuardLogEntry {
        at_ms: row.get(0)?,
        utc_offset_s: row.get(1)?,
        last_ms: row.get(2)?,
        count: row.get::<_, i64>(3)?.max(0) as u64,
        worktree: text(4)?.map(gitraptor_api::Untrusted::new),
        branch: text(5)?.map(gitraptor_api::Untrusted::new),
        actor: text(6)?.as_deref().map(from_code).transpose()?,
        operation: from_json(&row.get::<_, String>(7)?)?,
        kind: from_code(&row.get::<_, String>(8)?)?,
        detail: from_code::<LogDetail>(&row.get::<_, String>(9)?)?,
        effect: from_code(&row.get::<_, String>(10)?)?,
        applied_effect: from_code(&row.get::<_, String>(11)?)?,
        reasons: from_json(&row.get::<_, String>(12)?)?,
        layer: LogLayer::Hooks,
        origin: LogOrigin::Daemon,
        decision_id: row.get(13)?,
        authorship: text(14)?.as_deref().map(from_json).transpose()?,
    })
}
