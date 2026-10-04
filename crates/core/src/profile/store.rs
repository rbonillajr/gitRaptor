//! Per-repo store: the entities of ADR-GRP-013 § 1, written in batches by
//! the single writer (ADR-GRP-005) and queried by session and worktree.
//!
//! The store does not interpret `metadata`, `evidence` or `refs`: the engine
//! puts metadata there (refs, commit ids, paths), never file contents,
//! prompts or diffs (NFR-03).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::error::{ProfileError, Result};

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            /// Stable text stored in the database.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            fn parse(text: &str) -> rusqlite::Result<Self> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(rusqlite::Error::InvalidColumnType(
                        0, stringify!($name).into(), rusqlite::types::Type::Text)),
                }
            }
        }
    };
}

text_enum!(
    /// Which agent a session belongs to.
    AgentKind { ClaudeCode => "claude-code", Other => "other" }
);
text_enum!(
    /// How the initial attribution of a session came to be.
    Origin { Detected => "detected", Registered => "registered" }
);
text_enum!(
    /// Why a session ended. An ended session is never reopened (Q41).
    EndCause {
        ProcessGone => "process-gone",
        EndedDuringGap => "ended-during-gap",
        RegistrationWithdrawn => "registration-withdrawn",
    }
);
text_enum!(
    /// Type of an append-only attribution record.
    RecordKind {
        Register => "register",
        Confirm => "confirm",
        Correct => "correct",
        WithdrawCorrection => "withdraw-correction",
        WithdrawRegistration => "withdraw-registration",
    }
);
text_enum!(
    /// Who wrote an attribution record (BR-CONS-001).
    Author { Developer => "developer", Agent => "agent" }
);
text_enum!(
    /// Why the engine did not observe an interval.
    GapCause {
        MachineOff => "machine-off",
        DaemonDown => "daemon-down",
        DaemonDownDuringSession => "daemon-down-during-session",
        DaemonStopped => "daemon-stopped",
        RepoRetired => "repo-retired",
        GitUnavailable => "git-unavailable",
        ProfileLost => "profile-lost",
        StoreCorrupt => "store-corrupt",
    }
);

/// An agent: Claude Code, or "other agent" with its declared name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub kind: AgentKind,
    /// Declared name; untrusted text (SEC-12).
    pub name: Option<String>,
}

/// Observation time in UTC plus the local offset (ADR-GRP-013 § 4). The
/// caller computes both; order comes from `seq`, never from the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub utc_ms: i64,
    pub offset_s: i32,
}

/// A new event to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEvent {
    pub worktree: PathBuf,
    pub kind: String,
    /// Metadata only (refs, commit ids, affected paths), never content.
    pub metadata: String,
    pub observed: Timestamp,
    pub session_id: Option<String>,
    /// Signals of ADR-GRP-012 that back the attribution.
    pub evidence: Option<String>,
    pub gap_id: Option<String>,
}

/// Last known state of a worktree, base of reconciliation (ADR-GRP-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownState {
    pub head: Option<String>,
    pub refs: String,
    pub operation: Option<String>,
    pub dirty_fingerprint: Option<String>,
    pub updated_ms: i64,
}

/// One operation of a write batch. Worktrees are referenced by canonical
/// path; sessions and gaps by ids the caller generates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    UpsertWorktree {
        path: PathBuf,
        admin_name: Option<String>,
        seen_ms: i64,
    },
    MarkWorktreeGone {
        path: PathBuf,
        gone_ms: i64,
    },
    StartSession {
        session_id: String,
        worktree: PathBuf,
        agent: Agent,
        origin: Origin,
        detection_key: Option<String>,
        started_ms: i64,
    },
    EndSession {
        session_id: String,
        /// `None` when the end time is unknown (ended during a gap).
        ended_ms: Option<i64>,
        cause: EndCause,
    },
    AppendAttribution {
        session_id: String,
        kind: RecordKind,
        agent: Agent,
        author: Author,
        recorded_ms: i64,
    },
    AppendEvent(NewEvent),
    OpenGap {
        gap_id: String,
        started_ms: i64,
        cause: GapCause,
        /// Client that requested it, when a command caused it (SEC-13).
        requested_by: Option<String>,
    },
    CloseGap {
        gap_id: String,
        ended_ms: i64,
    },
    SetLastKnownState {
        worktree: PathBuf,
        state: KnownState,
    },
    SetObservedUntil {
        ms: i64,
    },
}

/// Sequences assigned by a committed batch, in operation order: one per
/// `AppendEvent` and per `AppendAttribution`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchResult {
    pub seqs: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub admin_name: Option<String>,
    pub first_seen_ms: i64,
    pub gone_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub session_id: String,
    pub worktree: PathBuf,
    pub agent: Agent,
    pub initial_origin: Origin,
    pub detection_key: Option<String>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub end_cause: Option<EndCause>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionRecord {
    pub effective_seq: i64,
    pub session_id: String,
    pub kind: RecordKind,
    pub agent: Agent,
    pub author: Author,
    pub recorded_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub seq: i64,
    pub worktree: PathBuf,
    pub kind: String,
    pub metadata: String,
    pub observed: Timestamp,
    pub session_id: Option<String>,
    pub evidence: Option<String>,
    pub gap_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub gap_id: String,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub cause: GapCause,
    pub requested_by: Option<String>,
}

/// An open per-repo store. Writing needs `&mut self`: the daemon is the
/// only writer (ADR-GRP-005).
pub struct RepoStore {
    conn: Connection,
    next_seq: i64,
}

/// Event query with a literal tail, assembled at compile time so the SQL
/// stays a single string literal.
macro_rules! event_select {
    ($tail:literal) => {
        concat!(
            "SELECT e.seq, w.canonical_path, e.kind, e.metadata, e.observed_utc_ms,
                 e.utc_offset_s, e.session_id, e.evidence, e.gap_id
             FROM events e JOIN worktrees w ON w.id = e.worktree_id ",
            $tail
        )
    };
}

impl RepoStore {
    pub(crate) fn from_conn(conn: Connection, repo_id: &str, common_dir: &Path) -> Result<Self> {
        conn.execute(
            "INSERT OR IGNORE INTO store_meta (key, value) VALUES ('repo_id', ?1)",
            params![repo_id],
        )?;
        // Lets a lost index be rebuilt from the stores later.
        conn.execute(
            "INSERT OR IGNORE INTO store_meta (key, value) VALUES ('common_dir', ?1)",
            params![path_text(common_dir)?],
        )?;
        let next_seq: i64 = conn.query_row(
            "SELECT MAX(
                 COALESCE((SELECT MAX(seq) FROM events), 0),
                 COALESCE((SELECT MAX(effective_seq) FROM attribution_records), 0)) + 1",
            [],
            |row| row.get(0),
        )?;
        Ok(Self { conn, next_seq })
    }

    /// Applies every operation in one transaction. If any fails, nothing is
    /// written and no sequence is consumed.
    pub fn write_batch(&mut self, ops: &[WriteOp]) -> Result<BatchResult> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut next_seq = self.next_seq;
        let mut result = BatchResult::default();
        for op in ops {
            apply(&tx, op, &mut next_seq, &mut result)?;
        }
        tx.commit()?;
        self.next_seq = next_seq;
        Ok(result)
    }

    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        self.collect(
            "SELECT canonical_path, admin_name, first_seen_ms, gone_ms FROM worktrees ORDER BY id",
            [],
            |row| {
                Ok(Worktree {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    admin_name: row.get(1)?,
                    first_seen_ms: row.get(2)?,
                    gone_ms: row.get(3)?,
                })
            },
        )
    }

    pub fn session(&self, session_id: &str) -> Result<Option<Session>> {
        Ok(self
            .conn
            .query_row(
                "SELECT s.session_id, w.canonical_path, s.agent_kind, s.agent_name,
                     s.initial_origin, s.detection_key, s.started_ms, s.ended_ms, s.end_cause
                 FROM sessions s JOIN worktrees w ON w.id = s.worktree_id
                 WHERE s.session_id = ?1",
                params![session_id],
                session_from_row,
            )
            .optional()?)
    }

    /// Sessions of a worktree, oldest first.
    pub fn sessions_for_worktree(&self, worktree: &Path) -> Result<Vec<Session>> {
        self.collect(
            "SELECT s.session_id, w.canonical_path, s.agent_kind, s.agent_name,
                 s.initial_origin, s.detection_key, s.started_ms, s.ended_ms, s.end_cause
             FROM sessions s JOIN worktrees w ON w.id = s.worktree_id
             WHERE w.canonical_path = ?1 ORDER BY s.started_ms, s.session_id",
            params![path_text(worktree)?],
            session_from_row,
        )
    }

    /// Attribution records of a session, in effective order.
    pub fn attribution_records(&self, session_id: &str) -> Result<Vec<AttributionRecord>> {
        self.collect(
            "SELECT effective_seq, session_id, kind, agent_kind, agent_name, author, recorded_ms
             FROM attribution_records WHERE session_id = ?1 ORDER BY effective_seq",
            params![session_id],
            |row| {
                Ok(AttributionRecord {
                    effective_seq: row.get(0)?,
                    session_id: row.get(1)?,
                    kind: RecordKind::parse(&row.get::<_, String>(2)?)?,
                    agent: Agent {
                        kind: AgentKind::parse(&row.get::<_, String>(3)?)?,
                        name: row.get(4)?,
                    },
                    author: Author::parse(&row.get::<_, String>(5)?)?,
                    recorded_ms: row.get(6)?,
                })
            },
        )
    }

    /// Events pointing to a session, in sequence order (index by session).
    pub fn events_for_session(&self, session_id: &str) -> Result<Vec<Event>> {
        self.collect(
            event_select!("WHERE e.session_id = ?1 ORDER BY e.seq"),
            params![session_id],
            event_from_row,
        )
    }

    /// Events of a worktree, in sequence order (index by worktree).
    pub fn events_for_worktree(&self, worktree: &Path) -> Result<Vec<Event>> {
        self.collect(
            event_select!("WHERE w.canonical_path = ?1 ORDER BY e.seq"),
            params![path_text(worktree)?],
            event_from_row,
        )
    }

    /// All events with `from <= seq <= to`.
    pub fn events_in_range(&self, from: i64, to: i64) -> Result<Vec<Event>> {
        self.collect(
            event_select!("WHERE e.seq BETWEEN ?1 AND ?2 ORDER BY e.seq"),
            params![from, to],
            event_from_row,
        )
    }

    pub fn gaps(&self) -> Result<Vec<Gap>> {
        self.collect(
            "SELECT gap_id, started_ms, ended_ms, cause, requested_by FROM gaps
             ORDER BY started_ms, gap_id",
            [],
            |row| {
                Ok(Gap {
                    gap_id: row.get(0)?,
                    started_ms: row.get(1)?,
                    ended_ms: row.get(2)?,
                    cause: GapCause::parse(&row.get::<_, String>(3)?)?,
                    requested_by: row.get(4)?,
                })
            },
        )
    }

    pub fn last_known_state(&self, worktree: &Path) -> Result<Option<KnownState>> {
        Ok(self
            .conn
            .query_row(
                "SELECT k.head, k.refs, k.operation, k.dirty_fingerprint, k.updated_ms
                 FROM last_known_state k JOIN worktrees w ON w.id = k.worktree_id
                 WHERE w.canonical_path = ?1",
                params![path_text(worktree)?],
                |row| {
                    Ok(KnownState {
                        head: row.get(0)?,
                        refs: row.get(1)?,
                        operation: row.get(2)?,
                        dirty_fingerprint: row.get(3)?,
                        updated_ms: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Moment up to which the repo was observed, if ever recorded.
    pub fn observed_until(&self) -> Result<Option<i64>> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'observed_until'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    /// Next sequence the store will assign.
    pub fn next_seq(&self) -> i64 {
        self.next_seq
    }

    fn collect<T, P: rusqlite::Params>(
        &self,
        sql: &'static str,
        params: P,
        map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Vec<T>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, map)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn apply(
    tx: &Transaction<'_>,
    op: &WriteOp,
    next_seq: &mut i64,
    out: &mut BatchResult,
) -> Result<()> {
    match op {
        WriteOp::UpsertWorktree {
            path,
            admin_name,
            seen_ms,
        } => {
            tx.execute(
                "INSERT INTO worktrees (canonical_path, admin_name, first_seen_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT (canonical_path)
                 DO UPDATE SET admin_name = excluded.admin_name, gone_ms = NULL",
                params![path_text(path)?, admin_name, seen_ms],
            )?;
        }
        WriteOp::MarkWorktreeGone { path, gone_ms } => {
            let id = worktree_id(tx, path)?;
            tx.execute(
                "UPDATE worktrees SET gone_ms = ?2 WHERE id = ?1",
                params![id, gone_ms],
            )?;
        }
        WriteOp::StartSession {
            session_id,
            worktree,
            agent,
            origin,
            detection_key,
            started_ms,
        } => {
            let id = worktree_id(tx, worktree)?;
            tx.execute(
                "INSERT INTO sessions (session_id, worktree_id, agent_kind, agent_name,
                     initial_origin, detection_key, started_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    session_id,
                    id,
                    agent.kind.as_str(),
                    agent.name,
                    origin.as_str(),
                    detection_key,
                    started_ms
                ],
            )?;
        }
        WriteOp::EndSession {
            session_id,
            ended_ms,
            cause,
        } => {
            let changed = tx.execute(
                "UPDATE sessions SET ended_ms = ?2, end_cause = ?3
                 WHERE session_id = ?1 AND end_cause IS NULL",
                params![session_id, ended_ms, cause.as_str()],
            )?;
            if changed == 0 {
                return Err(ProfileError::InvalidWrite(
                    ["session ", session_id, " is unknown or already ended"].concat(),
                ));
            }
        }
        WriteOp::AppendAttribution {
            session_id,
            kind,
            agent,
            author,
            recorded_ms,
        } => {
            let seq = take_seq(next_seq, out);
            tx.execute(
                "INSERT INTO attribution_records (effective_seq, session_id, kind, agent_kind,
                     agent_name, author, recorded_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    seq,
                    session_id,
                    kind.as_str(),
                    agent.kind.as_str(),
                    agent.name,
                    author.as_str(),
                    recorded_ms
                ],
            )?;
        }
        WriteOp::AppendEvent(event) => {
            let id = worktree_id(tx, &event.worktree)?;
            let seq = take_seq(next_seq, out);
            tx.execute(
                "INSERT INTO events (seq, worktree_id, kind, metadata, observed_utc_ms,
                     utc_offset_s, session_id, evidence, gap_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    seq,
                    id,
                    event.kind,
                    event.metadata,
                    event.observed.utc_ms,
                    event.observed.offset_s,
                    event.session_id,
                    event.evidence,
                    event.gap_id
                ],
            )?;
        }
        WriteOp::OpenGap {
            gap_id,
            started_ms,
            cause,
            requested_by,
        } => {
            tx.execute(
                "INSERT INTO gaps (gap_id, started_ms, cause, requested_by)
                 VALUES (?1, ?2, ?3, ?4)",
                params![gap_id, started_ms, cause.as_str(), requested_by],
            )?;
        }
        WriteOp::CloseGap { gap_id, ended_ms } => {
            let changed = tx.execute(
                "UPDATE gaps SET ended_ms = ?2 WHERE gap_id = ?1 AND ended_ms IS NULL",
                params![gap_id, ended_ms],
            )?;
            if changed == 0 {
                return Err(ProfileError::InvalidWrite(
                    ["gap ", gap_id, " is unknown or already closed"].concat(),
                ));
            }
        }
        WriteOp::SetLastKnownState { worktree, state } => {
            let id = worktree_id(tx, worktree)?;
            tx.execute(
                "INSERT INTO last_known_state (worktree_id, head, refs, operation,
                     dirty_fingerprint, updated_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (worktree_id) DO UPDATE SET head = excluded.head,
                     refs = excluded.refs, operation = excluded.operation,
                     dirty_fingerprint = excluded.dirty_fingerprint,
                     updated_ms = excluded.updated_ms",
                params![
                    id,
                    state.head,
                    state.refs,
                    state.operation,
                    state.dirty_fingerprint,
                    state.updated_ms
                ],
            )?;
        }
        WriteOp::SetObservedUntil { ms } => {
            tx.execute(
                "INSERT INTO store_meta (key, value) VALUES ('observed_until', ?1)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![ms.to_string()],
            )?;
        }
    }
    Ok(())
}

fn take_seq(next_seq: &mut i64, out: &mut BatchResult) -> i64 {
    let seq = *next_seq;
    *next_seq += 1;
    out.seqs.push(seq);
    seq
}

fn worktree_id(tx: &Transaction<'_>, path: &Path) -> Result<i64> {
    let text = path_text(path)?;
    tx.query_row(
        "SELECT id FROM worktrees WHERE canonical_path = ?1",
        params![text],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| ProfileError::InvalidWrite(["unknown worktree ", text].concat()))
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| ProfileError::InvalidPath {
        path: path.to_path_buf(),
        reason: "path is not valid UTF-8".into(),
    })
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<Session> {
    let end_cause: Option<String> = row.get(8)?;
    Ok(Session {
        session_id: row.get(0)?,
        worktree: PathBuf::from(row.get::<_, String>(1)?),
        agent: Agent {
            kind: AgentKind::parse(&row.get::<_, String>(2)?)?,
            name: row.get(3)?,
        },
        initial_origin: Origin::parse(&row.get::<_, String>(4)?)?,
        detection_key: row.get(5)?,
        started_ms: row.get(6)?,
        ended_ms: row.get(7)?,
        end_cause: end_cause.as_deref().map(EndCause::parse).transpose()?,
    })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        seq: row.get(0)?,
        worktree: PathBuf::from(row.get::<_, String>(1)?),
        kind: row.get(2)?,
        metadata: row.get(3)?,
        observed: Timestamp {
            utc_ms: row.get(4)?,
            offset_s: row.get(5)?,
        },
        session_id: row.get(6)?,
        evidence: row.get(7)?,
        gap_id: row.get(8)?,
    })
}
