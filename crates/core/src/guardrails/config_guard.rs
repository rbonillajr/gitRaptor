//! The protection of the Guardrails configuration on the daemon's side: the
//! `config.relax-ignored` notice and the downgrade for a connection without
//! `guard.config-protection`.
//!
//! stub: replaced by the implementation slice. Each function does nothing, which is today's
//! behavior.

use gitraptor_api::guard::{Decision, EvaluateParams, GuardLogResult};
use gitraptor_policy::layers::IgnoredRelaxation;

use super::log::{LogContext, LogEntry};

/// The `config.relax-ignored` notice of an evaluation: `None` without an agent actor, under the
/// executor, or with nothing ignored. Same normalized operation, decision id and effects as
/// the decision; one reason per distinct level, no params.
pub fn relax_entry(
    _params: &EvaluateParams,
    _decision: &Decision,
    _ignored: &[IgnoredRelaxation],
    _ctx: &LogContext,
) -> Option<LogEntry> {
    // stub: replaced by the implementation slice
    None
}

/// A decision for a connection without `guard.config-protection`.
pub fn legacy_decision(_decision: &mut Decision) {
    // stub: replaced by the implementation slice
}

/// A `guard.log` page for a connection without `guard.config-protection`.
pub fn legacy_log(_log: &mut GuardLogResult) {
    // stub: replaced by the implementation slice
}
