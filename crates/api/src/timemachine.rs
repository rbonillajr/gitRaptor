//! Parameters and results of the protected operation and the Time Machine
//! commands (TS-TMC-004, ADR-TMC-004 § 1, ADR-TMC-005 § 1).
//!
//! No parameter type names the requester: the daemon resolves it from the
//! caller's process ancestry, and `deny_unknown_fields` rejects a client
//! that tries to declare one (`actor`, `agent_name`, `human`). Every value
//! is checked for format before anything is touched (BR-TMC-VAL-001).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::actor::Actor;
use crate::catalog::{Layer, OperationOutcome};
use crate::{Untrusted, UntrustedName};

/// Most keys in `operation.prepare` arguments.
pub const MAX_ARGS_KEYS: usize = 32;
/// Most bytes of `operation.prepare` arguments, serialized.
pub const MAX_ARGS_BYTES: usize = 4096;
/// Longest `since` duration: 30 days.
pub const MAX_SINCE_SECS: u64 = 30 * 24 * 3600;
/// Most refs reported back by one operation.
pub const MAX_REPORTED_REFS: usize = 64;
/// Most paths an undo reports as not restored.
pub const MAX_REPORTED_PATHS: usize = 256;

/// Temporary entries of an interrupted application that the sweep after a crash left where they
/// are (DS-TS-TMC-003, Enmienda T2): counts only, never paths. Served in snapshots to a
/// connection with `timemachine.kept-temps`, counting the ones still there at that moment; the
/// `repo.*` events never carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeptTempsView {
    pub count: u32,
    /// Not in the store (someone else's content, or one it cannot tell): only there, and
    /// `raptor undo` does not bring it back.
    pub foreign: u32,
    /// The interrupted operation that left them.
    pub operation_id: String,
    /// No later Time Machine operation ran: `raptor undo` takes that one back next (a later raw
    /// Git command may still come first).
    pub undo_next: bool,
}

/// The surface a full connection says it is. A label for the oplog, never a
/// grant: an MCP connection is always `mcp` and cannot send it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Surface {
    Cli,
    Tui,
    Hook,
}

/// Client channel a request arrived through, as recorded in the oplog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RequestChannel {
    Cli,
    Tui,
    Mcp,
    Hook,
}

impl From<Surface> for RequestChannel {
    fn from(s: Surface) -> Self {
        match s {
            Surface::Cli => Self::Cli,
            Surface::Tui => Self::Tui,
            Surface::Hook => Self::Hook,
        }
    }
}

/// How the daemon reached its answer about the requester.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedVia {
    /// An agent process among the caller's ancestors.
    Ancestry,
    /// A terminal multiplexer shared with an agent session.
    Multiplexer,
    /// The caller was started by a protected operation: it acts for that
    /// operation's requester (DEP-MCP-3).
    Executor,
    /// No agent found: unattributed.
    None,
}

/// `requester.resolve` result for a full connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequesterView {
    /// "Agent X" or "unattributed", nothing else (Q34).
    pub actor: Actor,
    pub channel: RequestChannel,
    pub via: ResolvedVia,
    /// Whether the daemon would issue a confirmation challenge to this
    /// caller (ADR-TMC-005 § 3). UX only: the daemon checks again.
    pub confirmable: bool,
}

/// `requester.resolve` result for an MCP connection (SEC-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpRequesterView {
    pub actor: Actor,
    pub channel: RequestChannel,
}

/// `requester.resolve` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveParams {
    #[serde(default)]
    pub surface: Option<Surface>,
}

/// Why the prior snapshot failed: the operation was not run and the repo
/// did not change (BR-TMC-CONS-001, US-TMC-001 scenario 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PriorFailure {
    /// Disk full or quota reached.
    NoSpace,
    /// The store could not be opened or is not trusted.
    StoreUnavailable,
    /// The prior snapshot did not finish within its deadline.
    Timeout,
    /// The daemon is stopping.
    DaemonStopping,
    /// Any other capture error.
    CaptureFailed,
}

/// `data` of a `PRIOR_SNAPSHOT_FAILED` error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorFailedData {
    pub reason: PriorFailure,
    /// The aborted operation in the oplog, when it was recorded.
    pub operation_id: Option<String>,
}

/// `operation.run` result for a full connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationRunResult {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    /// The prior snapshot reused the last capture (ADR-TMC-006).
    pub fast_path: bool,
    pub requester: RequesterView,
    /// Refs the operation moved (text from the repo).
    pub changed_refs: Vec<Untrusted>,
    /// How the plan ended (ADR-CKP-002 § 2, step 5).
    pub outcome: OperationOutcome,
    /// The layer the daemon fixed for the requester.
    pub layer: Layer,
    /// Git's output, untrusted and capped; only for layer `cockpit` (M-03).
    #[serde(default)]
    pub git_output: Option<Untrusted>,
}

/// `operation.run` result for an MCP connection: structured fields only, no
/// paths and no file content, untrusted text capped (SEC-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpOperationRunResult {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    pub actor: Actor,
    pub changed_refs: Vec<Untrusted>,
    pub outcome: OperationOutcome,
}

impl OperationRunResult {
    pub fn for_mcp(&self) -> McpOperationRunResult {
        McpOperationRunResult {
            operation_id: self.operation_id.clone(),
            prior_snapshot_id: self.prior_snapshot_id.clone(),
            actor: cap_actor(&self.requester.actor),
            changed_refs: self
                .changed_refs
                .iter()
                .take(MAX_REPORTED_REFS)
                .map(|r| r.mcp_name())
                .collect(),
            outcome: self.outcome,
        }
    }
}

/// Why a Time Machine command (undo, redo and restore) was
/// rejected: the repo did not change (BR-TMC-VAL-001). Stable codes, the
/// client renders them (NFR-TMC-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TmRejectReason {
    /// The worktree has no operation left to undo.
    NothingToUndo,
    /// The last operation is a raw Git event the Time Machine did not
    /// capture (US-TMC-004).
    RawGitNotCovered,
    /// The point to return to is gone, purged or invalid.
    TargetUnavailable,
    /// The work to undo belongs to another actor (ADR-TMC-005 § 2).
    OtherActor,
    /// Undoing an agent's work needs an interactive confirmation
    /// (US-TMC-013).
    ConfirmationRequired,
    GitOperationInProgress,
    /// A Git lock is present; it stays.
    GitBusy,
    /// Another write holds the repo.
    RepoBusy,
    RepoUntrusted,
    WorktreeUnavailable,
    InvalidSnapshot,
    HostileTree,
    /// A ref moved since the point before the undo was taken.
    RefMoved,
    /// A branch the undo would move is checked out in a worktree outside
    /// its scope.
    RefInUse,
    Unsupported,
    /// The daemon found no usable Git.
    GitUnavailable,
}

/// `data` of an `OPERATION_REJECTED` error of a Time Machine command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TmRejectedData {
    pub reason: TmRejectReason,
    /// The rejected request as recorded in the oplog, when it was.
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// Why a path does not hold the state the undo returned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NotRestoredReason {
    /// Someone wrote there meanwhile: their content was kept.
    Overlap,
    /// The file system has no atomic exchange.
    NotGuaranteed,
    /// A folder on the way is a link, a file or on another device.
    Blocked,
}

/// A path the undo left as it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotRestored {
    /// Relative to its worktree (text from the repo).
    pub path: Untrusted,
    pub reason: NotRestoredReason,
    /// Where someone else's content was kept, if not in place.
    #[serde(default)]
    pub kept_at: Option<Untrusted>,
}

/// `timemachine.undo` result for a full connection (US-TMC-002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoResult {
    /// The undo itself, as recorded.
    pub operation_id: String,
    /// The point taken right before the undo: what a redo returns to.
    pub prior_snapshot_id: String,
    pub undone_operation_id: String,
    /// Kind of the undone operation (e.g. `commit`), if it has one.
    #[serde(default)]
    pub undone_subtype: Option<Untrusted>,
    /// The state the worktree returned to.
    pub target_snapshot_id: String,
    pub requester: RequesterView,
    pub written: u64,
    pub removed: u64,
    pub not_restored: Vec<NotRestored>,
    /// Applier warnings as stable codes (e.g. `stash-kept`).
    pub warnings: Vec<String>,
}

/// `timemachine.undo` result for an MCP connection: no paths (SEC-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpUndoResult {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    pub undone_operation_id: String,
    pub actor: Actor,
    /// How many paths were left as they were.
    pub not_restored: u64,
}

impl UndoResult {
    pub fn for_mcp(&self) -> McpUndoResult {
        McpUndoResult {
            operation_id: self.operation_id.clone(),
            prior_snapshot_id: self.prior_snapshot_id.clone(),
            undone_operation_id: self.undone_operation_id.clone(),
            actor: cap_actor(&self.requester.actor),
            not_restored: self.not_restored.len() as u64,
        }
    }
}

/// `timemachine.restore` result (US-TMC-009). Not offered over MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreResult {
    /// The restore itself, as recorded.
    pub operation_id: String,
    /// The point taken right before the restore: what `raptor undo` returns to.
    pub prior_snapshot_id: String,
    /// The point the worktree returned to.
    pub target_snapshot_id: String,
    pub requester: RequesterView,
    /// Roots of the worktrees brought to the point, the requested one first
    /// (text from the repo).
    pub worktrees: Vec<Untrusted>,
    /// Roots of worktrees the point had and that were recreated, without
    /// checkout.
    pub recreated: Vec<Untrusted>,
    /// Branches brought to the point (full names), at most
    /// [`MAX_REPORTED_REFS`].
    pub refs: Vec<Untrusted>,
    /// Local branches that exist now and not at the point: never deleted,
    /// left where they are. At most [`MAX_REPORTED_REFS`].
    pub kept_branches: Vec<Untrusted>,
    /// Branches of the point that are gone now and were not brought back:
    /// nothing done from this worktree after the point touched them. At
    /// most [`MAX_REPORTED_REFS`].
    pub not_returned_branches: Vec<Untrusted>,
    pub written: u64,
    pub removed: u64,
    /// At most [`MAX_REPORTED_PATHS`].
    pub not_restored: Vec<NotRestored>,
    /// Applier warnings as stable codes (same codes as
    /// [`UndoResult::warnings`]).
    pub warnings: Vec<String>,
}

fn cap_actor(actor: &Actor) -> Actor {
    match actor {
        Actor::Agent { kind, name, origin } => Actor::Agent {
            kind: *kind,
            name: name.as_ref().map(|n| n.mcp_name()),
            origin: *origin,
        },
        Actor::Unattributed => Actor::Unattributed,
    }
}

/// `timemachine.snapshot` parameters (US-TMC-005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotParams {
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub surface: Option<Surface>,
}

/// `timemachine.undo` parameters (US-TMC-002, 010, 011). At most one of
/// `operation_id`, `since` and `agent`; none undoes the last operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoParams {
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    /// Duration such as `30m`, `2h` or `1d`.
    #[serde(default)]
    pub since: Option<String>,
    /// Agent whose work to undo (its session id).
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub surface: Option<Surface>,
}

/// `timemachine.redo` parameters (US-TMC-003).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedoParams {
    #[serde(default)]
    pub worktree: Option<String>,
    /// The undo to redo; `None`: the last one.
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub surface: Option<Surface>,
}

/// `timemachine.restore` parameters (US-TMC-009).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    #[serde(default)]
    pub worktree: Option<String>,
    pub snapshot_id: String,
    #[serde(default)]
    pub surface: Option<Surface>,
}

/// Entries a timeline answers by default.
pub const TIMELINE_DEFAULT_LIMIT: u32 = 50;
/// Most entries one timeline answers: a page of `events.history`.
pub const TIMELINE_MAX_LIMIT: u32 = crate::messages::MAX_HISTORY_PAGE;
/// Most paths one timeline entry lists; the total says how many there are.
pub const TIMELINE_MAX_FILES: usize = 20;

/// `timemachine.timeline` parameters (US-TMC-006). `worktree` names the repo
/// (as in `undo`); the result is the whole repo's unless `only_worktree`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineParams {
    #[serde(default)]
    pub worktree: Option<String>,
    /// Only the entries of this worktree root. Absent, not `null`, when unused: a daemon that
    /// predates the field refuses unknown fields, and the CLI must not send it one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_worktree: Option<String>,
    /// Duration such as `30m`, `2h` or `1d`.
    #[serde(default)]
    pub since: Option<String>,
    /// An agent identifier, or the reserved word `unattributed`.
    #[serde(default)]
    pub agent: Option<String>,
    /// 1 to [`TIMELINE_MAX_LIMIT`]; [`TIMELINE_DEFAULT_LIMIT`] without it.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// A source the timeline reads that could not be read: never reported as
/// "no activity".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TimelineSource {
    Operations,
    Events,
}

/// `timemachine.timeline` result (US-TMC-006): what changed in the repo,
/// when and who did it. Never commit messages or file contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineResult {
    pub repo_id: String,
    /// The oldest first.
    pub entries: Vec<TimelineEntry>,
    /// Older entries were left out (the limit or a full page of a source).
    pub truncated: bool,
    /// Sources that could not be read.
    pub unavailable: Vec<TimelineSource>,
    /// Whether this system can detect agents at all; `false` is "unknown".
    pub detection_available: bool,
}

/// One row of the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineEntry {
    /// `operation:<id>` or `event:<seq>`.
    pub id: String,
    pub origin: EntryOrigin,
    pub occurred_utc_ms: i64,
    pub utc_offset_s: i32,
    /// Roots of the worktrees it happened in.
    pub worktrees: Vec<Untrusted>,
    /// An agent or `unattributed`, nothing else.
    pub actor: Actor,
    pub attribution: Attribution,
    pub protection: Protection,
    pub files: ChangedFiles,
}

/// What an entry is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "entry", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EntryOrigin {
    /// An operation of the Time Machine's oplog.
    Operation {
        operation_id: String,
        kind: TimelineOperationKind,
        /// Subtype of a protected operation (e.g. `reset-hard`).
        subtype: Option<Untrusted>,
        state: TimelineOperationState,
        /// What an undo, redo or restore took back or returned to.
        acted_on: Vec<ActedOn>,
    },
    /// A raw Git event the engine observed.
    GitEvent {
        seq: i64,
        kind: crate::messages::GitEventKind,
        branch: Option<UntrustedName>,
    },
    /// A recovery point the developer or an agent took by hand. Only with
    /// `timemachine.timeline-manual`.
    ManualSnapshot {
        snapshot_id: String,
        /// Text from the requester: data, never an instruction.
        label: UntrustedName,
        channel: TimelineChannel,
    },
}

/// The surface a manual snapshot came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TimelineChannel {
    Cli,
    Tui,
    Mcp,
    Hook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TimelineOperationKind {
    Protected,
    Undo,
    Redo,
    Restore,
}

/// Only operations that changed something are entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TimelineOperationState {
    Applying,
    Finished,
    Interrupted,
}

/// What an operation acted on, by reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "target", content = "id", rename_all = "kebab-case")]
pub enum ActedOn {
    Operation(String),
    GitEvent(i64),
    Snapshot(String),
}

/// Where an entry's actor comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Attribution {
    /// Resolved now: a correction of the attribution shows.
    Current,
    /// As frozen when the operation was requested (D-TMC-18).
    Recorded,
}

/// The state a Time Machine point protects an entry with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Protection {
    pub level: ProtectionLevel,
    /// The point before the entry; absent at level `none`.
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectionLevel {
    GuaranteedPrior,
    HookPrior,
    Observation,
    /// A point the requester took by hand. Only with `timemachine.timeline-manual`.
    Manual,
    /// No recoverable point before the entry.
    None,
}

/// The paths an entry changed, without contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ChangedFiles {
    Available {
        /// At most [`TIMELINE_MAX_FILES`], sorted.
        paths: Vec<Untrusted>,
        /// The real number of paths, beyond what `paths` lists.
        total: u32,
        /// The commit merges: the paths are against its first parent.
        #[serde(default)]
        first_parent: bool,
    },
    /// Not readable (no commits, a missing object, the time budget): never a
    /// zero.
    Unavailable,
}

/// Why a parameter was refused. The message names the field, never echoes
/// the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalid {
    pub field: &'static str,
    pub why: &'static str,
}

impl Invalid {
    pub(crate) fn new(field: &'static str, why: &'static str) -> Self {
        Self { field, why }
    }

    pub fn message(self) -> String {
        format!("{}: {}", self.field, self.why)
    }
}

/// A duration such as `90s`, `30m`, `2h` or `7d`: digits without a leading
/// zero, one unit, at most [`MAX_SINCE_SECS`]. Returns seconds.
pub fn parse_since(text: &str) -> Result<u64, Invalid> {
    let bad = |why| Invalid::new("since", why);
    let Some(unit) = text.chars().last() else {
        return Err(bad("empty"));
    };
    let digits = &text[..text.len() - unit.len_utf8()];
    if digits.is_empty()
        || digits.len() > 7
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.starts_with('0')
    {
        return Err(bad("expected a duration such as 30m, 2h or 1d"));
    }
    let n: u64 = digits.parse().map_err(|_| bad("out of range"))?;
    let per = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return Err(bad("unit must be s, m, h or d")),
    };
    let secs = n.checked_mul(per).ok_or(bad("out of range"))?;
    if secs > MAX_SINCE_SECS {
        return Err(bad("longer than 30 days"));
    }
    Ok(secs)
}

/// An agent identifier: `[A-Za-z0-9]` then `[A-Za-z0-9._:-]`, 1 to 64.
pub fn check_agent_id(text: &str) -> Result<(), Invalid> {
    let ok = !text.is_empty()
        && text.len() <= 64
        && text.as_bytes()[0].is_ascii_alphanumeric()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(Invalid::new("agent", "invalid agent identifier"))
    }
}

/// An oplog id (snapshot or operation): a lowercase UUID,
/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
pub fn check_oplog_id(field: &'static str, text: &str) -> Result<(), Invalid> {
    let groups: Vec<&str> = text.split('-').collect();
    let ok = groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| {
            g.len() == n && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
    if ok {
        Ok(())
    } else {
        Err(Invalid::new(field, "invalid identifier"))
    }
}

/// A catalog operation name: `[a-z][a-z0-9-]{0,63}`.
pub fn check_operation_name(text: &str) -> Result<(), Invalid> {
    let ok = !text.is_empty()
        && text.len() <= 64
        && text.as_bytes()[0].is_ascii_lowercase()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(Invalid::new("operation", "invalid operation name"))
    }
}

impl UndoParams {
    pub fn validate(&self) -> Result<(), Invalid> {
        let selectors = [
            self.operation_id.is_some(),
            self.since.is_some(),
            self.agent.is_some(),
        ];
        if selectors.iter().filter(|s| **s).count() > 1 {
            return Err(Invalid::new(
                "operation_id",
                "at most one of operation_id, since and agent",
            ));
        }
        if let Some(id) = &self.operation_id {
            check_oplog_id("operation_id", id)?;
        }
        if let Some(since) = &self.since {
            parse_since(since)?;
        }
        if let Some(agent) = &self.agent {
            check_agent_id(agent)?;
        }
        Ok(())
    }
}

impl RedoParams {
    pub fn validate(&self) -> Result<(), Invalid> {
        if let Some(id) = &self.operation_id {
            check_oplog_id("operation_id", id)?;
        }
        Ok(())
    }
}

impl RestoreParams {
    pub fn validate(&self) -> Result<(), Invalid> {
        check_oplog_id("snapshot_id", &self.snapshot_id)
    }
}

impl TimelineParams {
    pub fn validate(&self) -> Result<(), Invalid> {
        if let Some(since) = &self.since {
            parse_since(since)?;
        }
        if let Some(agent) = &self.agent {
            check_agent_id(agent)?;
        }
        if self.limit.is_some_and(|l| l == 0 || l > TIMELINE_MAX_LIMIT) {
            return Err(Invalid::new("limit", "out of range"));
        }
        if self
            .only_worktree
            .as_ref()
            .is_some_and(|w| w.is_empty() || w.len() > 4096 || w.contains('\0'))
        {
            return Err(Invalid::new("only_worktree", "invalid path"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::{AgentKind, AgentOrigin};
    use crate::mcp_view::MAX_MCP_NAME_CHARS;
    use serde_json::{Map, Value, json};

    const ID: &str = "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5b";

    #[test]
    fn durations() {
        assert_eq!(parse_since("90s"), Ok(90));
        assert_eq!(parse_since("30m"), Ok(1800));
        assert_eq!(parse_since("2h"), Ok(7200));
        assert_eq!(parse_since("30d"), Ok(MAX_SINCE_SECS));
        for bad in [
            "",
            "m",
            "0m",
            "05m",
            "-5m",
            "5",
            "5w",
            "1.5h",
            "31d",
            " 5m",
            "5m ",
            "9999999999d",
            "5mm",
            "５m",
        ] {
            assert!(parse_since(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn agent_ids() {
        for ok in ["claude-1", "a", "claude:4242:1700000000", "codex_2.x"] {
            assert!(check_agent_id(ok).is_ok(), "{ok}");
        }
        let long = "a".repeat(65);
        for bad in [
            "",
            "-x",
            ".x",
            "a b",
            "a/b",
            "a\u{1b}[31m",
            "agente-ñ",
            long.as_str(),
        ] {
            assert!(check_agent_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn oplog_ids() {
        assert!(check_oplog_id("snapshot_id", ID).is_ok());
        for bad in [
            "",
            "HEAD",
            "0F8E2B7A-1C3D-4E5F-8A9B-0C1D2E3F4A5B",
            "0f8e2b7a1c3d4e5f8a9b0c1d2e3f4a5b",
            "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5",
            "../../0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f",
            "0f8e2b7a-1c3d-4e5f-8a9b-0c1d2e3f4a5g",
        ] {
            assert!(check_oplog_id("snapshot_id", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn operation_names() {
        assert!(check_operation_name("checkout").is_ok());
        assert!(check_operation_name("rebase-onto-base").is_ok());
        for bad in ["", "Checkout", "-x", "a b", "a;rm", "a_b", &"a".repeat(65)] {
            assert!(check_operation_name(bad).is_err(), "{bad:?}");
        }
    }

    /// No parameter can carry the requester: a client that declares itself
    /// another agent or a human is refused before anything happens.
    #[test]
    fn the_requester_is_never_a_parameter() {
        for extra in [
            r#""actor":{"actor":"unattributed"}"#,
            r#""human":true"#,
            r#""agent_name":"claude-2""#,
            r#""requester":"claude-2""#,
            r#""channel":"cli""#,
        ] {
            let run = format!(r#"{{"operation":"create-worktree","worktree":"/r",{extra}}}"#);
            assert!(
                serde_json::from_str::<crate::catalog::PrepareParams>(&run).is_err(),
                "{extra}"
            );
            let undo = format!(r#"{{"worktree":"/r",{extra}}}"#);
            assert!(
                serde_json::from_str::<UndoParams>(&undo).is_err(),
                "{extra}"
            );
            let resolve = format!(r#"{{{extra}}}"#);
            assert!(
                serde_json::from_str::<ResolveParams>(&resolve).is_err(),
                "{extra}"
            );
        }
        // `agent` in an undo selects whose work, never who asks.
        let p: UndoParams = serde_json::from_str(r#"{"agent":"claude-2"}"#).unwrap();
        assert!(p.validate().is_ok());
    }

    #[test]
    fn param_validation() {
        let undo = |json: &str| serde_json::from_str::<UndoParams>(json).unwrap().validate();
        assert!(undo("{}").is_ok());
        assert!(undo(r#"{"since":"2h"}"#).is_ok());
        assert_eq!(undo(r#"{"since":"2 hours"}"#).unwrap_err().field, "since");
        assert_eq!(undo(r#"{"agent":"x y"}"#).unwrap_err().field, "agent");
        assert_eq!(
            undo(r#"{"operation_id":"latest"}"#).unwrap_err().field,
            "operation_id"
        );
        assert!(undo(r#"{"since":"2h","agent":"claude-1"}"#).is_err());
        let restore = RestoreParams {
            worktree: None,
            snapshot_id: "../x".into(),
            surface: None,
        };
        assert_eq!(restore.validate().unwrap_err().field, "snapshot_id");
        let mut run = crate::catalog::PrepareParams {
            operation: crate::catalog::OperationId::CreateWorktree,
            worktree: None,
            args: Map::new(),
            surface: None,
            session_env: Vec::new(),
        };
        assert!(run.validate().is_ok());
        for i in 0..=MAX_ARGS_KEYS {
            run.args.insert(format!("k{i}"), Value::Bool(true));
        }
        assert_eq!(run.validate().unwrap_err().field, "args");
        run.args = Map::from_iter([("k".into(), Value::String("x".repeat(MAX_ARGS_BYTES)))]);
        assert_eq!(run.validate().unwrap_err().field, "args");
        let timeline = TimelineParams {
            worktree: None,
            only_worktree: None,
            since: None,
            agent: None,
            limit: Some(0),
        };
        assert!(timeline.validate().is_err());
    }

    /// Names of every property of a JSON schema.
    fn collect_fields(node: &Value, out: &mut Vec<String>) {
        match node {
            Value::Object(map) => {
                if let Some(Value::Object(props)) = map.get("properties") {
                    out.extend(props.keys().map(|k| k.to_lowercase()));
                }
                map.values().for_each(|v| collect_fields(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| collect_fields(v, out)),
            _ => {}
        }
    }

    fn entry(origin: EntryOrigin, actor: Actor, attribution: Attribution) -> TimelineEntry {
        TimelineEntry {
            id: "event:7".into(),
            origin,
            occurred_utc_ms: 1,
            utc_offset_s: 0,
            worktrees: vec![Untrusted::new("/r")],
            actor,
            attribution,
            protection: Protection {
                level: ProtectionLevel::None,
                snapshot_id: None,
            },
            files: ChangedFiles::Available {
                paths: vec![Untrusted::new("a.rs")],
                total: 1,
                first_parent: false,
            },
        }
    }

    /// ADR-GRP-013 § 6: no "human" anywhere in the schema, and nowhere to put
    /// a commit message or a file's content.
    #[test]
    fn timeline_schema_has_no_human_variant_and_no_message_field() {
        let schema = serde_json::to_value(schemars::schema_for!(TimelineResult)).unwrap();
        let mut fields = Vec::new();
        collect_fields(&schema, &mut fields);
        assert!(fields.contains(&"actor".to_owned()), "{fields:?}");
        for forbidden in ["message", "content", "subject", "author", "email", "body"] {
            assert!(
                !fields.iter().any(|f| f.contains(forbidden)),
                "{forbidden}: {fields:?}"
            );
        }
        let text = schema.to_string().to_lowercase();
        assert!(!text.contains("human"), "{text}");
        assert!(text.contains("unattributed"));
        assert!(serde_json::from_str::<Actor>(r#"{"actor":"human"}"#).is_err());
    }

    #[test]
    fn timeline_wire_form() {
        let git = entry(
            EntryOrigin::GitEvent {
                seq: 7,
                kind: crate::messages::GitEventKind::Commit,
                branch: Some(UntrustedName::new("feat-login")),
            },
            Actor::Unattributed,
            Attribution::Current,
        );
        let v = serde_json::to_value(&git).unwrap();
        assert_eq!(v["origin"]["entry"], "git-event");
        assert_eq!(v["origin"]["kind"], "commit");
        assert_eq!(v["origin"]["branch"]["untrusted"], "feat-login");
        assert_eq!(v["actor"]["actor"], "unattributed");
        assert_eq!(v["attribution"], "current");
        assert_eq!(v["protection"]["level"], "none");
        assert!(v["protection"]["snapshot_id"].is_null());
        assert_eq!(v["worktrees"][0]["untrusted"], "/r");
        assert_eq!(v["files"]["state"], "available");
        assert_eq!(v["files"]["paths"][0]["untrusted"], "a.rs");
        assert_eq!(v["files"]["first_parent"], false);
        let op = entry(
            EntryOrigin::Operation {
                operation_id: ID.into(),
                kind: TimelineOperationKind::Undo,
                subtype: None,
                state: TimelineOperationState::Finished,
                acted_on: vec![ActedOn::GitEvent(3), ActedOn::Snapshot(ID.into())],
            },
            Actor::Unattributed,
            Attribution::Recorded,
        );
        let v = serde_json::to_value(&op).unwrap();
        assert_eq!(v["origin"]["entry"], "operation");
        assert_eq!(v["origin"]["kind"], "undo");
        assert_eq!(v["origin"]["state"], "finished");
        assert_eq!(
            v["origin"]["acted_on"][0],
            json!({"target": "git-event", "id": 3})
        );
        assert_eq!(v["origin"]["acted_on"][1]["target"], "snapshot");
        assert_eq!(v["attribution"], "recorded");
        let back: TimelineEntry = serde_json::from_value(v).unwrap();
        assert_eq!(back, op);
        let none = serde_json::to_value(ChangedFiles::Unavailable).unwrap();
        assert_eq!(none, json!({"state": "unavailable"}));
        for (level, text) in [
            (ProtectionLevel::GuaranteedPrior, "guaranteed-prior"),
            (ProtectionLevel::HookPrior, "hook-prior"),
            (ProtectionLevel::Observation, "observation"),
            (ProtectionLevel::None, "none"),
        ] {
            assert_eq!(serde_json::to_value(level).unwrap(), text);
        }
    }

    #[test]
    fn timeline_params_validation() {
        let p = |json: &str| {
            serde_json::from_str::<TimelineParams>(json)
                .unwrap()
                .validate()
        };
        assert!(p(r#"{"agent":"unattributed"}"#).is_ok());
        assert!(p(r#"{"limit":200,"only_worktree":"/r"}"#).is_ok());
        assert!(p(r#"{"limit":201}"#).is_err());
        assert_eq!(
            p(r#"{"only_worktree":""}"#).unwrap_err().field,
            "only_worktree"
        );
        assert!(serde_json::from_str::<TimelineParams>(r#"{"human":true}"#).is_err());
    }

    /// A daemon that predates `only_worktree` refuses unknown fields: without the filter the
    /// field is not sent at all, not even as `null`.
    #[test]
    fn only_worktree_is_absent_from_the_wire_when_unused() {
        let none = TimelineParams {
            worktree: Some("/r".into()),
            only_worktree: None,
            since: None,
            agent: None,
            limit: None,
        };
        let wire = serde_json::to_value(&none).unwrap();
        assert!(wire.get("only_worktree").is_none(), "{wire}");
        let some = TimelineParams {
            only_worktree: Some("/r/wt".into()),
            ..none
        };
        assert_eq!(
            serde_json::to_value(&some).unwrap()["only_worktree"],
            "/r/wt"
        );
    }

    /// SEC-12: a branch with escapes travels marked, prints clean and is
    /// capped for MCP; the MCP projection has no paths.
    #[test]
    fn output_is_marked_and_bounded() {
        let evil = format!("feat/\u{1b}]52;c;cm0gLXJmIH4=\u{7}x{}", "y".repeat(400));
        let result = OperationRunResult {
            operation_id: ID.into(),
            prior_snapshot_id: ID.into(),
            fast_path: true,
            requester: RequesterView {
                actor: Actor::Agent {
                    kind: AgentKind::ClaudeCode,
                    name: None,
                    origin: AgentOrigin::Detected,
                },
                channel: RequestChannel::Mcp,
                via: ResolvedVia::Ancestry,
                confirmable: false,
            },
            changed_refs: vec![Untrusted::new(evil)],
            outcome: OperationOutcome::Done,
            layer: Layer::Mcp,
            git_output: Some(Untrusted::new("hook said hi")),
        };
        let shown = result.changed_refs[0].sanitized();
        assert!(!shown.contains('\u{1b}') && !shown.contains('\u{7}'));
        let mcp = serde_json::to_value(result.for_mcp()).unwrap();
        let mut keys: Vec<_> = mcp.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "actor",
                "changed_refs",
                "operation_id",
                "outcome",
                "prior_snapshot_id"
            ]
        );
        // M-03 and SEC-12: never Git's output over MCP.
        assert!(!mcp.to_string().contains("hook said hi"));
        let r = &mcp["changed_refs"][0];
        assert!(r["untrusted"].as_str().unwrap().chars().count() <= MAX_MCP_NAME_CHARS);
        assert_eq!(r["truncated"], true);
    }

    /// A restore result travels and comes back intact; a field the client
    /// does not know is refused.
    #[test]
    fn restore_result_round_trip() {
        let result = RestoreResult {
            operation_id: ID.into(),
            prior_snapshot_id: ID.into(),
            target_snapshot_id: ID.into(),
            requester: RequesterView {
                actor: Actor::Unattributed,
                channel: RequestChannel::Cli,
                via: ResolvedVia::None,
                confirmable: false,
            },
            worktrees: vec![Untrusted::new("/r/feat-login")],
            recreated: vec![Untrusted::new("/r/feat-x")],
            refs: vec![Untrusted::new("refs/heads/feat-login")],
            kept_branches: vec![Untrusted::new("refs/heads/later")],
            not_returned_branches: vec![Untrusted::new("refs/heads/gone")],
            written: 3,
            removed: 1,
            not_restored: vec![NotRestored {
                path: Untrusted::new("a.rs"),
                reason: NotRestoredReason::Overlap,
                kept_at: None,
            }],
            warnings: vec!["stash-kept".into()],
        };
        let mut v = serde_json::to_value(&result).unwrap();
        assert_eq!(v["kept_branches"][0]["untrusted"], "refs/heads/later");
        assert_eq!(
            v["not_returned_branches"][0]["untrusted"],
            "refs/heads/gone"
        );
        let back: RestoreResult = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(back, result);
        v.as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        assert!(serde_json::from_value::<RestoreResult>(v).is_err());
    }
}
