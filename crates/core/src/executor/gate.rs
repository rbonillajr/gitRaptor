//! The executor's side of Guardrails (ADR-CKP-002 § 4, DEP-CKP-10).
//!
//! The decision engine and its record are TS-CKP-003's: until then the executor talks to them
//! through [`GuardrailsGate`]. Production has [`NoGuardrails`], which evaluates nothing and is
//! fail-closed: a governed operation is refused at run (`guardrails-denied`), never let through
//! as "allowed". "Ask for confirmation" counts as deny until the queue exists (S-GRD-9).

use gitraptor_api::Actor;
use gitraptor_api::catalog::{GovernedAs, Layer, OperationOutcome};

/// What Guardrails is asked about one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRequest {
    pub repo_id: String,
    pub operation: GovernedAs,
    pub layer: Layer,
    pub actor: Actor,
    /// Hex fingerprint of the plan, to bind the decision and its record.
    pub fingerprint: String,
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
}

/// Production until TS-CKP-003: evaluates nothing, records nothing, lets nothing governed run.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGuardrails;

impl GuardrailsGate for NoGuardrails {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::NotEvaluated
    }

    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}
