//! The confirmation step of `undo` and `restore` (ADR-TMC-005 § 3 and § 4).
//!
//! When the base permission rule says an "unattributed" requester needs to confirm an agent's
//! work, [`Confirmation::gate`] either offers a one-use challenge bound to the connection, the
//! process and the hash of the plan the daemon computed, or redeems the token the client sends
//! back. Nothing the client says enters the hash.

use std::cell::RefCell;
use std::time::Instant;

use gitraptor_api::Actor;
use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::{TmChallenge, TmConfirmData, TmRejectReason};

use super::oplog::{Channel, Requester, Scope};
use super::protected::ChallengeBook;

/// Whether the business rule offers confirming another actor's work on this platform: Unix only.
///
/// BR-TMC-AUTH-001 rejects it on Windows without a confirmation (TQ-14, MVP), as does
/// ADR-TMC-005 § 3 and its note on parent-process spoofing (M-01).
pub const FOREIGN_WORK_CONFIRMABLE: bool = cfg!(unix);

/// Which Time Machine command is being planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanKind {
    Undo,
    Restore,
}

/// What the daemon is about to do, as built by undo or restore from its own plan.
pub struct PlanFacts<'a> {
    pub kind: PlanKind,
    /// The requested worktree root (canonical, as the plan has it).
    pub worktree: &'a str,
    pub requester: &'a Requester,
    pub channel: Channel,
    /// Undo: its target (the undone operation's prior). Restore: the point.
    pub target_snapshot: &'a str,
    /// Undo: the undone operation id or `git-event-<seq>`. Restore: `None`.
    pub undone_id: Option<&'a str>,
    /// Display only; never hashed.
    pub undone_subtype: Option<&'a str>,
    pub scope: &'a Scope,
    /// Whose work the plan takes back (undo: one; restore: every owner).
    pub owners: &'a [Requester],
}

impl PlanFacts<'_> {
    /// SHA-256 of the canonical form of the plan.
    pub fn hash(&self) -> [u8; 32] {
        todo!("canonical plan hash")
    }
}

/// What the channel adds to a `confirmation-required` answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub challenge: Option<TmChallenge>,
    pub cannot_confirm: Option<RefusalReason>,
    /// Agents only, once per session, at most `MAX_CONFIRM_OWNERS`.
    pub owners: Vec<Actor>,
    pub undone_operation_id: Option<String>,
    pub undone_subtype: Option<String>,
}

impl Offer {
    /// The answer's `data`.
    pub fn into_data(
        self,
        _reason: TmRejectReason,
        _operation_id: Option<String>,
    ) -> TmConfirmData {
        todo!("offer to wire data")
    }
}

/// How a request may confirm: the book and the caller it binds a challenge to.
#[expect(dead_code, reason = "stub until the gate is built")]
struct Offering<'a> {
    book: &'a ChallengeBook,
    connection: u64,
    pid: u32,
    start_us: u64,
    token: Option<&'a str>,
    eligibility: &'a dyn Fn() -> Option<RefusalReason>,
}

/// The confirmation side of one Time Machine request, built by the channel.
#[expect(dead_code, reason = "stub until the gate is built")]
pub struct Confirmation<'a> {
    offering: Option<Offering<'a>>,
    /// Whether the business rule allows confirming foreign work here.
    rule_allows: bool,
    offer: RefCell<Option<Offer>>,
}

impl<'a> Confirmation<'a> {
    /// Never offered: MCP, a connection without the capability. The gate is the base rule alone
    /// (a token, if any, is `ChallengeInvalid`).
    pub fn unavailable() -> Confirmation<'static> {
        todo!("a confirmation that is never offered")
    }

    /// A full connection with the capability. `eligibility` runs the confirmation checks *now*;
    /// the gate calls it only when it issues or redeems.
    pub fn offered(
        _book: &'a ChallengeBook,
        _connection: u64,
        _pid: u32,
        _start_us: u64,
        _token: Option<&'a str>,
        _eligibility: &'a dyn Fn() -> Option<RefusalReason>,
    ) -> Self {
        todo!("a confirmation that may be offered")
    }

    /// Overrides [`FOREIGN_WORK_CONFIRMABLE`]: production never calls it; tests and the channel's
    /// test seam do, so both branches of the rule run on every OS.
    #[must_use]
    pub fn with_rule_allows(self, _allows: bool) -> Self {
        todo!("injectable business rule")
    }

    pub fn is_offered(&self) -> bool {
        todo!("whether the request may confirm")
    }

    /// The confirmation step, right after the base rule. `Ok(true)`: a challenge of this very
    /// plan was redeemed; `Ok(false)`: nothing to confirm; `Err(reason)`: rejected.
    pub fn gate(
        &self,
        base: Result<(), TmRejectReason>,
        plan: &PlanFacts<'_>,
    ) -> Result<bool, TmRejectReason> {
        self.gate_at(base, plan, Instant::now())
    }

    /// What `gate` left for the answer; `None` if it offered nothing.
    pub fn take_offer(&self) -> Option<Offer> {
        todo!("the offer of the last gate")
    }

    /// [`Self::gate`] on an injected clock, for the tests.
    fn gate_at(
        &self,
        _base: Result<(), TmRejectReason>,
        _plan: &PlanFacts<'_>,
        _now: Instant,
    ) -> Result<bool, TmRejectReason> {
        todo!("the gate table")
    }
}

#[cfg(test)]
mod tests;
