//! The user-operation executor of the daemon (ADR-CKP-002, TS-CKP-002).
//!
//! The only way the TUI and the MCP write to a repo: an operation of the catalog
//! ([`gitraptor_api::catalog`]) in two phases.
//!
//! - **Prepare** ([`Executor::prepare`]) has no effects and writes no oplog entry. It checks the
//!   requester and its [`Layer`], the arguments, the common preconditions ([`facts`]) and the
//!   operation's own ones (its story, through [`ProtectedBackend::plan_op`]), the base permission
//!   rule, and builds the plan: a random `plan_id` bound to the connection and the requester,
//!   a SHA-256 fingerprint that includes the requester, the layer and the catalog version, the
//!   warnings, a preview of the Guardrails decision and, when the plan touches another actor's
//!   work, a one-use confirmation challenge bound to the fingerprint.
//! - **Run** ([`Executor::run`]) takes the plan (it is consumed by any attempt), checks the
//!   warnings and the challenge, waits in the repo's write-lock queue ([`crate::repo_lock`],
//!   shared with the Time Machine applier), resolves the requester again, rebuilds the plan and
//!   compares identity and fingerprint, takes the decision that counts, and only then enters the
//!   protected operation (intent, guaranteed prior snapshot, step, record).
//!
//! A caller that descends from a child of the executor is refused before any lock (H-01). The
//! executor launches `git` only through [`git::run_git`], which is the only user of the
//! invocation of user operations of `crates/git`.

pub mod facts;
pub mod gate;
pub mod git;
pub mod new_path;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gitraptor_api::Actor;
use gitraptor_api::Untrusted;
use gitraptor_api::catalog::{
    CATALOG_VERSION, DecisionView, Diagnostic, Layer, MAX_PLANS_PER_CONNECTION,
    MAX_QUEUED_PER_REPO, MCP_TIME_LIMIT_MS, OperationArgs, OperationEventData, OperationId,
    OperationOutcome, PLAN_TTL_MS, PrepareParams, RejectReason, RunParams, WarningCode, entry,
};
use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_git::user_ops::{SessionEnv, validate_session_env};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub use facts::{RepoFacts, check_common};
pub use gate::{GateDecision, GateRequest, GuardrailsGate, NoGuardrails, PlanClose};
pub use git::{CancelToken, GitRun, Interrupt, TimeLimit, run_git};
pub use new_path::{NewPathError, check_new_worktree_path};

use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::ProcSource;
use crate::channel::requester::{Resolution, Who};
use crate::repo_lock::{self, QueueError};
use crate::timemachine::oplog::Channel;
use crate::timemachine::protected::{
    Binding, ChallengeBook, ChallengeError, ProtectedBackend, ProtectedError, ProtectedOperation,
    ProtectedOutcome, ProtectedRequest, ProtectedStep, RepoHandle, ScopeError, StepCtx, StepError,
    StepOutput, StepScope,
};

/// Whose work an operation affects (ADR-CKP-002 § 3), from the engine's events. Without a clear
/// attribution it counts as another actor's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Affected {
    /// Nobody's work (e.g. a new worktree).
    Nobody,
    /// Only unattributed work.
    Unattributed,
    /// Only the work of this agent session.
    Agent { session_id: String },
    /// Another agent, mixed or unclear.
    Other,
}

/// The operation's own part of a plan, from its story.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpPlan {
    /// Expected values (oids, heads) that enter the fingerprint and go to Git where it admits
    /// them.
    pub expected: serde_json::Value,
    pub warnings: Vec<WarningCode>,
    pub affected: Affected,
    /// Another agent session is present in the worktree (Q-MCP-21).
    pub other_session: bool,
}

/// Why the operation's story refused or could not plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    /// The story is not built yet.
    NotImplemented,
    Rejected(RejectReason),
}

/// What a step receives to run a plan.
pub struct StepPlan<'a> {
    pub operation: OperationId,
    pub args: &'a OperationArgs,
    pub repo: &'a RepoHandle,
    pub facts: &'a RepoFacts,
    pub op_plan: &'a OpPlan,
    pub layer: Layer,
    /// The validated session variables for the hooks (M-01).
    pub session_env: &'a SessionEnv,
    pub cancel: CancelToken,
}

/// The caller as the channel knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub connection: u64,
    pub pid: u32,
    pub start_us: u64,
    /// An MCP connection (its schema for `create-worktree` has no path).
    pub mcp: bool,
}

/// Why the executor did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    Rejected(RejectReason),
    /// The operation's story is not built yet: `implemented_by`.
    NotImplemented(&'static str),
    Invalid(String),
    /// Too many live plans on this connection.
    TooManyPlans,
    /// The step declared a scope outside its repo.
    Scope(ScopeError),
    Protected(ProtectedError),
}

impl From<RejectReason> for ExecError {
    fn from(r: RejectReason) -> Self {
        Self::Rejected(r)
    }
}

/// The layer the daemon fixes for a requester (ADR-CKP-002 § 4, M-03): `cockpit` only for an
/// unattributed caller that passes the reserved checks 1 to 3 (`confirmable`), never for a
/// descendant of the executor; `mcp` for everyone else.
pub fn layer_for(r: &Resolution) -> Layer {
    if r.via != ResolvedVia::Executor && !r.who.is_agent() && r.confirmable {
        Layer::Cockpit
    } else {
        Layer::Mcp
    }
}

/// Whether `who` with `layer` may ask for `op` at all (§ 1 and § 3).
pub fn admit(op: OperationId, layer: Layer, who: &Who) -> Result<(), RejectReason> {
    if layer == Layer::Mcp && !who.is_agent() {
        return Err(RejectReason::UnattributedWithoutCockpit);
    }
    if !entry(op).offered_to(layer) {
        return Err(RejectReason::NotAvailableForLayer);
    }
    Ok(())
}

/// The base permission rule (ADR-TMC-005 § 2, extended in ADR-CKP-002 § 3): `Ok(true)` when it
/// needs the confirmation bound to the plan.
pub fn permission(who: &Who, layer: Layer, affected: &Affected) -> Result<bool, RejectReason> {
    match (&who.requester, affected) {
        (_, Affected::Nobody) => Ok(false),
        (
            crate::timemachine::oplog::Requester::Agent { session_id, .. },
            Affected::Agent { session_id: owner },
        ) if session_id == owner => Ok(false),
        (crate::timemachine::oplog::Requester::Agent { .. }, _) => Err(RejectReason::ForeignWork),
        (crate::timemachine::oplog::Requester::Unattributed, Affected::Unattributed) => Ok(false),
        (crate::timemachine::oplog::Requester::Unattributed, _) if layer == Layer::Cockpit => {
            Ok(true)
        }
        (crate::timemachine::oplog::Requester::Unattributed, _) => Err(RejectReason::ForeignWork),
    }
}

#[derive(Serialize)]
struct FingerprintView<'a> {
    catalog_version: u32,
    operation: OperationId,
    args: serde_json::Value,
    layer: Layer,
    actor: &'a Actor,
    requester: String,
    repo_id: &'a str,
    worktree: &'a PathBuf,
    facts: &'a RepoFacts,
    op_plan: &'a OpPlan,
    session_env: Vec<&'a str>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[allow(clippy::too_many_arguments)]
fn fingerprint(
    op: OperationId,
    args: &OperationArgs,
    layer: Layer,
    who: &Who,
    repo: &RepoHandle,
    facts: &RepoFacts,
    op_plan: &OpPlan,
    session_env: &SessionEnv,
) -> [u8; 32] {
    let view = FingerprintView {
        catalog_version: CATALOG_VERSION,
        operation: op,
        args: args.fingerprint_view(),
        layer,
        actor: &who.actor,
        requester: format!("{:?}", who.requester),
        repo_id: &repo.repo_id,
        worktree: &repo.worktree,
        facts,
        op_plan,
        session_env: session_env.names(),
    };
    let bytes = serde_json::to_vec(&view).unwrap_or_default();
    Sha256::digest(&bytes).into()
}

struct StoredPlan {
    caller: Caller,
    who: Who,
    layer: Layer,
    channel: RequestChannel,
    operation: OperationId,
    args: OperationArgs,
    repo: RepoHandle,
    facts: RepoFacts,
    fingerprint: [u8; 32],
    warnings: Vec<WarningCode>,
    challenge: bool,
    session_env: SessionEnv,
    created: Instant,
}

impl StoredPlan {
    fn gate_request(&self) -> Option<GateRequest> {
        entry(self.operation).governed.map(|op| GateRequest {
            repo_id: self.repo.repo_id.clone(),
            operation: op,
            layer: self.layer,
            actor: self.who.actor.clone(),
            fingerprint: hex(&self.fingerprint),
        })
    }
}

/// A prepared plan, as the channel returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub plan_id: String,
    pub fingerprint: String,
    pub layer: Layer,
    pub warnings: Vec<WarningCode>,
    pub decision: DecisionView,
    pub challenge: Option<String>,
    pub expires_in_ms: u64,
    pub diagnostics: Vec<Diagnostic>,
}

/// What prepare needs from the channel.
pub struct PrepareInput<'a> {
    pub caller: Caller,
    pub resolution: &'a Resolution,
    pub layer: Layer,
    pub channel: RequestChannel,
    pub params: &'a PrepareParams,
    pub repo: RepoHandle,
    /// The confirmation checks run now (ADR-TMC-005 § 3): `None` if the caller may confirm.
    pub confirm_refusal: Option<RefusalReason>,
}

/// The daemon's view while running: who publishes events, the marks and the process table.
pub struct RunEnv<'a> {
    pub marks: &'a Arc<ExecutorMarks>,
    pub procs: &'a dyn ProcSource,
    pub stopping: &'a AtomicBool,
    pub prior_deadline: Duration,
    /// The engine mark of the operation's repo once its worktrees are calm
    /// (US-TMC-004); `None`: a `git` the engine has not persisted yet is
    /// still in the way.
    pub engine_mark: &'a (dyn Fn(&str, &[std::path::PathBuf]) -> Option<i64> + Sync),
    /// After the step ran: takes the anchor of the state it left, with the
    /// operation as cause (US-TMC-004).
    pub after_step: &'a (dyn Fn(&str, &[std::path::PathBuf], &str) + Sync),
    pub publish: &'a (dyn Fn(&str, OperationEventData) + Sync),
}

/// What run needs from the channel.
pub struct RunInput<'a> {
    pub caller: Caller,
    /// The requester when the request arrived (for the descendant check before any lock).
    pub resolution: &'a Resolution,
    pub params: &'a RunParams,
    /// Resolves the requester and its layer again, under the lock; with the confirmation
    /// checks run now.
    pub resolve_again:
        &'a dyn Fn() -> Result<(Resolution, Layer, Option<RefusalReason>), ExecError>,
}

/// An executed plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub outcome: ProtectedOutcome,
    pub result: OperationOutcome,
    pub who: Who,
    pub layer: Layer,
    pub channel: RequestChannel,
    /// Git's output, only for layer `cockpit` (M-03); untrusted, capped.
    pub git_output: Option<String>,
}

#[derive(Debug, Clone)]
struct RunningOp {
    cancel: CancelToken,
}

/// The executor of the daemon: plans, running operations and the confirmation challenges.
pub struct Executor {
    gate: Arc<dyn GuardrailsGate>,
    plans: Mutex<HashMap<String, StoredPlan>>,
    running: Arc<Mutex<HashMap<String, RunningOp>>>,
    challenges: ChallengeBook,
    plan_ttl: Duration,
    mcp_time_limit: Duration,
}

impl std::fmt::Debug for Executor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Executor")
            .field("plan_ttl", &self.plan_ttl)
            .field("mcp_time_limit", &self.mcp_time_limit)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Executor {
    pub fn new(gate: Arc<dyn GuardrailsGate>) -> Self {
        Self {
            gate,
            plans: Mutex::new(HashMap::new()),
            running: Arc::new(Mutex::new(HashMap::new())),
            challenges: ChallengeBook::default(),
            plan_ttl: Duration::from_millis(PLAN_TTL_MS),
            mcp_time_limit: Duration::from_millis(MCP_TIME_LIMIT_MS),
        }
    }

    /// Shorter lifetimes, for tests.
    pub fn with_limits(mut self, plan_ttl: Duration, mcp_time_limit: Duration) -> Self {
        self.plan_ttl = plan_ttl;
        self.mcp_time_limit = mcp_time_limit;
        self
    }

    fn close(&self, plan: &StoredPlan, close: PlanClose) {
        if let Some(req) = plan.gate_request() {
            self.gate.record_close(&req, close);
        }
    }

    /// Drops expired plans, each with its record.
    fn purge(&self) {
        let now = Instant::now();
        let expired: Vec<StoredPlan> = {
            let mut plans = lock(&self.plans);
            let ids: Vec<String> = plans
                .iter()
                .filter(|(_, p)| now.duration_since(p.created) >= self.plan_ttl)
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter().filter_map(|id| plans.remove(id)).collect()
        };
        for p in &expired {
            self.close(p, PlanClose::Dropped);
        }
    }

    /// The connection closed: its plans go, each with its record.
    pub fn drop_connection(&self, connection: u64) {
        let dropped: Vec<StoredPlan> = {
            let mut plans = lock(&self.plans);
            let ids: Vec<String> = plans
                .iter()
                .filter(|(_, p)| p.caller.connection == connection)
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter().filter_map(|id| plans.remove(id)).collect()
        };
        for p in &dropped {
            self.close(p, PlanClose::Dropped);
        }
    }

    /// Live plans (for tests and diagnostics).
    pub fn live_plans(&self) -> usize {
        self.purge();
        lock(&self.plans).len()
    }

    /// Builds the plan of `op` once: the common preconditions, the operation's own part, the
    /// permission rule and the fingerprint.
    #[allow(clippy::too_many_arguments)]
    fn plan(
        &self,
        backend: &dyn ProtectedBackend,
        op: OperationId,
        args: &OperationArgs,
        repo: &RepoHandle,
        who: &Who,
        layer: Layer,
        session_env: &SessionEnv,
    ) -> Result<(RepoFacts, OpPlan, bool, [u8; 32]), ExecError> {
        let facts = backend.facts(repo)?;
        check_common(op, &facts)?;
        let op_plan = match backend.plan_op(op, args, repo, &facts) {
            Ok(p) => p,
            Err(PlanError::NotImplemented) => {
                return Err(ExecError::NotImplemented(entry(op).owner));
            }
            Err(PlanError::Rejected(r)) => return Err(r.into()),
        };
        if op_plan.other_session && layer == Layer::Mcp && op == OperationId::RebaseOntoBase {
            return Err(RejectReason::OtherSessionPresent.into());
        }
        let confirm = permission(who, layer, &op_plan.affected)?;
        let fp = fingerprint(op, args, layer, who, repo, &facts, &op_plan, session_env);
        Ok((facts, op_plan, confirm, fp))
    }

    /// The first phase: no effects, no oplog entry.
    pub fn prepare(
        &self,
        backend: &dyn ProtectedBackend,
        input: PrepareInput<'_>,
    ) -> Result<Prepared, ExecError> {
        let r = input.resolution;
        if r.via == ResolvedVia::Executor {
            return Err(RejectReason::ExecutorDescendant.into());
        }
        let op = input.params.operation;
        admit(op, input.layer, &r.who)?;
        let args = OperationArgs::parse(op, &input.params.args, input.caller.mcp)
            .map_err(|e| ExecError::Invalid(e.message()))?;
        let declared: Vec<(String, String)> = input
            .params
            .session_env
            .iter()
            .map(|v| (v.name.name().to_owned(), v.value.clone()))
            .collect();
        // Excluded from PATH: the repo's Git directory and its worktree (M-01).
        let excluded = backend
            .facts(&input.repo)
            .map(|f| vec![f.common_dir, f.root])
            .unwrap_or_default();
        let (session_env, diags) = validate_session_env(&declared, &excluded);

        let (facts, op_plan, confirm, fp) = self.plan(
            backend,
            op,
            &args,
            &input.repo,
            &r.who,
            input.layer,
            &session_env,
        )?;
        let mut warnings = op_plan.warnings.clone();
        if op_plan.other_session && !warnings.contains(&WarningCode::ActiveSession) {
            warnings.push(WarningCode::ActiveSession);
        }
        if confirm && !warnings.contains(&WarningCode::OtherActorsWork) {
            warnings.push(WarningCode::OtherActorsWork);
        }
        warnings.sort();

        self.purge();
        let open = lock(&self.plans)
            .values()
            .filter(|p| p.caller.connection == input.caller.connection)
            .count();
        if open >= MAX_PLANS_PER_CONNECTION {
            return Err(ExecError::TooManyPlans);
        }

        let challenge = if confirm {
            let binding = Binding {
                connection: input.caller.connection,
                pid: input.caller.pid,
                start_us: input.caller.start_us,
                plan_hash: fp,
            };
            match self
                .challenges
                .issue(binding, input.confirm_refusal, Instant::now())
            {
                Ok(token) => Some(token),
                Err(ChallengeError::NotEligible(_)) => {
                    return Err(RejectReason::ForeignWork.into());
                }
                Err(_) => return Err(RejectReason::ChallengeInvalid.into()),
            }
        } else {
            None
        };

        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| ExecError::Invalid("no randomness".into()))?;
        let plan_id = hex(&id);
        let stored = StoredPlan {
            caller: input.caller,
            who: r.who.clone(),
            layer: input.layer,
            channel: input.channel,
            operation: op,
            args,
            repo: input.repo,
            facts,
            fingerprint: fp,
            warnings: warnings.clone(),
            challenge: challenge.is_some(),
            session_env,
            created: Instant::now(),
        };
        let decision = match stored.gate_request() {
            None => DecisionView::NotGoverned,
            Some(req) => match self.gate.evaluate(&req) {
                GateDecision::Allow => DecisionView::Allow,
                GateDecision::Deny => DecisionView::Deny,
                GateDecision::NotEvaluated => DecisionView::NotEvaluated,
            },
        };
        lock(&self.plans).insert(plan_id.clone(), stored);
        let _ = op_plan;
        Ok(Prepared {
            plan_id,
            fingerprint: hex(&fp),
            layer: input.layer,
            warnings,
            decision,
            challenge,
            expires_in_ms: u64::try_from(self.plan_ttl.as_millis()).unwrap_or(u64::MAX),
            diagnostics: diags
                .into_iter()
                .map(|d| Diagnostic {
                    code: d.code.to_owned(),
                    subject: Untrusted::new(d.var),
                })
                .collect(),
        })
    }

    /// Takes the plan for a run: consumed by any attempt of its own connection.
    fn take(&self, caller: Caller, plan_id: &str) -> Result<StoredPlan, ExecError> {
        self.purge();
        let mut plans = lock(&self.plans);
        match plans.get(plan_id) {
            Some(p) if p.caller.connection == caller.connection => {}
            _ => return Err(RejectReason::PlanUnknown.into()),
        }
        plans
            .remove(plan_id)
            .ok_or(ExecError::Rejected(RejectReason::PlanUnknown))
    }

    /// The second phase.
    pub fn run(
        &self,
        backend: &dyn ProtectedBackend,
        input: RunInput<'_>,
        env: &RunEnv<'_>,
    ) -> Result<Executed, ExecError> {
        // H-01: a descendant of the executor never waits for the lock its own plan holds.
        if input.resolution.via == ResolvedVia::Executor {
            return Err(RejectReason::ExecutorDescendant.into());
        }
        let plan = self.take(input.caller, &input.params.plan_id)?;
        let refuse = |plan: &StoredPlan, r: RejectReason| {
            self.close(plan, PlanClose::Rejected);
            Err(ExecError::Rejected(r))
        };
        let mut accepted = input.params.accepted_warnings.clone();
        accepted.sort();
        accepted.dedup();
        if accepted != plan.warnings {
            return refuse(&plan, RejectReason::WarningsMismatch);
        }
        if plan.challenge {
            let Some(token) = input.params.confirmation.as_deref() else {
                return refuse(&plan, RejectReason::ConfirmationRequired);
            };
            let refusal = match (input.resolve_again)() {
                Ok((_, _, refusal)) => refusal,
                Err(_) => Some(RefusalReason::IdentityUnverified),
            };
            let binding = Binding {
                connection: plan.caller.connection,
                pid: input.caller.pid,
                start_us: input.caller.start_us,
                plan_hash: plan.fingerprint,
            };
            if self
                .challenges
                .redeem(binding, token, refusal, Instant::now())
                .is_err()
            {
                return refuse(&plan, RejectReason::ChallengeInvalid);
            }
        }

        // The repo's write lock, in arrival order (Q-CKP-19).
        let event =
            |position: Option<u32>, operation_id: Option<String>, outcome| OperationEventData {
                repo_id: plan.repo.repo_id.clone(),
                operation: plan.operation,
                layer: plan.layer,
                operation_id,
                position,
                outcome,
            };
        let key = backend.write_lock_key(&plan.repo);
        let stopping = || env.stopping.load(std::sync::atomic::Ordering::SeqCst);
        let guard = match repo_lock::lock_queued(&key, MAX_QUEUED_PER_REPO, &stopping, |ahead| {
            (env.publish)(
                gitraptor_api::event::OPERATION_QUEUED,
                event(Some(ahead), None, None),
            );
        }) {
            Ok(g) => g,
            Err(QueueError::Full) => return refuse(&plan, RejectReason::QueueFull),
            Err(QueueError::Cancelled) => return refuse(&plan, RejectReason::DaemonStopping),
        };

        // Under the lock: the requester again, the plan again, identity and fingerprint.
        let (again, layer_now, _) = match (input.resolve_again)() {
            Ok(r) => r,
            Err(e) => {
                self.close(&plan, PlanClose::Rejected);
                return Err(e);
            }
        };
        if again.via == ResolvedVia::Executor {
            return refuse(&plan, RejectReason::ExecutorDescendant);
        }
        if again.who != plan.who || layer_now != plan.layer {
            return refuse(&plan, RejectReason::StateChanged);
        }
        let now_facts = match backend.facts(&plan.repo) {
            Ok(f) => f,
            Err(r) => return refuse(&plan, r),
        };
        if !plan.facts.same_identity(&now_facts) {
            return refuse(&plan, RejectReason::RepoIdentityChanged);
        }
        let (facts, op_plan, _, fp) = match self.plan(
            backend,
            plan.operation,
            &plan.args,
            &plan.repo,
            &plan.who,
            plan.layer,
            &plan.session_env,
        ) {
            Ok(p) => p,
            Err(ExecError::Rejected(r)) => {
                // A precondition that fails now means the state changed (BR-CKP-CONS-004).
                let _ = r;
                return refuse(&plan, RejectReason::StateChanged);
            }
            Err(e) => {
                self.close(&plan, PlanClose::Rejected);
                return Err(e);
            }
        };
        if fp != plan.fingerprint {
            return refuse(&plan, RejectReason::StateChanged);
        }

        // The decision that counts, before any effect (BR-CKP-WF-002).
        if let Some(req) = plan.gate_request()
            && self.gate.evaluate(&req) != GateDecision::Allow
        {
            self.close(&plan, PlanClose::Denied);
            return Err(RejectReason::GuardrailsDenied.into());
        }

        let cancel = CancelToken::default();
        let step_plan = StepPlan {
            operation: plan.operation,
            args: &plan.args,
            repo: &plan.repo,
            facts: &facts,
            op_plan: &op_plan,
            layer: plan.layer,
            session_env: &plan.session_env,
            cancel: cancel.clone(),
        };
        let inner = match backend.step(&step_plan) {
            Ok(s) => s,
            Err(e) => {
                self.close(&plan, PlanClose::Rejected);
                return Err(ExecError::Invalid(e.message));
            }
        };
        let mut step = ExecStep {
            inner,
            cancel: cancel.clone(),
            running: Arc::clone(&self.running),
            time_limit: (plan.layer == Layer::Mcp).then_some(self.mcp_time_limit),
            started: &|operation_id: &str| {
                (env.publish)(
                    gitraptor_api::event::OPERATION_STARTED,
                    event(None, Some(operation_id.to_owned()), None),
                );
            },
        };
        // The scope the step declares, every worktree checked to be one of
        // the repo's (US-TMC-001); refused before the intent is recorded.
        let mut req = match ProtectedRequest::for_step(
            &plan.repo,
            &step,
            plan.who.clone(),
            oplog_channel(plan.channel),
            0,
        ) {
            Ok(req) => req,
            Err(e) => {
                self.close(&plan, PlanClose::Rejected);
                return Err(ExecError::Scope(e));
            }
        };
        // The intent's mark, with every `git` that already ended in the
        // scope persisted by the engine: a raw `git` just before is never
        // taken for this operation's echo (US-TMC-004).
        req.engine_mark = match (env.engine_mark)(&plan.repo.repo_id, &req.worktree_paths) {
            Some(mark) => mark,
            None => return refuse(&plan, RejectReason::GitBusy),
        };
        req.confirmed = plan.challenge;
        req.warnings = plan.warnings.iter().map(|w| format!("{w:?}")).collect();
        let protected = ProtectedOperation {
            oplog: &plan.repo.oplog,
            snapshotter: Arc::clone(&plan.repo.snapshotter),
            marks: env.marks,
            procs: env.procs,
            stopping: env.stopping,
            deadline: env.prior_deadline,
        };
        let ran = protected.run(&req, &mut step);
        // The state the step left, while the repo is still held.
        if let Ok(o) = &ran {
            (env.after_step)(&plan.repo.repo_id, &req.worktree_paths, &o.operation_id);
        } else if let Err(ProtectedError::Step { operation_id, .. }) = &ran {
            (env.after_step)(&plan.repo.repo_id, &req.worktree_paths, operation_id);
        }
        drop(guard);
        let (outcome, close) = match &ran {
            Ok(o) => {
                let r = match cancel.reason() {
                    Some(Interrupt::Cancelled) => OperationOutcome::Cancelled,
                    _ => o.output.outcome.unwrap_or(OperationOutcome::Done),
                };
                (Some(o.operation_id.clone()), r)
            }
            Err(ProtectedError::Prior { operation_id, .. }) => {
                (operation_id.clone(), OperationOutcome::Aborted)
            }
            Err(ProtectedError::Step { operation_id, .. }) => (
                Some(operation_id.clone()),
                match cancel.reason() {
                    Some(Interrupt::Cancelled) => OperationOutcome::Cancelled,
                    _ => OperationOutcome::FailedChanged,
                },
            ),
            Err(ProtectedError::Oplog(_)) => (None, OperationOutcome::FailedUnchanged),
        };
        (env.publish)(
            gitraptor_api::event::OPERATION_FINISHED,
            event(None, outcome, Some(close)),
        );
        self.close(&plan, PlanClose::Ran(close));
        let done = ran.map_err(ExecError::Protected)?;
        let git_output = if plan.layer == Layer::Cockpit {
            done.output.git_output.clone()
        } else {
            None
        };
        Ok(Executed {
            outcome: done,
            result: close,
            who: plan.who,
            layer: plan.layer,
            channel: plan.channel,
            git_output,
        })
    }

    /// Cancel (BR-CKP-WF-008): only layer `cockpit`, never a descendant of the executor; only an
    /// operation this executor runs. `Ok(false)` if it is not running.
    pub fn cancel(
        &self,
        resolution: &Resolution,
        layer: Layer,
        operation_id: &str,
    ) -> Result<bool, ExecError> {
        if resolution.via == ResolvedVia::Executor {
            return Err(RejectReason::ExecutorDescendant.into());
        }
        if layer != Layer::Cockpit {
            return Err(RejectReason::NotAvailableForLayer.into());
        }
        match lock(&self.running).get(operation_id) {
            Some(op) => {
                op.cancel.cancel(Interrupt::Cancelled);
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

pub(crate) fn oplog_channel(c: RequestChannel) -> Channel {
    match c {
        RequestChannel::Cli => Channel::Cli,
        RequestChannel::Tui => Channel::Tui,
        RequestChannel::Mcp => Channel::Mcp,
        RequestChannel::Hook => Channel::Hook,
    }
}

/// The backend's step, registered as running (for Cancel) and under the time limit of layer
/// `mcp`.
struct ExecStep<'a> {
    inner: Box<dyn ProtectedStep>,
    cancel: CancelToken,
    running: Arc<Mutex<HashMap<String, RunningOp>>>,
    time_limit: Option<Duration>,
    started: &'a (dyn Fn(&str) + Sync),
}

impl ProtectedStep for ExecStep<'_> {
    fn subtype(&self) -> &str {
        self.inner.subtype()
    }

    fn scope(&self) -> StepScope {
        self.inner.scope()
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        let id = ctx.operation_id().to_owned();
        lock(&self.running).insert(
            id.clone(),
            RunningOp {
                cancel: self.cancel.clone(),
            },
        );
        (self.started)(&id);
        let _limit = self
            .time_limit
            .map(|limit| TimeLimit::start(&self.cancel, limit));
        let out = self.inner.run(ctx);
        lock(&self.running).remove(&id);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timemachine::oplog::{Requester, RequesterOrigin};
    use gitraptor_api::{AgentKind, AgentOrigin};

    fn agent(session: &str) -> Who {
        Who {
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            requester: Requester::Agent {
                name: "claude-code".into(),
                origin: RequesterOrigin::Detected,
                session_id: session.into(),
            },
        }
    }

    fn resolution(who: Who, via: ResolvedVia, confirmable: bool) -> Resolution {
        Resolution {
            who,
            via,
            executor_operation: None,
            confirmable,
        }
    }

    /// M-03 (Validación 21): the daemon fixes the layer; an agent never gets `cockpit`, nor a
    /// descendant of the executor.
    #[test]
    fn the_layer_comes_from_the_requester() {
        let human = resolution(Who::unattributed(), ResolvedVia::None, true);
        assert_eq!(layer_for(&human), Layer::Cockpit);
        let blind = resolution(Who::unattributed(), ResolvedVia::None, false);
        assert_eq!(layer_for(&blind), Layer::Mcp);
        let claude = resolution(agent("1:1"), ResolvedVia::Ancestry, true);
        assert_eq!(layer_for(&claude), Layer::Mcp);
        let child = resolution(Who::unattributed(), ResolvedVia::Executor, true);
        assert_eq!(layer_for(&child), Layer::Mcp);
    }

    /// M-03: an agent from the TUI or from raw JSON-RPC gets layer `mcp`: no merge, no discard;
    /// an unattributed caller without `cockpit` gets nothing.
    #[test]
    fn what_each_layer_may_ask() {
        let claude = agent("1:1");
        assert_eq!(
            admit(OperationId::MergeIntoBase, Layer::Mcp, &claude),
            Err(RejectReason::NotAvailableForLayer)
        );
        assert_eq!(
            admit(OperationId::DiscardWorktree, Layer::Mcp, &claude),
            Err(RejectReason::NotAvailableForLayer)
        );
        assert_eq!(admit(OperationId::Commit, Layer::Mcp, &claude), Ok(()));
        assert_eq!(
            admit(OperationId::Commit, Layer::Mcp, &Who::unattributed()),
            Err(RejectReason::UnattributedWithoutCockpit)
        );
        assert_eq!(
            admit(OperationId::Commit, Layer::Cockpit, &Who::unattributed()),
            Err(RejectReason::NotAvailableForLayer)
        );
        assert_eq!(
            admit(
                OperationId::MergeIntoBase,
                Layer::Cockpit,
                &Who::unattributed()
            ),
            Ok(())
        );
    }

    /// ADR-TMC-005 § 2 extended (ADR-CKP-002 § 3).
    #[test]
    fn the_base_permission_rule() {
        let me = agent("1:1");
        assert_eq!(permission(&me, Layer::Mcp, &Affected::Nobody), Ok(false));
        assert_eq!(
            permission(
                &me,
                Layer::Mcp,
                &Affected::Agent {
                    session_id: "1:1".into()
                }
            ),
            Ok(false)
        );
        assert_eq!(
            permission(
                &me,
                Layer::Mcp,
                &Affected::Agent {
                    session_id: "2:2".into()
                }
            ),
            Err(RejectReason::ForeignWork)
        );
        assert_eq!(
            permission(&me, Layer::Mcp, &Affected::Unattributed),
            Err(RejectReason::ForeignWork)
        );
        let human = Who::unattributed();
        assert_eq!(
            permission(&human, Layer::Cockpit, &Affected::Unattributed),
            Ok(false)
        );
        assert_eq!(
            permission(&human, Layer::Cockpit, &Affected::Other),
            Ok(true)
        );
        assert_eq!(
            permission(&human, Layer::Mcp, &Affected::Other),
            Err(RejectReason::ForeignWork)
        );
    }

    #[test]
    fn no_guardrails_never_allows() {
        let req = GateRequest {
            repo_id: "r".into(),
            operation: gitraptor_api::catalog::GovernedAs::Merge,
            layer: Layer::Cockpit,
            actor: Actor::Unattributed,
            fingerprint: String::new(),
        };
        assert_eq!(NoGuardrails.evaluate(&req), GateDecision::NotEvaluated);
    }
}
