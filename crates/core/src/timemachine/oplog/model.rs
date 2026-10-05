//! Records, states and transitions of the oplog (ADR-TMC-003 § 2 and § 3).

use serde::{Deserialize, Serialize};

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident {
        $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($(#[$vmeta])* $variant),+ }

        impl $name {
            /// Stable text stored in the oplog.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            pub(crate) fn parse(text: &str) -> rusqlite::Result<Self> {
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
    /// Coverage level of a snapshot (ADR-TMC-004).
    SnapshotLevel {
        GuaranteedPrior => "guaranteed-prior",
        Observation => "observation",
        HookPrior => "hook-prior",
    }
);

text_enum!(
    /// What kind of Time Machine operation a record is.
    OperationKind {
        Protected => "protected",
        Undo => "undo",
        Redo => "redo",
        Restore => "restore",
    }
);

text_enum!(
    /// Client channel the request arrived through.
    Channel { Cli => "cli", Tui => "tui", Mcp => "mcp", Hook => "hook" }
);

text_enum!(
    /// State of a snapshot, kept in the journal (ADR-TMC-003 § 2,
    /// ADR-TMC-007 § 4).
    SnapshotState {
        Pending => "pending",
        Complete => "complete",
        Discarded => "discarded",
        PurgeAnnounced => "purge-announced",
        PurgeIntent => "purge-intent",
        Purged => "purged",
        PurgeCancelled => "purge-cancelled",
    }
);

text_enum!(
    /// State of an operation, kept in the journal (ADR-TMC-003 § 3).
    OperationState {
        Intent => "intent",
        PriorSnapshot => "prior-snapshot",
        Ready => "ready",
        Applying => "applying",
        Finished => "finished",
        Rejected => "rejected",
        Aborted => "aborted",
        Interrupted => "interrupted",
    }
);

text_enum!(
    /// Why a notice is waiting for a client.
    NoticeKind { Interruption => "interruption", Purge => "purge" }
);

text_enum!(
    /// Why the hash chain does not hold at a row (SEC-TMC-09).
    BreakCause {
        /// The row's content no longer matches its hash, or the row is gone.
        RowAltered => "row-altered",
        /// The row does not point at the previous row: a row was removed or
        /// inserted in between.
        LinkBroken => "link-broken",
        /// A row exists outside the chain.
        Unchained => "unchained",
        /// The oplog has rows but the head kept outside it is gone.
        HeadMissing => "head-missing",
        /// The head names a row the oplog no longer has: rows were cut off.
        HeadAhead => "head-ahead",
        /// The head's hash differs from the row it names.
        HeadMismatch => "head-mismatch",
        /// Rows were appended after the head beyond the last batch: written
        /// outside the daemon.
        HeadBehind => "head-behind",
        /// The oplog was corrupt and set aside; this one starts empty and
        /// nothing before it can be trusted.
        Quarantined => "quarantined",
    }
);

impl SnapshotState {
    /// Whether the transition `self -> next` is allowed.
    pub fn can_become(self, next: Self) -> bool {
        use SnapshotState::*;
        matches!(
            (self, next),
            (Pending, Complete | Discarded)
                | (Complete | PurgeCancelled, PurgeAnnounced | PurgeIntent)
                | (PurgeAnnounced, PurgeIntent | PurgeCancelled)
                | (PurgeIntent, Purged | PurgeCancelled)
        )
    }

    /// States in which a snapshot, if its ref exists, is a point of the
    /// timeline that can be restored (ADR-TMC-003 § 3).
    pub fn is_available(self) -> bool {
        matches!(
            self,
            Self::Complete | Self::PurgeAnnounced | Self::PurgeCancelled
        )
    }
}

impl OperationState {
    /// Terminal states take no further transition.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Finished | Self::Rejected | Self::Aborted | Self::Interrupted
        )
    }

    /// Whether the transition `self -> next` is allowed. `applying` may
    /// repeat, once per applier step. `ready -> rejected` is the applier
    /// finding a precondition failed under its locks, before any change
    /// (TS-TMC-003).
    pub fn can_become(self, next: Self) -> bool {
        use OperationState::*;
        matches!(
            (self, next),
            (Intent, PriorSnapshot | Rejected | Aborted)
                | (PriorSnapshot, Ready | Rejected | Aborted)
                | (Ready, Applying | Aborted | Rejected)
                | (Applying, Applying | Finished | Interrupted)
        )
    }
}

/// Who asked for an operation, frozen when it is recorded (D-TMC-18).
/// There is no "human" variant: an unattributed request stays so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "variant")]
pub enum Requester {
    Agent {
        /// Agent name as resolved at request time; untrusted text.
        name: String,
        origin: RequesterOrigin,
        session_id: String,
    },
    Unattributed,
}

impl Requester {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Agent { session_id, .. } => Some(session_id),
            Self::Unattributed => None,
        }
    }
}

/// How the requesting agent was known at request time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequesterOrigin {
    Detected,
    Registered,
}

/// Something the timeline can undo: an oplog operation or a raw Git event
/// of the engine, referenced by its sequence and never copied.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpRef {
    Oplog(String),
    GitEvent(i64),
}

/// Worktrees and refs an operation acts on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// Canonical worktree paths, as the engine stores them.
    pub worktrees: Vec<String>,
    pub refs: Vec<String>,
}

/// What an operation acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    /// A protected operation acts on its scope only.
    None,
    /// An undo: the operations or events it undoes.
    Undo(Vec<OpRef>),
    /// A redo: the undo it redoes.
    Redo(String),
    /// A restore: the destination snapshot.
    Snapshot(String),
}

/// A new operation, written once at intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOperation {
    pub kind: OperationKind,
    /// Subtype of a protected operation (e.g. `reset-hard`).
    pub subtype: Option<String>,
    pub scope: Scope,
    pub requester: Requester,
    pub channel: Channel,
    /// Whether an interactive confirmation was given.
    pub confirmed: bool,
    pub target: Target,
    /// Warnings shown with the plan (already pushed, exclusions, overlap).
    pub warnings: Vec<String>,
    /// Engine sequence (ADR-GRP-013) when the intent was recorded, to place
    /// the operation among raw Git events without relying on clocks.
    pub engine_mark: i64,
}

/// An operation as recorded. Immutable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
    pub operation_id: String,
    pub seq: i64,
    pub kind: OperationKind,
    pub subtype: Option<String>,
    pub scope: Scope,
    pub requester: Requester,
    pub channel: Channel,
    pub confirmed: bool,
    pub target: Target,
    pub warnings: Vec<String>,
    pub engine_mark: i64,
    pub recorded_ms: i64,
}

/// An operation with its current state, derived from the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationView {
    pub record: OperationRecord,
    pub state: OperationState,
    /// Last applier step annotated while `applying`.
    pub step: Option<u32>,
    /// Snapshot taken before the operation, once recorded.
    pub prior_snapshot: Option<String>,
    /// A row of this operation is at a declared break of the chain.
    pub tampered: bool,
}

/// A new snapshot, written when its capture starts (state `pending`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSnapshot {
    pub level: SnapshotLevel,
    pub worktrees: Vec<String>,
    /// Engine sequence (ADR-GRP-013) up to which the read state reaches.
    pub engine_mark: Option<i64>,
    /// Operation that caused it, for a guaranteed prior.
    pub cause_operation: Option<String>,
    /// Engine event that caused it, for an observation or hook prior.
    pub cause_event_seq: Option<i64>,
}

/// A snapshot as recorded. Immutable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub snapshot_id: String,
    pub seq: i64,
    pub level: SnapshotLevel,
    pub worktrees: Vec<String>,
    /// Ref of the snapshot in the store (ADR-TMC-001).
    pub store_ref: String,
    pub engine_mark: Option<i64>,
    pub cause_operation: Option<String>,
    pub cause_event_seq: Option<i64>,
    pub recorded_ms: i64,
}

/// What is known once a capture completes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteInfo {
    /// Bytes this snapshot adds to the store.
    pub unique_size_bytes: u64,
    /// Paths left out (size, submodules) with the reason.
    pub exclusions: Vec<Exclusion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusion {
    pub path: String,
    pub reason: String,
}

/// A snapshot with its current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotView {
    pub record: SnapshotRecord,
    pub state: SnapshotState,
    pub complete: Option<CompleteInfo>,
    pub tampered: bool,
}

/// A notice waiting for a client (ADR-TMC-003 § 6.5, ADR-TMC-007 § 4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub notice_id: String,
    pub seq: i64,
    pub kind: NoticeKind,
    /// Worktree it is for; `None` for the whole repo.
    pub worktree: Option<String>,
    pub operation_id: Option<String>,
    /// Structured detail (state, step, count, period, size).
    pub detail: serde_json::Value,
    pub recorded_ms: i64,
    /// First time a client received it.
    pub first_delivered_ms: Option<i64>,
}

/// A break of the hash chain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainBreak {
    pub seq: i64,
    pub cause: BreakCause,
}

/// One journal row, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub seq: i64,
    pub entry: String,
    pub subject_id: Option<String>,
    pub state: Option<String>,
    pub step: Option<i64>,
    pub related_id: Option<String>,
    pub path: Option<String>,
    pub inode: Option<i64>,
    pub pid: Option<i64>,
    pub detail: Option<String>,
    pub recorded_ms: i64,
    /// Birth time of an annotated lock, in ns since the epoch.
    pub birth_ns: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_happy_path_and_exits() {
        use OperationState::*;
        for (from, to) in [
            (Intent, PriorSnapshot),
            (PriorSnapshot, Ready),
            (Ready, Applying),
            (Applying, Applying),
            (Applying, Finished),
            (Intent, Rejected),
            (PriorSnapshot, Aborted),
            (PriorSnapshot, Rejected),
            (Applying, Interrupted),
            (Ready, Rejected),
        ] {
            assert!(from.can_become(to), "{from:?} -> {to:?}");
        }
        for (from, to) in [
            (Intent, Applying),
            (Finished, Applying),
            (Applying, Aborted),
        ] {
            assert!(!from.can_become(to), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn snapshot_purge_states() {
        use SnapshotState::*;
        assert!(Pending.can_become(Complete));
        assert!(Complete.can_become(PurgeIntent));
        assert!(PurgeIntent.can_become(Purged));
        assert!(!Purged.can_become(Complete));
        assert!(!Discarded.can_become(Complete));
        assert!(PurgeCancelled.is_available());
        assert!(!PurgeIntent.is_available());
    }

    #[test]
    fn requester_has_no_human_variant_and_round_trips() {
        let r = Requester::Agent {
            name: "claude".into(),
            origin: RequesterOrigin::Registered,
            session_id: "s1".into(),
        };
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Requester>(&text).unwrap(), r);
        assert!(serde_json::from_str::<Requester>(r#"{"variant":"human"}"#).is_err());
    }
}
