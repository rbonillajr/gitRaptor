//! Without a recovery point, an allowed destructive operation does not run (US-GRD-017,
//! BR-EDGE-005, NFR-01).
//!
//! A pure conversion: the daemon calls [`enforce`] between the `hook-prior` snapshot and the
//! decision log, and the hook client reuses [`degraded_reason`] and [`client_fallback`] without
//! a daemon or with one that does not know the rule, so the two modes cannot diverge.

use gitraptor_api::guard::{
    Cause, Decision, Effect, ExceptionState, GuardLogResult, Level, Operation, Reason, Rule,
};
use gitraptor_api::timemachine::{HookPriorFailure, HookPriorSnapshot};

use crate::timemachine::hook_prior::wants_prior;

/// What the daemon granted to the hook client (D11).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Granted {
    pub policies: bool,
    pub prior_snapshot: bool,
    pub snapshot_required: bool,
}

/// The cause of `system.snapshot-failed` for a failed prior: total, one to one.
pub fn cause(failure: HookPriorFailure) -> Cause {
    match failure {
        HookPriorFailure::TimeLimit => Cause::SnapshotTimeLimit,
        HookPriorFailure::NoSpace => Cause::SnapshotNoSpace,
        HookPriorFailure::QuotaExceeded => Cause::SnapshotQuotaExceeded,
        HookPriorFailure::Discarded => Cause::SnapshotDiscarded,
        HookPriorFailure::NoWorktree => Cause::SnapshotNoWorktree,
        HookPriorFailure::Unavailable => Cause::SnapshotUnavailable,
        HookPriorFailure::Internal => Cause::SnapshotInternal,
    }
}

/// An allowed decision whose prior failed becomes a denial (D5, D7).
///
/// Converted in place: the decision keeps its id, its configuration status and its prior field,
/// so the log and the client see the same decision with one more reason. With the exception
/// applied the operation still goes ahead (`appliedEffect` stays `allow`) and the reason records
/// the rule it skipped. Anything else (no prior, a complete one, a denial) is left alone.
pub fn enforce(decision: &mut Decision) {
    if let Some(HookPriorSnapshot::Failed { cause: failure }) = decision.prior_snapshot {
        deny(decision, snapshot_failed(cause(failure)));
    }
}

/// The decision for a connection without `guard.snapshot-required` (D6).
///
/// `system.snapshot-failed` reads as `system.internal-error` without a cause, and the prior field
/// goes with it; the effects are not touched, so a denial stays a denial.
pub fn legacy_decision(decision: &mut Decision) {
    let mut mapped = false;
    for reason in decision
        .reasons
        .iter_mut()
        .chain(decision.notices.iter_mut())
        .filter(|r| r.rule == Rule::SnapshotFailed)
    {
        reason.rule = Rule::InternalError;
        reason.cause = None;
        reason.params.clear();
        mapped = true;
    }
    if mapped {
        decision.prior_snapshot = None;
    }
}

/// A `guard.log` page for a connection without `guard.snapshot-required` (D6): the same entries,
/// with `system.snapshot-failed` read as `system.internal-error` without a cause.
pub fn legacy_log(log: &mut GuardLogResult) {
    for reason in log
        .entries
        .iter_mut()
        .flat_map(|e| &mut e.reasons)
        .filter(|r| r.rule == Rule::SnapshotFailed)
    {
        reason.rule = Rule::InternalError;
        reason.cause = None;
    }
}

/// The reason degraded mode adds to an allowed operation that wants a prior (D2).
pub fn degraded_reason() -> Reason {
    snapshot_failed(Cause::SnapshotUnavailable)
}

/// The hook client's answer to a daemon that does not convert (D11).
///
/// With `guard.snapshot-required` the daemon already decided. With only `guard.prior-snapshot`
/// the client converts the prior the daemon took. With neither, no prior was taken: an allowed
/// operation that wants one is denied as unavailable, as in degraded mode.
pub fn client_fallback(op: &Operation, decision: &mut Decision, granted: Granted) {
    if granted.snapshot_required {
        return;
    }
    if granted.prior_snapshot {
        enforce(decision);
    } else if wants_prior(op) {
        deny(decision, degraded_reason());
    }
}

fn snapshot_failed(cause: Cause) -> Reason {
    Reason {
        rule: Rule::SnapshotFailed,
        level: Level::System,
        cause: Some(cause),
        params: Vec::new(),
    }
}

/// Adds `reason` to an allowed decision and denies it, unless the exception is applied (D5).
fn deny(decision: &mut Decision, reason: Reason) {
    if decision.applied_effect != Effect::Allow {
        return;
    }
    decision.reasons.push(reason);
    decision.effect = Effect::Deny;
    if decision.exception != ExceptionState::Applied {
        decision.applied_effect = Effect::Deny;
        // A warning only travels with an operation that goes ahead.
        decision.notices.clear();
    }
}
