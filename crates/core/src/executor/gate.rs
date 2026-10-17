//! The executor's side of Guardrails (ADR-CKP-002 § 4, DEP-CKP-10).
//!
//! The executor talks to the decision engine and its record through [`GuardrailsGate`].
//! Production wires the Guardrails engine (`guardrails::plan_gate`); [`NoGuardrails`] is the
//! fail-closed gate for wirings without one: a governed operation is refused at run
//! (`guardrails-denied`), never let through as "allowed". "Ask for confirmation" counts as deny
//! until the queue exists.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gitraptor_api::Actor;
use gitraptor_api::catalog::{GovernedAs, Layer, OperationOutcome};
use gitraptor_api::guard::Decision;
use gitraptor_policy::guard::plan::{Expected, PlanShape};

use crate::channel::requester::Who;
use crate::guardrails::log::LogEntry;
use crate::guardrails::registry::GuardRegistry;

/// What Guardrails is asked about one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRequest {
    pub repo_id: String,
    pub operation: GovernedAs,
    pub layer: Layer,
    pub actor: Actor,
    /// Hex fingerprint of the plan, to bind the decision and its record.
    pub fingerprint: String,
    /// The plan the decision and its single record belong to.
    pub plan_id: String,
    pub common_dir: PathBuf,
    pub worktree: PathBuf,
    /// The worktree's branch, short name, when it has one.
    pub branch: Option<String>,
    /// What the plan does, as Guardrails evaluates it; `None` is denied by a real engine.
    pub shape: Option<PlanShape>,
}

/// A decision with what it was made of.
#[derive(Debug, Clone)]
pub struct Verdict {
    pub view: GateDecision,
    /// The decision that produced `view`, when an engine decided.
    pub decision: Option<Decision>,
    /// Present only when the plan is allowed: what its children inherit.
    pub binding: Option<Arc<PlanBinding>>,
}

/// An allowed plan, as its child processes inherit it.
#[derive(Debug)]
pub struct PlanBinding {
    pub plan_id: String,
    /// The decision that counted, allowed.
    pub decision: Decision,
    pub layer: Layer,
    pub who: Who,
    pub registered: Arc<[Expected]>,
    /// The first denial of a direct child that left the registered transitions.
    pub late: Mutex<Option<Decision>>,
}

/// A decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    Deny,
    /// No decision engine: never read as "allow".
    NotEvaluated,
}

/// How a plan closed, for its single record (ADR-GRD-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanClose {
    /// The decision that counts denied it.
    Denied,
    /// Refused before the decision (state changed, warnings, ...).
    Rejected,
    /// Expired, or dropped with its connection.
    Dropped,
    /// It ran, with this outcome.
    Ran(OperationOutcome),
}

/// The decision engine and its record, as the executor uses them.
pub trait GuardrailsGate: Send + Sync {
    /// Evaluates a plan. Called as preview at prepare and as the decision that counts at run.
    fn evaluate(&self, req: &GateRequest) -> GateDecision;
    /// Closes a plan: called exactly once per governed plan, to write at most one entry.
    fn record_close(&self, req: &GateRequest, close: PlanClose);
    /// Evaluates a plan and keeps what the decision was made of.
    fn evaluate_detailed(&self, req: &GateRequest) -> Verdict {
        Verdict {
            view: self.evaluate(req),
            decision: None,
            binding: None,
        }
    }
    /// Whether this gate takes real decisions (a governed arm needs one).
    fn decides(&self) -> bool {
        true
    }
    /// Gives the gate the registry and the log record of the channel; called once at mount.
    fn attach(
        &self,
        _registry: &Arc<GuardRegistry>,
        _record: Arc<dyn Fn(LogEntry) -> bool + Send + Sync>,
    ) {
    }
}

/// Evaluates nothing, records nothing, lets nothing governed run.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGuardrails;

impl GuardrailsGate for NoGuardrails {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::NotEvaluated
    }

    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}

    fn decides(&self) -> bool {
        false
    }
}
