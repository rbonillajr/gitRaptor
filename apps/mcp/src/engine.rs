//! `raptor-mcp` as a client of the engine (ADR-MCP-001 § 1, BR-MCP-CONS-001):
//! the connection opens on the first tool call, never at the handshake
//! (BR-MCP-TIME-003), and starts the engine on demand with the `raptor`
//! installed next to this binary. Nothing is read from the repo here: every
//! answer comes from the engine.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};

use gitraptor_api::catalog::{
    GuardrailsDenial, MAX_COMMIT_MESSAGE_BYTES, MAX_COMMIT_PATHS, McpPrepareResult, OperationId,
    PrepareParams, RejectReason, RunParams, SnapshotRunResult, WarningCode,
};
use gitraptor_api::mcp_view::{
    MCP_COMMIT_CALL_LIMIT, MCP_WRITE_RETRY_AFTER_S, MCP_WRITE_TIME_LIMIT, McpToolError,
};
use gitraptor_api::messages::{ClientKind, RepoStateView};
use gitraptor_api::methods::{
    self, MCP_UNAVAILABLE, McpStatus, McpStatusParams, McpUnavailable, McpUnavailableData,
    OPERATION_SNAPSHOT_QUOTA, OPERATION_SNAPSHOT_TIME_LIMIT,
};
use gitraptor_api::rpc::{Id, Request, ScopeRefusal, ScopeRefusedData, ServerMessage, code};
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::profile::ProfileDirs;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::messages::ToolRefusal;
use crate::safe_commit::{CommitCall, CommitOutcome};
use crate::safe_commit_paths::path_params;

/// Most rule codes a `policy-denied` names.
const MAX_DENIED_RULES: usize = 4;

/// The id of the `operation.run` request. The client library's own calls wait at most 10 s,
/// less than a snapshot may take, so this one is sent and awaited here; its id lies far from
/// the library's counter, which never reaches it.
const RUN_REQUEST_ID: u64 = 1 << 40;

/// The lazy connection to the engine.
#[derive(Default)]
pub struct Engine {
    client: Mutex<Option<Client>>,
    /// Whether the last call ended with an open connection to the engine.
    connected: AtomicBool,
    /// Whether the last snapshot call sent its `operation.run`: past that point a failure of
    /// the transport leaves the outcome unknown.
    run_sent: AtomicBool,
    /// Whether a write call holds the connection now.
    writing: AtomicBool,
}

/// Marks a write as holding the connection until it is dropped.
struct Writing<'a>(&'a AtomicBool);

impl<'a> Writing<'a> {
    fn start(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Relaxed);
        Self(flag)
    }
}

impl Drop for Writing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

impl Engine {
    /// `mcp.status`: the caller's repo, resolved by the engine. A broken
    /// connection (the engine restarted) is opened again once.
    pub fn status(&self, cursor: Option<String>) -> Result<McpStatus, McpToolError> {
        // A call still running past its time limit holds the connection:
        // this one answers at once rather than queue behind it, spending
        // the connection's read budget on an answer nobody reads.
        let mut slot = match self.client.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(self.late()),
        };
        let reused = slot.is_some();
        let result = match call_status(&mut slot, cursor.as_deref()) {
            Err(ClientError::Io(_) | ClientError::Protocol(_)) if reused => {
                call_status(&mut slot, cursor.as_deref())
            }
            other => other,
        };
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        result.map_err(|err| refusal(&err))
    }

    /// `operation.prepare` and `operation.run` of a `snapshot`, in the same connection and under
    /// the same lock as [`Engine::status`]. A connection that breaks before `run` is sent opens
    /// again and repeats `prepare` only; `run` is never sent twice. `outcome-unknown` is
    /// only for a transport that fails once `run` is sent.
    pub fn snapshot(&self, label: &str) -> Result<SnapshotRunResult, ToolRefusal> {
        let started = Instant::now();
        let mut slot = match self.client.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(self.late().into()),
        };
        let _writing = Writing::start(&self.writing);
        self.run_sent.store(false, Ordering::Relaxed);
        let reused = slot.is_some();
        let args = serde_json::Map::from_iter([("label".to_owned(), json!(label))]);
        let plan = match call_prepare(&mut slot, OperationId::Snapshot, args.clone()) {
            Err(ClientError::Io(_) | ClientError::Protocol(_)) if reused => {
                call_prepare(&mut slot, OperationId::Snapshot, args)
            }
            other => other,
        };
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        let plan = plan.map_err(|err| of_snapshot(snapshot_refusal(&err)))?;
        let Some(client) = slot.as_mut() else {
            return Err(McpToolError::Internal.into());
        };
        self.run_sent.store(true, Ordering::Relaxed);
        let budget = MCP_WRITE_TIME_LIMIT.saturating_sub(started.elapsed());
        let result = call_run(client, plan.plan_id, plan.warnings, budget);
        if matches!(result, Err(ClientError::Io(_) | ClientError::Protocol(_))) {
            *slot = None;
        }
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        result.map_err(|err| {
            of_snapshot(match err {
                ClientError::Io(_) | ClientError::Protocol(_) => {
                    McpToolError::OutcomeUnknown.into()
                }
                other => snapshot_refusal(&other),
            })
        })
    }

    /// `safe_commit`: the session's worktree from `mcp.status`, then `operation.prepare` and
    /// `operation.run` of a `commit` in the same connection, under the same lock as
    /// [`Engine::status`]. A second write while one holds the connection is
    /// `operation-in-progress` (`write`) at once. A connection that breaks before `run` is sent
    /// opens again and repeats `prepare` only; `run` is never sent twice, and a transport that
    /// fails after it, or [`MCP_COMMIT_CALL_LIMIT`] passing, is `outcome-unknown`. The preview's
    /// decision and warnings are not looked at: the decision that counts is the one in `run`.
    pub fn commit(&self, call: &CommitCall) -> Result<CommitOutcome, ToolRefusal> {
        let started = Instant::now();
        let mut slot = match self.client.try_lock() {
            Ok(slot) => slot,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) if self.writing.load(Ordering::Relaxed) => {
                return Err(write_in_progress());
            }
            Err(TryLockError::WouldBlock) => return Err(self.late().into()),
        };
        let _writing = Writing::start(&self.writing);
        self.run_sent.store(false, Ordering::Relaxed);
        let reused = slot.is_some();
        let status = match call_status(&mut slot, None) {
            Err(ClientError::Io(_) | ClientError::Protocol(_)) if reused => {
                call_status(&mut slot, None)
            }
            other => other,
        };
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        let status = status.map_err(|err| commit_refusal(&err))?;
        if status.repo_state == RepoStateView::Unavailable {
            return Err(McpToolError::RepoUnavailable.into());
        }
        let mut args = serde_json::Map::from_iter([("message".to_owned(), json!(call.message))]);
        if let Some(paths) = &call.paths {
            args.insert("paths".to_owned(), json!(paths));
        }
        let plan = match call_prepare(&mut slot, OperationId::Commit, args.clone()) {
            Err(ClientError::Io(_) | ClientError::Protocol(_)) => {
                call_prepare(&mut slot, OperationId::Commit, args)
            }
            other => other,
        };
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        let plan = plan.map_err(|err| commit_refusal(&err))?;
        let Some(client) = slot.as_mut() else {
            return Err(McpToolError::Internal.into());
        };
        self.run_sent.store(true, Ordering::Relaxed);
        let budget = MCP_COMMIT_CALL_LIMIT.saturating_sub(started.elapsed());
        let result = call_run(client, plan.plan_id, Vec::new(), budget);
        if matches!(result, Err(ClientError::Io(_) | ClientError::Protocol(_))) {
            *slot = None;
        }
        self.connected.store(slot.is_some(), Ordering::Relaxed);
        let run = result.map_err(|err| match err {
            ClientError::Io(_) | ClientError::Protocol(_) => McpToolError::OutcomeUnknown.into(),
            other => commit_refusal(&other),
        })?;
        Ok(CommitOutcome {
            run,
            worktree: status.worktree,
        })
    }

    /// What a snapshot call that did not come back in time means: once `run` is sent, nobody
    /// knows whether the point was saved.
    pub fn late_write(&self) -> McpToolError {
        if self.run_sent.load(Ordering::Relaxed) {
            McpToolError::OutcomeUnknown
        } else {
            self.late()
        }
    }

    /// What a call that did not end in time means: with no connection yet,
    /// the engine is still being started or replaced and counts as
    /// unavailable (BR-MCP-EDGE-001); with one, it was late.
    pub fn late(&self) -> McpToolError {
        if self.connected.load(Ordering::Relaxed) {
            McpToolError::TimeLimit
        } else {
            McpToolError::EngineUnavailable
        }
    }
}

/// One `mcp.status`, connecting first when there is no connection; a
/// connection that breaks is dropped.
fn call_status(slot: &mut Option<Client>, cursor: Option<&str>) -> Result<McpStatus, ClientError> {
    if slot.is_none() {
        let dirs = ProfileDirs::resolve().map_err(|_| ClientError::NotRunning)?;
        *slot = Some(ensure_daemon(&ClientOptions::new(dirs, ClientKind::Mcp))?);
    }
    let client = slot.as_mut().ok_or(ClientError::NotRunning)?;
    let params = McpStatusParams {
        cursor: cursor.map(str::to_owned),
    };
    let result = client.call::<_, McpStatus>(methods::MCP_STATUS, params);
    if matches!(result, Err(ClientError::Io(_) | ClientError::Protocol(_))) {
        *slot = None;
    }
    result
}

fn connected(slot: &mut Option<Client>) -> Result<&mut Client, ClientError> {
    if slot.is_none() {
        let dirs = ProfileDirs::resolve().map_err(|_| ClientError::NotRunning)?;
        *slot = Some(ensure_daemon(&ClientOptions::new(dirs, ClientKind::Mcp))?);
    }
    slot.as_mut().ok_or(ClientError::NotRunning)
}

/// One `operation.prepare`; a connection that breaks is dropped.
fn call_prepare(
    slot: &mut Option<Client>,
    operation: OperationId,
    args: serde_json::Map<String, Value>,
) -> Result<McpPrepareResult, ClientError> {
    let params = PrepareParams {
        operation,
        worktree: None,
        args,
        surface: None,
        session_env: Vec::new(),
    };
    let client = connected(slot)?;
    let result = client.call::<_, McpPrepareResult>(methods::OPERATION_PREPARE, params);
    if matches!(result, Err(ClientError::Io(_) | ClientError::Protocol(_))) {
        *slot = None;
    }
    result
}

/// `operation.run` of a prepared plan, answered within `budget`.
fn call_run<T: DeserializeOwned>(
    client: &mut Client,
    plan_id: String,
    accepted_warnings: Vec<WarningCode>,
    budget: Duration,
) -> Result<T, ClientError> {
    let params = RunParams {
        plan_id,
        accepted_warnings,
        confirmation: None,
    };
    let request = Request::new(RUN_REQUEST_ID, methods::OPERATION_RUN, params);
    let line = serde_json::to_vec(&request).map_err(|_| ClientError::Protocol("encode"))?;
    client.send_raw(&line)?;
    let deadline = Instant::now() + budget;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ClientError::Io(io::ErrorKind::TimedOut.into()));
        }
        match client.read_message(Some(remaining))? {
            None => return Err(ClientError::Io(io::ErrorKind::UnexpectedEof.into())),
            Some(ServerMessage::Response(r)) if r.id == Some(Id::Num(RUN_REQUEST_ID)) => {
                return r.into_result().map_err(ClientError::Rpc);
            }
            // An error the daemon could not tie to a request answers this one.
            Some(ServerMessage::Response(r)) if r.id.is_none() => {
                return Err(r.error.map_or(
                    ClientError::Protocol("response without id"),
                    ClientError::Rpc,
                ));
            }
            Some(_) => {}
        }
    }
}

/// The refusal of a snapshot call that reached the daemon, with the typed `params` its
/// template needs.
fn snapshot_refusal(err: &ClientError) -> ToolRefusal {
    let ClientError::Rpc(e) = err else {
        return refusal(err).into();
    };
    let with = |code, params| ToolRefusal {
        code,
        params: Some(params),
    };
    match e.code {
        c if c == code::OPERATION_REJECTED => {
            rejected_refusal(rejected_reason(e.data.as_ref()), None)
        }
        c if c == OPERATION_SNAPSHOT_QUOTA.code => ToolRefusal {
            code: McpToolError::QuotaExceeded,
            params: e.data.clone(),
        },
        c if c == OPERATION_SNAPSHOT_TIME_LIMIT.code => {
            with(McpToolError::TimeLimit, json!({"saved": false}))
        }
        c if c == code::RATE_LIMITED => with(
            McpToolError::RateLimited,
            json!({"retry_after_s": MCP_WRITE_RETRY_AFTER_S}),
        ),
        c if c == code::INVALID_PARAMS => McpToolError::InvalidText.into(),
        _ => refusal(err).into(),
    }
}

/// A snapshot's own texts where a code is shared with `safe_commit`: the hint stays out of the
/// answer's `params`.
fn of_snapshot(mut refusal: ToolRefusal) -> ToolRefusal {
    if matches!(
        refusal.code,
        McpToolError::OperationInProgress | McpToolError::OutcomeUnknown
    ) {
        let mut params = refusal.params.take().unwrap_or_else(|| json!({}));
        params["tool"] = "snapshot".into();
        refusal.params = Some(params);
    }
    refusal
}

fn write_in_progress() -> ToolRefusal {
    ToolRefusal {
        code: McpToolError::OperationInProgress,
        params: Some(json!({"kind": "write"})),
    }
}

/// The `reason` of `OPERATION_REJECTED` data, read field by field: a field this binary does
/// not know does not hide the reason.
fn rejected_reason(data: Option<&Value>) -> Option<RejectReason> {
    data.and_then(|d| d.get("reason"))
        .and_then(|r| serde_json::from_value(r.clone()).ok())
}

/// The refusal of a `safe_commit` call that reached the daemon, with the typed `params` its
/// template needs. The rule codes of a denial are the policy's own names, at most
/// [`MAX_DENIED_RULES`]; a path is repo text and travels as untrusted.
pub(crate) fn commit_refusal(err: &ClientError) -> ToolRefusal {
    let ClientError::Rpc(e) = err else {
        return refusal(err).into();
    };
    let with = |code, params| ToolRefusal {
        code,
        params: Some(params),
    };
    let field = |key: &str| e.data.as_ref().and_then(|d| d.get(key));
    let text = |key: &str| field(key).and_then(Value::as_str);
    match e.code {
        c if c == code::OPERATION_REJECTED => {
            let reason = rejected_reason(e.data.as_ref());
            if reason == Some(RejectReason::GuardrailsDenied) {
                return denied(field("guardrails"));
            }
            rejected_refusal(reason, text("path"))
        }
        c if c == code::PRIOR_SNAPSHOT_FAILED => with(
            McpToolError::Aborted,
            json!({"operation_id": text("operation_id")}),
        ),
        c if c == code::OPERATION_FAILED => {
            let code = match text("failure") {
                Some("time-limit") => McpToolError::TimeLimit,
                Some("unchanged") => McpToolError::FailedUnchanged,
                _ => McpToolError::FailedChanged,
            };
            with(code, json!({"operation_id": text("operation_id")}))
        }
        c if c == code::RATE_LIMITED || c == code::LIMIT_REACHED => with(
            McpToolError::RateLimited,
            json!({"retry_after_s": MCP_WRITE_RETRY_AFTER_S}),
        ),
        // A client that spoke to the daemon directly could reach these; this tool checks
        // both first.
        c if c == code::INVALID_PARAMS => match (text("field"), text("reason")) {
            (Some("paths"), Some("too-many")) => {
                with(McpToolError::TooManyPaths, json!({"max": MAX_COMMIT_PATHS}))
            }
            (Some("paths"), _) => McpToolError::InvalidPath.into(),
            (Some("message"), reason) => with(
                McpToolError::InvalidText,
                json!({
                    "field": "message",
                    "reason": reason.unwrap_or("invalid-chars"),
                    "max_bytes": MAX_COMMIT_MESSAGE_BYTES,
                }),
            ),
            _ => McpToolError::Internal.into(),
        },
        _ => refusal(err).into(),
    }
}

/// `policy-denied` with the rules that denied and the decision's id.
fn denied(denial: Option<&Value>) -> ToolRefusal {
    let Some(denial) =
        denial.and_then(|d| serde_json::from_value::<GuardrailsDenial>(d.clone()).ok())
    else {
        return McpToolError::PolicyDenied.into();
    };
    let mut rules: Vec<Value> = Vec::new();
    for reason in &denial.reasons {
        let rule = serde_json::to_value(reason.rule).unwrap_or(Value::Null);
        if rule.is_string() && !rules.contains(&rule) && rules.len() < MAX_DENIED_RULES {
            rules.push(rule);
        }
    }
    ToolRefusal {
        code: McpToolError::PolicyDenied,
        params: Some(json!({"rules": rules, "decision_id": denial.decision_id})),
    }
}

/// The tool refusal for a plan the daemon rejected: the write table every write tool shares.
/// `path` is the path the daemon refused, when it names one.
fn rejected_refusal(reason: Option<RejectReason>, path: Option<&str>) -> ToolRefusal {
    let with = |code, params| ToolRefusal {
        code,
        params: Some(params),
    };
    match reason {
        Some(RejectReason::UnattributedWithoutCockpit) => McpToolError::Unattributed.into(),
        Some(RejectReason::OperationInProgress) => {
            with(McpToolError::OperationInProgress, json!({"kind": "git"}))
        }
        Some(RejectReason::WriteInProgress) => write_in_progress(),
        Some(RejectReason::GitBusy) => McpToolError::GitBusy.into(),
        Some(RejectReason::GuardrailsConfirmationRequired) => {
            McpToolError::ConfirmationRequired.into()
        }
        Some(
            RejectReason::StateChanged
            | RejectReason::PlanUnknown
            | RejectReason::WarningsMismatch
            | RejectReason::RepoIdentityChanged,
        ) => McpToolError::StateChanged.into(),
        Some(RejectReason::DaemonStopping | RejectReason::GitUnavailable) => {
            McpToolError::EngineUnavailable.into()
        }
        Some(RejectReason::ExecutorDescendant) => McpToolError::IdentityUnverified.into(),
        Some(RejectReason::DetachedHead) => McpToolError::DetachedHead.into(),
        Some(RejectReason::NothingToCommit) => McpToolError::NothingToCommit.into(),
        Some(
            reason @ (RejectReason::WorktreeLocked
            | RejectReason::BranchCheckedOutElsewhere
            | RejectReason::Grafts),
        ) => with(
            McpToolError::FailedUnchanged,
            json!({"reason": serde_json::to_value(reason).unwrap_or(Value::Null)}),
        ),
        Some(RejectReason::QueueFull) => with(
            McpToolError::RateLimited,
            json!({"retry_after_s": MCP_WRITE_RETRY_AFTER_S}),
        ),
        Some(RejectReason::SharedWorktreeNeedsPaths) => McpToolError::OtherSessionPresent.into(),
        Some(
            reason @ (RejectReason::PathNotFound
            | RejectReason::PathInvalid
            | RejectReason::PathOutsideWorktree
            | RejectReason::PathIgnored),
        ) => ToolRefusal {
            code: match reason {
                RejectReason::PathNotFound => McpToolError::PathNotFound,
                RejectReason::PathInvalid => McpToolError::InvalidPath,
                RejectReason::PathIgnored => McpToolError::PathIgnored,
                _ => McpToolError::PathOutsideWorktree,
            },
            params: path.map(path_params),
        },
        _ => McpToolError::Internal.into(),
    }
}

fn refusal(err: &ClientError) -> McpToolError {
    match err {
        ClientError::Rpc(e) if e.code == code::SCOPE_REFUSED => {
            let reason = e
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<ScopeRefusedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(ScopeRefusal::NotAllowlisted) => McpToolError::RepoNotEnabled,
                _ => McpToolError::NotInObservedWorktree,
            }
        }
        // A repo or worktree the engine cannot read says why and nothing else. A reason this
        // binary does not know, or none, is a repo that cannot be read: it fails closed.
        ClientError::Rpc(e) if e.code == MCP_UNAVAILABLE.code => {
            let reason = e
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<McpUnavailableData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(McpUnavailable::WorktreeMissing) => McpToolError::WorktreeMissing,
                Some(McpUnavailable::OtherOwner) => McpToolError::RepoOtherOwner,
                Some(McpUnavailable::WorktreeUntrusted) => McpToolError::WorktreeUntrusted,
                Some(McpUnavailable::RepoUnreadable) | None => McpToolError::RepoUnavailable,
            }
        }
        // A cursor the daemon does not know, or finds malformed.
        ClientError::Rpc(e) if e.code == code::NOT_FOUND || e.code == code::INVALID_PARAMS => {
            McpToolError::InvalidCursor
        }
        ClientError::Rpc(e) if e.code == code::IDENTITY_UNVERIFIED => {
            McpToolError::IdentityUnverified
        }
        // The connection's read budget (US-MCP-005, ADR-MCP-001 § 6).
        ClientError::Rpc(e) if e.code == code::RATE_LIMITED => McpToolError::RateLimited,
        ClientError::Rpc(_) | ClientError::Protocol(_) | ClientError::Unsupported(_) => {
            McpToolError::Internal
        }
        // Not running and not startable, an incompatible engine, a rejected
        // channel or a broken connection: the installation must be checked.
        _ => McpToolError::EngineUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::rpc::ErrorObject;

    #[test]
    fn engine_refusals_map_to_the_tool_codes() {
        let scope = |reason| {
            ClientError::Rpc(
                ErrorObject::new(code::SCOPE_REFUSED, "x").with_data(ScopeRefusedData { reason }),
            )
        };
        assert_eq!(
            refusal(&scope(ScopeRefusal::NotAllowlisted)),
            McpToolError::RepoNotEnabled
        );
        for reason in [ScopeRefusal::NotObserved, ScopeRefusal::NoWorkingFolder] {
            assert_eq!(refusal(&scope(reason)), McpToolError::NotInObservedWorktree);
        }
        let limited = ClientError::Rpc(ErrorObject::new(code::RATE_LIMITED, "rate limited"));
        assert_eq!(refusal(&limited), McpToolError::RateLimited);
    }

    /// Past the time limit: an engine never reached is unavailable; a
    /// connected one was late. A second call never queues behind the first.
    #[test]
    fn a_late_call_is_told_apart_and_never_queued() {
        let engine = Engine::default();
        assert_eq!(engine.late(), McpToolError::EngineUnavailable);
        engine.connected.store(true, Ordering::Relaxed);
        assert_eq!(engine.late(), McpToolError::TimeLimit);
        let _held = engine.client.lock().unwrap();
        assert_eq!(engine.status(None), Err(McpToolError::TimeLimit));
    }
}

#[cfg(test)]
#[path = "engine_status_tests.rs"]
mod status_tests;

#[cfg(test)]
#[path = "engine_confirmation_tests.rs"]
mod confirmation_tests;
