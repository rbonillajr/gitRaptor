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

use super::evaluate::AskedBy;

/// The agent behind the hook client, or `None` for "unattributed".
pub fn resolve(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
) -> Option<AgentKind> {
    resolve_for_ask(peer, checks, marks).0
}

/// [`resolve`], and who an `ask` is applied to: the agent, the person (the walk proved the
/// client unattributed) or nobody proved (the walk could not verify the client: an unreadable
/// process, a changed identity, an unvouched ancestry). Only a proved person passes an `ask`.
pub fn resolve_for_ask(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
) -> (Option<AgentKind>, AskedBy) {
    match requester::resolve(peer, checks, marks) {
        Ok(r) => match r.who.actor {
            Actor::Agent { kind, .. } => (Some(kind), AskedBy::Agent),
            Actor::Unattributed => (None, AskedBy::Person),
        },
        Err(_) => (None, AskedBy::Unproved),
    }
}

/// [`resolve`] for the decision log (US-GRD-005): the agent, and how the client relates to the
/// executor. Only a direct child of an operation inherits its plan's entry; a grandchild logs
/// its own (H-01).
pub fn resolve_logged(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
) -> (Option<AgentKind>, super::link::Link) {
    let agent = resolve(peer, checks, marks);
    let link = marks.map_or(super::link::Link::None, |m| {
        super::link::classify(peer, checks, m)
    });
    (agent, link)
}
