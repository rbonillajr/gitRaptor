//! Reads of the oplog: snapshots and operations by worktree, period,
//! operation, snapshot and level, with the state derived from the journal.
//!
//! The requester of an operation is the frozen one (D-TMC-18). The current
//! actor is resolved on each query through [`CurrentAttribution`], which the
//! engine implements over its own store (ADR-GRP-013 § 2); the oplog never
//! stores it (Q37).

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::model::{
    CompleteInfo, JournalEntry, Notice, NoticeKind, OperationKind, OperationRecord, OperationState,
    OperationView, Requester, Scope, SnapshotLevel, SnapshotRecord, SnapshotState, SnapshotView,
    Target,
};
use super::recovery::SnapshotRefs;
use super::{Channel, Oplog};
use crate::profile::Result;

/// Resolves the current actor of a session at query time. Implemented by
/// the engine; tests use a fake that applies a correction.
pub trait CurrentAttribution {
    type Actor;
    fn current_actor(&self, session_id: &str) -> Option<Self::Actor>;
}

/// Filter of [`Oplog::snapshots`]. Unset fields do not filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotFilter {
    pub snapshot_id: Option<String>,
    pub level: Option<SnapshotLevel>,
    /// Recorded at or after, in ms since the epoch.
    pub from_ms: Option<i64>,
    /// Recorded before.
    pub to_ms: Option<i64>,
    /// Snapshots caused by, or prior to, this operation.
    pub operation_id: Option<String>,
    /// Snapshots that include this worktree.
    pub worktree: Option<String>,
}

/// Filter of [`Oplog::operations`]. Unset fields do not filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationFilter {
    pub operation_id: Option<String>,
    pub kind: Option<OperationKind>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    /// Operations whose prior snapshot, or whose restore target, is this.
    pub snapshot_id: Option<String>,
    /// Operations whose scope includes this worktree.
    pub worktree: Option<String>,
    /// Operations requested from this session.
    pub requester_session: Option<String>,
}

const SNAPSHOT_COLUMNS: &str = "SELECT s.snapshot_id, s.seq, s.level, s.worktrees, s.store_ref,
        s.engine_mark, s.cause_operation, s.cause_event_seq, s.recorded_ms
    FROM snapshots s
    WHERE (?1 IS NULL OR s.snapshot_id = ?1)
      AND (?2 IS NULL OR s.level = ?2)
      AND (?3 IS NULL OR s.recorded_ms >= ?3)
      AND (?4 IS NULL OR s.recorded_ms < ?4)
      AND (?5 IS NULL OR s.cause_operation = ?5 OR s.snapshot_id IN (
            SELECT related_id FROM journal
            WHERE entry = 'operation-state' AND state = 'prior-snapshot' AND subject_id = ?5))
      AND (?6 IS NULL OR CASE WHEN json_valid(s.worktrees)
            THEN EXISTS (SELECT 1 FROM json_each(s.worktrees) WHERE value = ?6)
            ELSE 0 END)
    ORDER BY s.seq";

const OPERATION_COLUMNS: &str = "SELECT o.operation_id, o.seq, o.kind, o.subtype, o.scope,
        o.requester, o.channel, o.confirmed, o.target, o.warnings, o.engine_mark, o.recorded_ms
    FROM operations o
    WHERE (?1 IS NULL OR o.operation_id = ?1)
      AND (?2 IS NULL OR o.kind = ?2)
      AND (?3 IS NULL OR o.recorded_ms >= ?3)
      AND (?4 IS NULL OR o.recorded_ms < ?4)
      AND (?5 IS NULL
           OR o.operation_id IN (SELECT subject_id FROM journal
                WHERE entry = 'operation-state' AND state = 'prior-snapshot' AND related_id = ?5)
           OR CASE WHEN json_valid(o.target)
                THEN json_extract(o.target, '$.snapshot') = ?5 ELSE 0 END)
      AND (?6 IS NULL OR CASE WHEN json_valid(o.scope)
            THEN EXISTS (SELECT 1 FROM json_each(o.scope, '$.worktrees') WHERE value = ?6)
            ELSE 0 END)
      AND (?7 IS NULL OR o.requester_session = ?7)
    ORDER BY o.seq";

impl Oplog {
    pub fn snapshot(&self, snapshot_id: &str) -> Result<Option<SnapshotView>> {
        Ok(self
            .snapshots(&SnapshotFilter {
                snapshot_id: Some(snapshot_id.to_owned()),
                ..SnapshotFilter::default()
            })?
            .pop())
    }

    /// Snapshots matching `filter`, in recording order, whatever their
    /// state. A row that does not verify is returned marked `tampered`.
    pub fn snapshots(&self, filter: &SnapshotFilter) -> Result<Vec<SnapshotView>> {
        let mut stmt = self.conn.prepare_cached(SNAPSHOT_COLUMNS)?;
        let rows = stmt
            .query_map(
                params![
                    filter.snapshot_id,
                    filter.level.map(SnapshotLevel::as_str),
                    filter.from_ms,
                    filter.to_ms,
                    filter.operation_id,
                    filter.worktree
                ],
                snapshot_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(record, parsed)| self.snapshot_view(record, parsed))
            .collect()
    }

    /// The snapshots of `filter` that are points of the timeline and can
    /// be restored: their ref exists in the store **and** their row is
    /// `complete` (or announced for purge), and nothing about them is at a
    /// break of the chain (ADR-TMC-003 § 3, SEC-TMC-09). Without a store,
    /// none is.
    pub fn offerable_snapshots(
        &self,
        filter: &SnapshotFilter,
        refs: &dyn SnapshotRefs,
    ) -> Result<Vec<SnapshotView>> {
        let Some(present) = refs.snapshot_ids()? else {
            return Ok(Vec::new());
        };
        Ok(self
            .snapshots(filter)?
            .into_iter()
            .filter(|s| {
                s.state.is_available() && !s.tampered && present.contains(&s.record.snapshot_id)
            })
            .collect())
    }

    fn snapshot_view(&self, record: SnapshotRecord, parsed: bool) -> Result<SnapshotView> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT seq, state, detail FROM journal
             WHERE entry = 'snapshot-state' AND subject_id = ?1 ORDER BY seq",
        )?;
        let entries = stmt
            .query_map(params![record.snapshot_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tampered = !parsed || entries.is_empty() || self.tampered.contains(&record.seq);
        let mut state = SnapshotState::Pending;
        let mut complete = None;
        for (seq, text, detail) in entries {
            tampered |= self.tampered.contains(&seq);
            match text.as_deref().map(SnapshotState::parse) {
                Some(Ok(s)) => state = s,
                _ => tampered = true,
            }
            if state == SnapshotState::Complete {
                complete = detail.and_then(|d| serde_json::from_str::<CompleteInfo>(&d).ok());
            }
        }
        Ok(SnapshotView {
            record,
            state,
            complete,
            tampered,
        })
    }

    pub fn operation(&self, operation_id: &str) -> Result<Option<OperationView>> {
        Ok(self
            .operations(&OperationFilter {
                operation_id: Some(operation_id.to_owned()),
                ..OperationFilter::default()
            })?
            .pop())
    }

    /// Operations matching `filter`, in recording order, with the frozen
    /// requester and the state of the journal.
    pub fn operations(&self, filter: &OperationFilter) -> Result<Vec<OperationView>> {
        let mut stmt = self.conn.prepare_cached(OPERATION_COLUMNS)?;
        let rows = stmt
            .query_map(
                params![
                    filter.operation_id,
                    filter.kind.map(OperationKind::as_str),
                    filter.from_ms,
                    filter.to_ms,
                    filter.snapshot_id,
                    filter.worktree,
                    filter.requester_session
                ],
                operation_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(record, parsed)| self.operation_view(record, parsed))
            .collect()
    }

    /// [`Oplog::operations`] with the current actor of each requester's
    /// session resolved now. The frozen requester is not changed.
    pub fn operations_with_current_actor<A: CurrentAttribution>(
        &self,
        filter: &OperationFilter,
        attribution: &A,
    ) -> Result<Vec<(OperationView, Option<A::Actor>)>> {
        Ok(self
            .operations(filter)?
            .into_iter()
            .map(|op| {
                let actor = op
                    .record
                    .requester
                    .session_id()
                    .and_then(|s| attribution.current_actor(s));
                (op, actor)
            })
            .collect())
    }

    fn operation_view(&self, record: OperationRecord, parsed: bool) -> Result<OperationView> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT seq, state, step, related_id FROM journal
             WHERE entry = 'operation-state' AND subject_id = ?1 ORDER BY seq",
        )?;
        let entries = stmt
            .query_map(params![record.operation_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tampered = !parsed || entries.is_empty() || self.tampered.contains(&record.seq);
        let mut state = OperationState::Intent;
        let mut step = None;
        let mut prior_snapshot = None;
        for (seq, text, entry_step, related) in entries {
            tampered |= self.tampered.contains(&seq);
            match text.as_deref().map(OperationState::parse) {
                Some(Ok(s)) => state = s,
                _ => tampered = true,
            }
            if state == OperationState::PriorSnapshot {
                prior_snapshot = related;
            }
            if let Some(s) = entry_step {
                step = u32::try_from(s).ok();
            }
        }
        Ok(OperationView {
            record,
            state,
            step,
            prior_snapshot,
            tampered,
        })
    }

    /// Every journal row about `subject_id`, in order.
    pub fn journal(&self, subject_id: &str) -> Result<Vec<JournalEntry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT seq, entry, subject_id, state, step, related_id, path, inode, pid, detail,
                 recorded_ms
             FROM journal WHERE subject_id = ?1 ORDER BY seq",
        )?;
        Ok(stmt
            .query_map(params![subject_id], journal_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every notice, in order, with its first delivery.
    pub fn notices(&self) -> Result<Vec<Notice>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT n.notice_id, n.seq, n.kind, n.worktree, n.operation_id, n.detail,
                 n.recorded_ms,
                 (SELECT MIN(j.recorded_ms) FROM journal j
                  WHERE j.entry = 'notice-delivered' AND j.subject_id = n.notice_id)
             FROM notices n ORDER BY n.seq",
        )?;
        Ok(stmt
            .query_map([], |row| {
                let kind = NoticeKind::parse(&row.get::<_, String>(2)?)?;
                let detail: String = row.get(5)?;
                Ok(Notice {
                    notice_id: row.get(0)?,
                    seq: row.get(1)?,
                    kind,
                    worktree: row.get(3)?,
                    operation_id: row.get(4)?,
                    detail: serde_json::from_str(&detail).unwrap_or(serde_json::Value::Null),
                    recorded_ms: row.get(6)?,
                    first_delivered_ms: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Notices no client has received yet that concern `worktree` (or the
    /// whole repo). A client connecting from that worktree shows them and
    /// marks them delivered, so each is shown once (US-TMC-019).
    pub fn pending_notices(&self, worktree: Option<&str>) -> Result<Vec<Notice>> {
        Ok(self
            .notices()?
            .into_iter()
            .filter(|n| n.first_delivered_ms.is_none())
            .filter(|n| n.worktree.is_none() || n.worktree.as_deref() == worktree)
            .collect())
    }

    /// First time a notice was shown on the CLI or the TUI: the start of
    /// the purge grace (ADR-TMC-007 § 4.2). MCP deliveries do not count.
    pub fn first_interactive_delivery_ms(&self, notice_id: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT MIN(recorded_ms) FROM journal
                 WHERE entry = 'notice-delivered' AND subject_id = ?1
                   AND state IN (?2, ?3)",
                params![notice_id, Channel::Cli.as_str(), Channel::Tui.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten())
    }
}

/// Reads a snapshot row. The flag is `false` when a JSON column does not
/// parse (an edited row); the view is then marked tampered.
fn snapshot_row(row: &Row<'_>) -> rusqlite::Result<(SnapshotRecord, bool)> {
    let worktrees: String = row.get(3)?;
    let parsed_worktrees = serde_json::from_str::<Vec<String>>(&worktrees).ok();
    let parsed = parsed_worktrees.is_some();
    Ok((
        SnapshotRecord {
            snapshot_id: row.get(0)?,
            seq: row.get(1)?,
            level: SnapshotLevel::parse(&row.get::<_, String>(2)?)?,
            worktrees: parsed_worktrees.unwrap_or_default(),
            store_ref: row.get(4)?,
            engine_mark: row.get(5)?,
            cause_operation: row.get(6)?,
            cause_event_seq: row.get(7)?,
            recorded_ms: row.get(8)?,
        },
        parsed,
    ))
}

fn operation_row(row: &Row<'_>) -> rusqlite::Result<(OperationRecord, bool)> {
    let scope = serde_json::from_str::<Scope>(&row.get::<_, String>(4)?).ok();
    let requester = serde_json::from_str::<Requester>(&row.get::<_, String>(5)?).ok();
    let target = serde_json::from_str::<Target>(&row.get::<_, String>(8)?).ok();
    let warnings = serde_json::from_str::<Vec<String>>(&row.get::<_, String>(9)?).ok();
    let parsed = scope.is_some() && requester.is_some() && target.is_some() && warnings.is_some();
    Ok((
        OperationRecord {
            operation_id: row.get(0)?,
            seq: row.get(1)?,
            kind: OperationKind::parse(&row.get::<_, String>(2)?)?,
            subtype: row.get(3)?,
            scope: scope.unwrap_or_default(),
            requester: requester.unwrap_or(Requester::Unattributed),
            channel: Channel::parse(&row.get::<_, String>(6)?)?,
            confirmed: row.get(7)?,
            target: target.unwrap_or(Target::None),
            warnings: warnings.unwrap_or_default(),
            engine_mark: row.get(10)?,
            recorded_ms: row.get(11)?,
        },
        parsed,
    ))
}

fn journal_row(row: &Row<'_>) -> rusqlite::Result<JournalEntry> {
    Ok(JournalEntry {
        seq: row.get(0)?,
        entry: row.get(1)?,
        subject_id: row.get(2)?,
        state: row.get(3)?,
        step: row.get(4)?,
        related_id: row.get(5)?,
        path: row.get(6)?,
        inode: row.get(7)?,
        pid: row.get(8)?,
        detail: row.get(9)?,
        recorded_ms: row.get(10)?,
    })
}

/// Every journal row of one kind of entry, in order. For the recovery.
pub(super) fn entries_of(conn: &Connection, entry: &str) -> Result<Vec<JournalEntry>> {
    let mut stmt = conn.prepare_cached(
        "SELECT seq, entry, subject_id, state, step, related_id, path, inode, pid, detail,
             recorded_ms
         FROM journal WHERE entry = ?1 ORDER BY seq",
    )?;
    Ok(stmt
        .query_map(params![entry], journal_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
