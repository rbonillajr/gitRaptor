//! The Time Machine oplog: one append-only SQLite file per repo with the
//! snapshots, the operations with their frozen requester, the journal of
//! states and the pending notices (TS-TMC-002, ADR-TMC-003).
//!
//! Only the daemon writes it. Every row is immutable (triggers reject
//! `UPDATE` and `DELETE`) and chained by hash, with the head also kept in a
//! file next to the oplog ([`chain`]). The state of a snapshot or an
//! operation is the last transition of the journal, never a column that
//! changes.
//!
//! The oplog lives next to the snapshot store (`<data>/tm/<repo_id>/`) and
//! never shares a file with the engine store: losing the engine store loses
//! attribution, not snapshots (ADR-TMC-003 § 1, Q26). Engine data (events,
//! sessions, attribution) is referenced by sequence or session id and
//! resolved at query time, never copied.

mod chain;
#[cfg(test)]
mod hook_prior_migration_tests;
#[cfg(test)]
#[path = "migration_tests.rs"]
mod migration_tests;
mod model;
mod query;
mod recovery;
#[cfg(test)]
mod recovery_tests;
mod schema;
mod stack;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::profile::{ProfileDirs, ProfileError, Result, fsperm, sqlite};

use chain::{Hash, RowKind};

pub use model::{
    BreakCause, ChainBreak, Channel, CompleteInfo, Exclusion, HookPriorMeta, JournalEntry,
    ManualMeta, NewOperation, NewSnapshot, Notice, NoticeKind, OpRef, OperationKind, OperationRecord,
    OperationState, OperationView, Requester, RequesterOrigin, Scope, SnapshotLevel,
    SnapshotRecord, SnapshotState, SnapshotView, Target,
};
pub use query::{CurrentAttribution, OperationFilter, SnapshotFilter};
pub use recovery::{
    AbsentStore, FileIdentity, KeptLock, KeptLockReason, ProcessProbe, RecoveryOptions,
    RecoveryReport, SnapshotRefs, SystemProbe, file_identity,
};
pub use stack::{ExternalEvent, StackItem, StackScope, UndoStack};

/// Folder of the Time Machine inside the data folder.
pub const TM_DIR: &str = "tm";
/// File name of the oplog inside `tm/<repo_id>/`.
pub const OPLOG_FILE: &str = "oplog.db";
/// File with the chain head, next to the oplog.
pub const HEAD_FILE: &str = "oplog.head";
/// Prefix of the snapshot refs in the store (ADR-TMC-001).
pub const SNAPSHOT_REF_PREFIX: &str = "refs/tm/snap/";

/// How the oplog file was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OplogStatus {
    Existing,
    Created,
    /// The oplog was corrupt and was moved to `quarantined`; a new one
    /// starts with a declared break (SEC-TMC-09).
    Recovered {
        quarantined: PathBuf,
    },
}

/// What opening the oplog found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OplogOpen {
    pub status: OplogStatus,
    /// Breaks found by this open that were not declared yet. Each one was
    /// declared now with a `chain-break` journal entry.
    pub new_breaks: Vec<ChainBreak>,
}

/// A step of an operation (ADR-TMC-003 § 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTransition<'a> {
    /// The prior snapshot completed in the store and in the oplog.
    PriorSnapshot {
        snapshot_id: &'a str,
    },
    Ready,
    /// Annotated before the applier runs step `step` (ADR-TMC-002 § 3).
    Applying {
        step: u32,
    },
    Finished,
    /// Permissions, overlap or preconditions: the repo was not touched. From
    /// `ready`, the applier found a precondition failed under its locks.
    Rejected {
        reason: &'a str,
    },
    /// The prior snapshot failed: the repo was not touched.
    Aborted {
        reason: &'a str,
    },
    /// The process died or a failure was detected while applying.
    Interrupted,
}

impl OperationTransition<'_> {
    fn state(self) -> OperationState {
        match self {
            Self::PriorSnapshot { .. } => OperationState::PriorSnapshot,
            Self::Ready => OperationState::Ready,
            Self::Applying { .. } => OperationState::Applying,
            Self::Finished => OperationState::Finished,
            Self::Rejected { .. } => OperationState::Rejected,
            Self::Aborted { .. } => OperationState::Aborted,
            Self::Interrupted => OperationState::Interrupted,
        }
    }
}

/// The open oplog of one repo.
pub struct Oplog {
    conn: Connection,
    repo_id: String,
    head_path: PathBuf,
    /// Every break known: found now or declared before.
    breaks: Vec<ChainBreak>,
    /// Sequences of rows that are at a break and cannot be trusted.
    tampered: HashSet<i64>,
}

/// Folder of the Time Machine data of a repo. `repo_id` must be the opaque
/// key of the index; anything that could escape the folder is refused.
pub fn repo_dir(dirs: &ProfileDirs, repo_id: &str) -> Result<PathBuf> {
    let safe = !repo_id.is_empty()
        && repo_id.len() <= 64
        && repo_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    if !safe {
        return Err(ProfileError::InvalidPath {
            path: PathBuf::from(repo_id),
            reason: "not a repo key".into(),
        });
    }
    Ok(dirs.data.join(TM_DIR).join(repo_id))
}

impl Oplog {
    /// Opens (or creates) the oplog of `repo_id`, verifies the hash chain
    /// against the head and declares any new break. Folders are 0700 and
    /// files 0600 (SEC-TMC-01); a corrupt file is set aside, never deleted.
    pub fn open(dirs: &ProfileDirs, repo_id: &str, now_ms: i64) -> Result<(Self, OplogOpen)> {
        let dir = repo_dir(dirs, repo_id)?;
        fsperm::ensure_private_dir(&dirs.data.join(TM_DIR))?;
        fsperm::ensure_private_dir(&dir)?;
        let path = dir.join(OPLOG_FILE);
        let head_path = dir.join(HEAD_FILE);
        restore_interrupted(&path)?;
        let existed = path.exists();
        let opened = open_guarded(&path, &dirs.quarantine_dir(), &|conn| {
            chain::verify(conn, repo_id)
        })?;
        // On macOS a plain fsync does not flush the drive cache: a `complete` row could be lost
        // to a power cut after the protected operation ran (NFR-01). No effect elsewhere.
        opened.conn.pragma_update(None, "fullfsync", true)?;
        opened
            .conn
            .pragma_update(None, "checkpoint_fullfsync", true)?;
        let status = match opened.quarantined {
            Some(quarantined) => {
                // The old head belongs to the old file: keep it with it.
                if head_path.exists() {
                    let mut aside = quarantined.clone().into_os_string();
                    aside.push(".head");
                    fs::rename(&head_path, PathBuf::from(aside))?;
                }
                OplogStatus::Recovered { quarantined }
            }
            None if !existed => OplogStatus::Created,
            None => OplogStatus::Existing,
        };

        let mut oplog = Self {
            conn: opened.conn,
            repo_id: repo_id.to_owned(),
            head_path,
            breaks: Vec::new(),
            tampered: HashSet::new(),
        };
        let mut found = chain::verify(&oplog.conn, repo_id)?;
        if let Some(head) = chain::check_head(&oplog.conn, &oplog.head_path)? {
            found.push(head);
        }
        if matches!(status, OplogStatus::Recovered { .. }) {
            found.push(ChainBreak {
                seq: 0,
                cause: BreakCause::Quarantined,
            });
        }
        let declared = oplog.declared_breaks()?;
        let new_breaks: Vec<ChainBreak> = found
            .iter()
            .filter(|b| !declared.contains(b))
            .cloned()
            .collect();
        if new_breaks.is_empty() {
            // Brings a head left one batch behind up to date.
            if let Some(tip) = chain::tip(&oplog.conn)? {
                chain::write_head(&oplog.head_path, &tip)?;
            }
        } else {
            oplog.write(|batch| {
                for b in &new_breaks {
                    let detail = serde_json::json!({ "seq": b.seq }).to_string();
                    batch.journal(
                        &Entry {
                            entry: "chain-break",
                            state: Some(b.cause.as_str()),
                            detail: Some(&detail),
                            ..Entry::default()
                        },
                        now_ms,
                    )?;
                }
                Ok(())
            })?;
        }
        let mut breaks: Vec<ChainBreak> = declared.into_iter().collect();
        breaks.extend(new_breaks.iter().cloned());
        breaks.sort_by_key(|b| (b.seq, b.cause.as_str()));
        breaks.dedup();
        oplog.tampered = breaks
            .iter()
            .filter(|b| {
                matches!(
                    b.cause,
                    BreakCause::RowAltered | BreakCause::LinkBroken | BreakCause::Unchained
                )
            })
            .map(|b| b.seq)
            .collect();
        oplog.breaks = breaks;
        Ok((oplog, OplogOpen { status, new_breaks }))
    }

    /// The last row's sequence (0 for an empty oplog).
    pub fn last_seq(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM chain", [], |r| r.get(0))?)
    }

    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    /// Every break of the chain, declared now or before. The timeline shows
    /// each one as a gap with its cause (SEC-TMC-09).
    pub fn breaks(&self) -> &[ChainBreak] {
        &self.breaks
    }

    /// Walks the chain again; for diagnostics and tests.
    pub fn verify_chain(&self) -> Result<Vec<ChainBreak>> {
        chain::verify(&self.conn, &self.repo_id)
    }

    fn declared_breaks(&self) -> Result<HashSet<ChainBreak>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state, detail FROM journal WHERE entry = 'chain-break'")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })?;
        let mut declared = HashSet::new();
        for row in rows {
            let (state, detail) = row?;
            let cause = state.as_deref().and_then(|s| BreakCause::parse(s).ok());
            let seq = detail
                .as_deref()
                .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
                .and_then(|v| v.get("seq").and_then(serde_json::Value::as_i64));
            if let (Some(cause), Some(seq)) = (cause, seq) {
                declared.insert(ChainBreak { seq, cause });
            }
        }
        Ok(declared)
    }

    /// Runs `f` in one immediate transaction that appends chained rows,
    /// then moves the head. Nothing is written if `f` fails.
    fn write<T>(&mut self, f: impl FnOnce(&mut Batch<'_>) -> Result<T>) -> Result<T> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (next_seq, prev) = match chain::tip(&tx)? {
            Some(tip) => (tip.seq + 1, tip.hash),
            None => (1, chain::genesis(&self.repo_id)),
        };
        let mut batch = Batch {
            tx,
            next_seq,
            batch: next_seq,
            prev,
        };
        let out = f(&mut batch)?;
        let tip = chain::tip(&batch.tx)?;
        batch.tx.commit()?;
        if let Some(tip) = tip {
            chain::write_head(&self.head_path, &tip)?;
        }
        Ok(out)
    }

    // ----- Snapshots ------------------------------------------------------

    /// Records a snapshot whose capture is starting, in state `pending`.
    /// Must be called **before** the ref is created in the store: a ref
    /// without a row is never deleted by the recovery (ADR-TMC-003 § 6.1).
    pub fn begin_snapshot(&mut self, new: &NewSnapshot, now_ms: i64) -> Result<String> {
        let worktrees = to_json(&new.worktrees)?;
        self.write(|batch| {
            let id = sqlite::new_uuid(&batch.tx)?;
            let store_ref = [SNAPSHOT_REF_PREFIX, &id].concat();
            batch.append(RowKind::Snapshot, |tx, seq| {
                tx.execute(
                    "INSERT INTO snapshots (snapshot_id, seq, level, worktrees, store_ref,
                         engine_mark, cause_operation, cause_event_seq, recorded_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        id,
                        seq,
                        new.level.as_str(),
                        worktrees,
                        store_ref,
                        new.engine_mark,
                        new.cause_operation,
                        new.cause_event_seq,
                        now_ms
                    ],
                )
            })?;
            batch.journal(
                &Entry {
                    entry: "snapshot-state",
                    subject_id: Some(&id),
                    state: Some(SnapshotState::Pending.as_str()),
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(id)
        })
    }

    /// Records a `manual` snapshot whose capture is starting, in state `pending`, with who asked
    /// for it and what for. Every row of the attempt is marked with `meta.requested_ms`. The
    /// request must have a session (an unattributed one never gets a point) and the snapshot
    /// must be of level `manual`; anything else fails without writing.
    pub fn begin_manual_snapshot(
        &mut self,
        new: &NewSnapshot,
        meta: &ManualMeta,
    ) -> Result<String> {
        let Some(session) = meta.requester.session_id() else {
            return Err(ProfileError::InvalidWrite(
                "a manual snapshot needs a requester with a session".into(),
            ));
        };
        if new.level != SnapshotLevel::Manual {
            return Err(ProfileError::InvalidWrite(
                "begin_manual_snapshot takes a manual snapshot".into(),
            ));
        }
        let worktrees = to_json(&new.worktrees)?;
        let requester = to_json(&meta.requester)?;
        let now_ms = meta.requested_ms;
        self.write(|batch| {
            let id = sqlite::new_uuid(&batch.tx)?;
            let store_ref = [SNAPSHOT_REF_PREFIX, &id].concat();
            batch.append(RowKind::Snapshot, |tx, seq| {
                tx.execute(
                    "INSERT INTO snapshots (snapshot_id, seq, level, worktrees, store_ref,
                         engine_mark, cause_operation, cause_event_seq, recorded_ms,
                         label, requester, requester_session, worktree_key, channel)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        id,
                        seq,
                        new.level.as_str(),
                        worktrees,
                        store_ref,
                        new.engine_mark,
                        new.cause_operation,
                        new.cause_event_seq,
                        now_ms,
                        meta.label,
                        requester,
                        session,
                        meta.worktree_key,
                        meta.channel.as_str()
                    ],
                )
            })?;
            batch.journal(
                &Entry {
                    entry: "snapshot-state",
                    subject_id: Some(&id),
                    state: Some(SnapshotState::Pending.as_str()),
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(id)
        })
    }

    /// Records a `hook-prior` snapshot whose capture is starting, in state `pending`, with who
    /// asked for it. Every row of the attempt is marked with `meta.requested_ms`. An
    /// unattributed requester is allowed (it has no session); the snapshot must be of level
    /// `hook-prior`, anything else fails without writing.
    pub fn begin_hook_prior_snapshot(
        &mut self,
        new: &NewSnapshot,
        meta: &HookPriorMeta,
    ) -> Result<String> {
        if new.level != SnapshotLevel::HookPrior {
            return Err(ProfileError::InvalidWrite(
                "begin_hook_prior_snapshot takes a hook-prior snapshot".into(),
            ));
        }
        let worktrees = to_json(&new.worktrees)?;
        let requester = to_json(&meta.requester)?;
        let session = meta.requester.session_id();
        let now_ms = meta.requested_ms;
        self.write(|batch| {
            let id = sqlite::new_uuid(&batch.tx)?;
            let store_ref = [SNAPSHOT_REF_PREFIX, &id].concat();
            batch.append(RowKind::Snapshot, |tx, seq| {
                tx.execute(
                    "INSERT INTO snapshots (snapshot_id, seq, level, worktrees, store_ref,
                         engine_mark, cause_operation, cause_event_seq, recorded_ms,
                         requester, requester_session, worktree_key, channel)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        id,
                        seq,
                        new.level.as_str(),
                        worktrees,
                        store_ref,
                        new.engine_mark,
                        new.cause_operation,
                        new.cause_event_seq,
                        now_ms,
                        requester,
                        session,
                        meta.worktree_key,
                        Channel::Hook.as_str()
                    ],
                )
            })?;
            batch.journal(
                &Entry {
                    entry: "snapshot-state",
                    subject_id: Some(&id),
                    state: Some(SnapshotState::Pending.as_str()),
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(id)
        })
    }

    /// Marks a snapshot `complete`, after its ref exists in the store.
    pub fn complete_snapshot(
        &mut self,
        snapshot_id: &str,
        info: &CompleteInfo,
        now_ms: i64,
    ) -> Result<()> {
        let detail = to_json(info)?;
        self.snapshot_transition(snapshot_id, SnapshotState::Complete, Some(&detail), now_ms)
    }

    /// Any other snapshot transition: discarded or the purge states of
    /// ADR-TMC-007 § 4. Invalid transitions fail.
    pub fn set_snapshot_state(
        &mut self,
        snapshot_id: &str,
        state: SnapshotState,
        now_ms: i64,
    ) -> Result<()> {
        if state == SnapshotState::Complete {
            return Err(ProfileError::InvalidWrite(
                "use complete_snapshot to complete a snapshot".into(),
            ));
        }
        self.snapshot_transition(snapshot_id, state, None, now_ms)
    }

    fn snapshot_transition(
        &mut self,
        snapshot_id: &str,
        next: SnapshotState,
        detail: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        self.write(|batch| {
            let current = snapshot_state(&batch.tx, snapshot_id)?
                .ok_or_else(|| ProfileError::InvalidWrite("unknown snapshot".into()))?;
            if !current.can_become(next) {
                return Err(invalid_transition(current.as_str(), next.as_str()));
            }
            batch.journal(
                &Entry {
                    entry: "snapshot-state",
                    subject_id: Some(snapshot_id),
                    state: Some(next.as_str()),
                    detail,
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(())
        })
    }

    // ----- Operations -----------------------------------------------------

    /// Records an operation and its intent, before anything is touched
    /// (ADR-TMC-003 § 3, step 1). The row, with the requester as resolved
    /// now, is never rewritten (D-TMC-18).
    pub fn record_operation(&mut self, new: &NewOperation, now_ms: i64) -> Result<String> {
        let scope = to_json(&new.scope)?;
        let requester = to_json(&new.requester)?;
        let target = to_json(&new.target)?;
        let warnings = to_json(&new.warnings)?;
        self.write(|batch| {
            let id = sqlite::new_uuid(&batch.tx)?;
            batch.append(RowKind::Operation, |tx, seq| {
                tx.execute(
                    "INSERT INTO operations (operation_id, seq, kind, subtype, scope, requester,
                         requester_session, channel, confirmed, target, warnings, engine_mark,
                         recorded_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        id,
                        seq,
                        new.kind.as_str(),
                        new.subtype,
                        scope,
                        requester,
                        new.requester.session_id(),
                        new.channel.as_str(),
                        new.confirmed,
                        target,
                        warnings,
                        new.engine_mark,
                        now_ms
                    ],
                )
            })?;
            batch.journal(
                &Entry {
                    entry: "operation-state",
                    subject_id: Some(&id),
                    state: Some(OperationState::Intent.as_str()),
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(id)
        })
    }

    /// Annotates the next state of an operation. Each state is annotated
    /// before it is acted on; invalid transitions fail.
    pub fn advance_operation(
        &mut self,
        operation_id: &str,
        transition: OperationTransition<'_>,
        now_ms: i64,
    ) -> Result<()> {
        self.write(|batch| advance(batch, operation_id, transition, now_ms))
    }

    /// Annotates a Git lock the applier is about to take (ADR-TMC-003 § 2),
    /// with the identity of the file it took (see [`file_identity`]).
    pub fn record_lock_taken(
        &mut self,
        operation_id: &str,
        path: &Path,
        identity: FileIdentity,
        now_ms: i64,
    ) -> Result<()> {
        let path = path_text(path)?;
        let inode = recovery::inode_to_column(identity.inode);
        self.write(|batch| {
            require_operation(&batch.tx, operation_id)?;
            batch.journal(
                &Entry {
                    entry: "lock-taken",
                    subject_id: Some(operation_id),
                    path: Some(&path),
                    inode: Some(inode),
                    birth_ns: identity.birth_ns,
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(())
        })
    }

    pub fn record_lock_released(
        &mut self,
        operation_id: &str,
        path: &Path,
        now_ms: i64,
    ) -> Result<()> {
        let path = path_text(path)?;
        self.write(|batch| {
            require_operation(&batch.tx, operation_id)?;
            batch.journal(
                &Entry {
                    entry: "lock-released",
                    subject_id: Some(operation_id),
                    path: Some(&path),
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(())
        })
    }

    /// Annotates a child process the applier started (ADR-TMC-003 § 2), with
    /// its start time (µs since the epoch, as the channel's process reader
    /// gives it): with the pid, the identity recovery checks, so a reused pid
    /// is not taken for the child.
    pub fn record_child_started(
        &mut self,
        operation_id: &str,
        pid: u32,
        start_us: Option<u64>,
        now_ms: i64,
    ) -> Result<()> {
        let detail = start_us.map(|s| serde_json::json!({ "start_us": s }).to_string());
        self.child_entry(
            "child-started",
            operation_id,
            pid,
            detail.as_deref(),
            now_ms,
        )
    }

    pub fn record_child_ended(&mut self, operation_id: &str, pid: u32, now_ms: i64) -> Result<()> {
        self.child_entry("child-ended", operation_id, pid, None, now_ms)
    }

    fn child_entry(
        &mut self,
        entry: &'static str,
        operation_id: &str,
        pid: u32,
        detail: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        self.write(|batch| {
            require_operation(&batch.tx, operation_id)?;
            batch.journal(
                &Entry {
                    entry,
                    subject_id: Some(operation_id),
                    pid: Some(i64::from(pid)),
                    detail,
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(())
        })
    }

    // ----- Notices --------------------------------------------------------

    /// Records a notice for clients: an interruption (one per worktree) or
    /// an announced purge (whole repo).
    pub fn record_notice(
        &mut self,
        kind: NoticeKind,
        worktree: Option<&str>,
        operation_id: Option<&str>,
        detail: &serde_json::Value,
        now_ms: i64,
    ) -> Result<String> {
        let detail = detail.to_string();
        self.write(|batch| notice(batch, kind, worktree, operation_id, &detail, now_ms))
    }

    /// Records that a client received a notice. For a purge, only the first
    /// delivery on the CLI or the TUI starts the grace period (ADR-TMC-007
    /// § 4.2).
    pub fn mark_notice_delivered(
        &mut self,
        notice_id: &str,
        channel: Channel,
        now_ms: i64,
    ) -> Result<()> {
        self.write(|batch| {
            let known: Option<i64> = batch
                .tx
                .query_row(
                    "SELECT seq FROM notices WHERE notice_id = ?1",
                    params![notice_id],
                    |row| row.get(0),
                )
                .optional()?;
            if known.is_none() {
                return Err(ProfileError::InvalidWrite("unknown notice".into()));
            }
            batch.journal(
                &Entry {
                    entry: "notice-delivered",
                    subject_id: Some(notice_id),
                    state: Some(channel.as_str()),
                    ..Entry::default()
                },
                now_ms,
            )?;
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Migration 3 broke the hash chain where it held before: the oplog was put back as it was and
/// the Time Machine of the repo stays closed (the open fails with this as its source, see
/// [`migration_broke`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationBroke {
    /// The breaks the migration introduced.
    pub new_breaks: Vec<ChainBreak>,
}

impl std::fmt::Display for MigrationBroke {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the oplog migration broke the hash chain at {} row(s); the previous oplog was restored",
            self.new_breaks.len()
        )
    }
}

impl std::error::Error for MigrationBroke {}

/// The [`MigrationBroke`] an open failed with, if it is that error.
pub fn migration_broke(err: &ProfileError) -> Option<&MigrationBroke> {
    match err {
        ProfileError::Io(io) => io.get_ref()?.downcast_ref::<MigrationBroke>(),
        _ => None,
    }
}

/// How a migration checks the chain: the walk of [`chain::verify`] in production.
type Verify<'a> = &'a dyn Fn(&Connection) -> Result<Vec<ChainBreak>>;

/// Suffix of the copy of the oplog taken before a migration.
const COPY_SUFFIX: &str = ".pre-migration";

/// Opens the oplog migrating it. Before a migration of an existing file, a copy is taken with
/// SQLite's own `VACUUM INTO` (consistent whatever the WAL holds), and after it the chain is
/// walked again: a break that was not there before restores the copy and fails the open
/// (NFR-01). A migration that fails rolls back inside its transaction, so the file is untouched.
fn open_guarded(
    path: &Path,
    quarantine_dir: &Path,
    verify: Verify<'_>,
) -> Result<sqlite::OpenedDb> {
    let Some(copy) = take_copy(path, verify)? else {
        return sqlite::open_db(path, schema::OPLOG_MIGRATIONS, quarantine_dir);
    };
    let opened = match sqlite::open_db(path, schema::OPLOG_MIGRATIONS, quarantine_dir) {
        Ok(opened) => opened,
        Err(err) => {
            copy.discard();
            return Err(err);
        }
    };
    let after = match verify(&opened.conn) {
        Ok(after) => after,
        Err(err) => {
            drop(opened);
            copy.restore(path)?;
            return Err(err);
        }
    };
    let new_breaks: Vec<ChainBreak> = after
        .into_iter()
        .filter(|b| !copy.before.contains(b))
        .collect();
    if new_breaks.is_empty() {
        copy.discard();
        return Ok(opened);
    }
    drop(opened);
    copy.restore(path)?;
    Err(ProfileError::Io(io::Error::new(
        io::ErrorKind::InvalidData,
        MigrationBroke { new_breaks },
    )))
}

/// The copy of an oplog taken before migrating it, and the breaks the chain had then.
struct PreMigrationCopy {
    path: PathBuf,
    before: Vec<ChainBreak>,
}

impl PreMigrationCopy {
    /// The migration went well: the copy is not needed.
    fn discard(self) {
        remove_with_companions(&self.path);
    }

    /// Puts the copy back in place of the migrated file.
    fn restore(self, original: &Path) -> Result<()> {
        put_back(&self.path, original)
    }
}

/// A restore that stopped after the oplog was gone but before the copy was back leaves only the
/// copy, which is the oplog as it was before the migration: put it back before anything opens.
fn restore_interrupted(path: &Path) -> Result<()> {
    let mut leftover = path.as_os_str().to_owned();
    leftover.push(COPY_SUFFIX);
    let leftover = PathBuf::from(leftover);
    if !path.exists() && leftover.exists() {
        put_back(&leftover, path)?;
    }
    Ok(())
}

/// Renames `copy` over `original` in one step, so there is no moment without an oplog. Only the
/// side files of the migrated database go first: they belong to it, not to the copy.
fn put_back(copy: &Path, original: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut companion = original.as_os_str().to_owned();
        companion.push(suffix);
        let _ = fs::remove_file(PathBuf::from(companion));
    }
    fs::rename(copy, original)?;
    fsperm::set_private_file_mode(original)?;
    Ok(())
}

/// Removes a database file and the `-wal`, `-shm` and `-journal` files SQLite keeps beside it.
fn remove_with_companions(path: &Path) {
    let _ = fs::remove_file(path);
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        let _ = fs::remove_file(PathBuf::from(companion));
    }
}

/// Takes the copy when `path` is an oplog of an older version that a migration will change.
/// `None` when there is nothing to protect: no file, a new file, one already migrated, one this
/// build cannot open (the open decides: newer schema, corrupt) or an unreadable one.
fn take_copy(path: &Path, verify: Verify<'_>) -> Result<Option<PreMigrationCopy>> {
    if !path.exists() {
        return Ok(None);
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let Ok(source) = Connection::open_with_flags(path, flags) else {
        return Ok(None);
    };
    let version = source
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0);
    let current = i64::try_from(schema::OPLOG_MIGRATIONS.len()).unwrap_or(i64::MAX);
    if version <= 0 || version >= current {
        return Ok(None);
    }
    let mut copy = path.as_os_str().to_owned();
    copy.push(COPY_SUFFIX);
    let copy = PathBuf::from(copy);
    remove_with_companions(&copy);
    // Created private and empty first: `VACUUM INTO` accepts an empty file.
    fsperm::create_private_file(&copy)?;
    if let Err(err) = source.execute("VACUUM INTO ?1", params![path_text(&copy)?]) {
        remove_with_companions(&copy);
        return match err.sqlite_error_code() {
            // A damaged file is the open's to set aside, as it always was.
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => Ok(None),
            _ => Err(err.into()),
        };
    }
    drop(source);
    let before = Connection::open_with_flags(&copy, flags)
        .map_err(ProfileError::from)
        .and_then(|conn| verify(&conn));
    match before {
        Ok(before) => Ok(Some(PreMigrationCopy { path: copy, before })),
        Err(err) => {
            remove_with_companions(&copy);
            Err(err)
        }
    }
}

/// A transaction that appends chained rows.
struct Batch<'c> {
    tx: Transaction<'c>,
    next_seq: i64,
    /// Sequence of the first row of this transaction.
    batch: i64,
    prev: Hash,
}

/// Columns of a journal row; unset ones are NULL.
#[derive(Default)]
struct Entry<'a> {
    entry: &'static str,
    subject_id: Option<&'a str>,
    state: Option<&'a str>,
    step: Option<i64>,
    related_id: Option<&'a str>,
    path: Option<&'a str>,
    inode: Option<i64>,
    pid: Option<i64>,
    detail: Option<&'a str>,
    birth_ns: Option<i64>,
}

impl Batch<'_> {
    fn append(
        &mut self,
        kind: RowKind,
        insert: impl FnOnce(&Connection, i64) -> rusqlite::Result<usize>,
    ) -> Result<i64> {
        let seq = self.next_seq;
        insert(&self.tx, seq)?;
        self.prev = chain::link(&self.tx, kind, seq, self.batch, &self.prev)?;
        self.next_seq += 1;
        Ok(seq)
    }

    fn journal(&mut self, e: &Entry<'_>, now_ms: i64) -> Result<i64> {
        self.append(RowKind::Journal, |tx, seq| {
            tx.execute(
                "INSERT INTO journal (seq, entry, subject_id, state, step, related_id, path,
                     inode, pid, detail, recorded_ms, birth_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    seq,
                    e.entry,
                    e.subject_id,
                    e.state,
                    e.step,
                    e.related_id,
                    e.path,
                    e.inode,
                    e.pid,
                    e.detail,
                    now_ms,
                    e.birth_ns
                ],
            )
        })
    }
}

fn advance(
    batch: &mut Batch<'_>,
    operation_id: &str,
    transition: OperationTransition<'_>,
    now_ms: i64,
) -> Result<()> {
    let (current, last_step) = operation_state(&batch.tx, operation_id)?
        .ok_or_else(|| ProfileError::InvalidWrite("unknown operation".into()))?;
    let next = transition.state();
    if !current.can_become(next) {
        return Err(invalid_transition(current.as_str(), next.as_str()));
    }
    let mut entry = Entry {
        entry: "operation-state",
        subject_id: Some(operation_id),
        state: Some(next.as_str()),
        ..Entry::default()
    };
    match transition {
        OperationTransition::PriorSnapshot { snapshot_id } => {
            if snapshot_state(&batch.tx, snapshot_id)? != Some(SnapshotState::Complete) {
                return Err(ProfileError::InvalidWrite(
                    "the prior snapshot is not complete".into(),
                ));
            }
            entry.related_id = Some(snapshot_id);
        }
        OperationTransition::Applying { step } => {
            if last_step.is_some_and(|last| i64::from(step) <= last) {
                return Err(ProfileError::InvalidWrite(
                    "applier steps must increase".into(),
                ));
            }
            entry.step = Some(i64::from(step));
        }
        OperationTransition::Rejected { reason } | OperationTransition::Aborted { reason } => {
            entry.detail = Some(reason);
        }
        OperationTransition::Ready
        | OperationTransition::Finished
        | OperationTransition::Interrupted => {}
    }
    batch.journal(&entry, now_ms)?;
    Ok(())
}

fn notice(
    batch: &mut Batch<'_>,
    kind: NoticeKind,
    worktree: Option<&str>,
    operation_id: Option<&str>,
    detail: &str,
    now_ms: i64,
) -> Result<String> {
    let id = sqlite::new_uuid(&batch.tx)?;
    batch.append(RowKind::Notice, |tx, seq| {
        tx.execute(
            "INSERT INTO notices (notice_id, seq, kind, worktree, operation_id, detail,
                 recorded_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                seq,
                kind.as_str(),
                worktree,
                operation_id,
                detail,
                now_ms
            ],
        )
    })?;
    Ok(id)
}

/// Last state of a snapshot, or `None` if it is unknown.
fn snapshot_state(conn: &Connection, snapshot_id: &str) -> Result<Option<SnapshotState>> {
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM journal WHERE entry = 'snapshot-state' AND subject_id = ?1
             ORDER BY seq DESC LIMIT 1",
            params![snapshot_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(state.as_deref().map(SnapshotState::parse).transpose()?)
}

/// Last state of an operation and its last applier step.
fn operation_state(
    conn: &Connection,
    operation_id: &str,
) -> Result<Option<(OperationState, Option<i64>)>> {
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM journal WHERE entry = 'operation-state' AND subject_id = ?1
             ORDER BY seq DESC LIMIT 1",
            params![operation_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(state) = state else {
        return Ok(None);
    };
    let step: Option<i64> = conn.query_row(
        "SELECT MAX(step) FROM journal WHERE entry = 'operation-state' AND subject_id = ?1",
        params![operation_id],
        |row| row.get(0),
    )?;
    Ok(Some((OperationState::parse(&state)?, step)))
}

fn require_operation(conn: &Connection, operation_id: &str) -> Result<()> {
    let known: Option<i64> = conn
        .query_row(
            "SELECT seq FROM operations WHERE operation_id = ?1",
            params![operation_id],
            |row| row.get(0),
        )
        .optional()?;
    known
        .map(|_| ())
        .ok_or_else(|| ProfileError::InvalidWrite("unknown operation".into()))
}

fn invalid_transition(from: &str, to: &str) -> ProfileError {
    ProfileError::InvalidWrite(["transition ", from, " -> ", to, " is not allowed"].concat())
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|err| ProfileError::Io(io::Error::new(io::ErrorKind::InvalidData, err)))
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ProfileError::InvalidPath {
            path: path.to_path_buf(),
            reason: "not valid UTF-8".into(),
        })
}
