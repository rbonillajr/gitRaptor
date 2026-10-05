//! Parameters and results of the protected operation and the Time Machine
//! commands (TS-TMC-004, ADR-TMC-004 § 1, ADR-TMC-005 § 1).
//!
//! No parameter type names the requester: the daemon resolves it from the
//! caller's process ancestry, and `deny_unknown_fields` rejects a client
//! that tries to declare one (`actor`, `agent_name`, `human`). Every value
//! is checked for format before anything is touched (BR-TMC-VAL-001).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Untrusted;
use crate::actor::Actor;
use crate::catalog::{Layer, OperationOutcome};
use crate::untrusted::MAX_MCP_UNTRUSTED_BYTES;

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
                .map(|r| r.capped(MAX_MCP_UNTRUSTED_BYTES))
                .collect(),
            outcome: self.outcome,
        }
    }
}

/// Why a Time Machine command (undo, and later redo and restore) was
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

fn cap_actor(actor: &Actor) -> Actor {
    match actor {
        Actor::Agent { kind, name, origin } => Actor::Agent {
            kind: *kind,
            name: name.as_ref().map(|n| n.capped(MAX_MCP_UNTRUSTED_BYTES)),
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

/// `timemachine.timeline` parameters (US-TMC-006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineParams {
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
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
        if self.limit == Some(0) {
            return Err(Invalid::new("limit", "must be positive"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::{AgentKind, AgentOrigin};
    use serde_json::{Map, Value};

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
            since: None,
            agent: None,
            limit: Some(0),
        };
        assert!(timeline.validate().is_err());
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
        assert!(r["untrusted"].as_str().unwrap().len() <= MAX_MCP_UNTRUSTED_BYTES);
        assert_eq!(r["truncated"], true);
    }
}
