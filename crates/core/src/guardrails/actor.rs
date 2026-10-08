//! The actor of a governed operation (ADR-GRD-003 § 4, DS-US-GRD-018 D5): resolved by the daemon
//! from the hook client's ancestry, never declared by the client.
//!
//! The walk is the requester's (`crate::channel::requester`): it starts at the hook client (the
//! peer of the channel, identity `(pid, start)` checked), climbs past the `git` that ran the hook
//! (agent → shell → `git` → hook) and stops at the first agent process or at a process marked by
//! an executor operation. Anything that cannot be verified is "unattributed", and with
//! "unattributed" no authorship rule applies (BR-EDGE-004). US-GRD-005 and US-GRD-006 reuse this
//! module instead of rewriting it.
//!
//! Pending: an agent registered in the worktree (US-GRP-009) without a detected process is not
//! looked up here yet (the registrations live in the daemon's store, out of reach of the
//! connection thread).

use gitraptor_api::{Actor, AgentKind};

use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::marks::ExecutorMarks;
use crate::channel::requester;

/// The agent behind the hook client, or `None` for "unattributed".
pub fn resolve(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
) -> Option<AgentKind> {
    match requester::resolve(peer, checks, marks).ok()?.who.actor {
        Actor::Agent { kind, .. } => Some(kind),
        Actor::Unattributed => None,
    }
}

/// [`resolve`] for the decision log (US-GRD-005): the agent, and whether the client runs under
/// an operation of the executor, whose plan logs instead of the hook (ADR-GRD-006, Enmienda
/// Cockpit).
pub fn resolve_logged(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
) -> (Option<AgentKind>, bool) {
    let Ok(resolution) = requester::resolve(peer, checks, marks) else {
        return (None, false);
    };
    let agent = match resolution.who.actor {
        Actor::Agent { kind, .. } => Some(kind),
        Actor::Unattributed => None,
    };
    (agent, resolution.executor_operation.is_some())
}
