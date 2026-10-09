//! The confirmation step of `undo` and `restore` (ADR-TMC-005 § 3 and § 4).
//!
//! When the base permission rule says an "unattributed" requester needs to confirm an agent's
//! work, [`Confirmation::gate`] either offers a one-use challenge bound to the connection, the
//! process and the hash of the plan the daemon computed, or redeems the token the client sends
//! back. Nothing the client says enters the hash.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::time::Instant;

use gitraptor_api::Actor;
use gitraptor_api::Untrusted;
use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::{MAX_CONFIRM_OWNERS, TmChallenge, TmConfirmData, TmRejectReason};

use super::oplog::{Channel, Requester, Scope};
use super::protected::{Binding, CHALLENGE_TTL, ChallengeBook, plan_hash};
use super::timeline::recorded_actor;

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

impl PlanKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Restore => "restore",
        }
    }
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

/// How an actor enters the hash and the owners list: by session, never by display name.
fn actor_key(requester: &Requester) -> String {
    match requester {
        Requester::Agent { session_id, .. } => format!("agent:{session_id}"),
        Requester::Unattributed => "unattributed".to_owned(),
    }
}

/// Appends a length-prefixed field, so no two plans share an encoding.
fn field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn set<'a>(out: &mut Vec<u8>, items: impl Iterator<Item = &'a str>) {
    let sorted: BTreeSet<&str> = items.collect();
    out.extend_from_slice(&(sorted.len() as u64).to_be_bytes());
    for item in sorted {
        field(out, item.as_bytes());
    }
}

impl PlanFacts<'_> {
    /// SHA-256 of the canonical form of the plan: versioned, length-prefixed, with the lists
    /// sorted and without duplicates. Names and subtypes (text from agents) do not enter.
    pub fn hash(&self) -> [u8; 32] {
        self.hash_over(&[])
    }

    /// [`Self::hash`] over the operations and raw Git events the plan takes back besides
    /// `undone_id` (a restore takes back everything after its point): another one, by the same
    /// owner, is another plan.
    pub fn hash_over(&self, taken_back: &[&str]) -> [u8; 32] {
        let mut out = Vec::with_capacity(256);
        field(&mut out, b"gitraptor-confirm-plan/1");
        field(&mut out, self.kind.tag().as_bytes());
        field(&mut out, self.worktree.as_bytes());
        field(&mut out, actor_key(self.requester).as_bytes());
        field(&mut out, self.channel.as_str().as_bytes());
        field(&mut out, self.target_snapshot.as_bytes());
        match self.undone_id {
            Some(id) => {
                out.push(1);
                field(&mut out, id.as_bytes());
            }
            None => out.push(0),
        }
        set(&mut out, self.scope.worktrees.iter().map(String::as_str));
        set(&mut out, self.scope.refs.iter().map(String::as_str));
        let owners: Vec<String> = self.owners.iter().map(actor_key).collect();
        set(&mut out, owners.iter().map(String::as_str));
        set(&mut out, taken_back.iter().copied());
        plan_hash(&out)
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
    pub fn into_data(self, reason: TmRejectReason, operation_id: Option<String>) -> TmConfirmData {
        TmConfirmData {
            challenge: self.challenge,
            cannot_confirm: self.cannot_confirm,
            owners: self.owners,
            undone_operation_id: self.undone_operation_id,
            undone_subtype: self.undone_subtype.map(Untrusted::new),
            ..TmConfirmData::rejected(reason, operation_id)
        }
    }
}

/// How a request may confirm: the book and the caller it binds a challenge to.
struct Offering<'a> {
    book: &'a ChallengeBook,
    connection: u64,
    pid: u32,
    start_us: u64,
    token: Option<&'a str>,
    eligibility: &'a dyn Fn() -> Option<RefusalReason>,
}

/// The confirmation side of one Time Machine request, built by the channel.
pub struct Confirmation<'a> {
    offering: Option<Offering<'a>>,
    /// Whether the business rule allows confirming foreign work here.
    rule_allows: bool,
    offer: RefCell<Option<Offer>>,
}

impl<'a> Confirmation<'a> {
    /// Never offered: MCP, a connection without the capability. The gate is the base rule alone.
    pub fn unavailable() -> Confirmation<'static> {
        Confirmation {
            offering: None,
            rule_allows: FOREIGN_WORK_CONFIRMABLE,
            offer: RefCell::new(None),
        }
    }

    /// A full connection with the capability. `eligibility` runs the confirmation checks *now*;
    /// the gate calls it only when it issues or redeems.
    pub fn offered(
        book: &'a ChallengeBook,
        connection: u64,
        pid: u32,
        start_us: u64,
        token: Option<&'a str>,
        eligibility: &'a dyn Fn() -> Option<RefusalReason>,
    ) -> Self {
        Self {
            offering: Some(Offering {
                book,
                connection,
                pid,
                start_us,
                token,
                eligibility,
            }),
            rule_allows: FOREIGN_WORK_CONFIRMABLE,
            offer: RefCell::new(None),
        }
    }

    /// Overrides [`FOREIGN_WORK_CONFIRMABLE`]: production never calls it; tests and the channel's
    /// test seam do, so both branches of the rule run on every OS.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn with_rule_allows(mut self, allows: bool) -> Self {
        self.rule_allows = allows;
        self
    }

    pub fn is_offered(&self) -> bool {
        self.offering.is_some()
    }

    /// The confirmation step, right after the base rule. `Ok(true)`: a challenge of this very
    /// plan was redeemed; `Ok(false)`: nothing to confirm; `Err(reason)`: rejected.
    pub fn gate(
        &self,
        base: Result<(), TmRejectReason>,
        plan: &PlanFacts<'_>,
    ) -> Result<bool, TmRejectReason> {
        self.gate_over(base, plan, &[])
    }

    /// [`Self::gate`] for a plan that takes back `taken_back` besides `plan.undone_id`; they
    /// enter the plan hash (see [`PlanFacts::hash_over`]).
    pub fn gate_over(
        &self,
        base: Result<(), TmRejectReason>,
        plan: &PlanFacts<'_>,
        taken_back: &[&str],
    ) -> Result<bool, TmRejectReason> {
        self.gate_at_over(base, plan, taken_back, Instant::now())
    }

    /// What `gate` left for the answer; `None` if it offered nothing.
    pub fn take_offer(&self) -> Option<Offer> {
        self.offer.borrow_mut().take()
    }

    /// [`Self::gate`] on an injected clock, for the tests.
    #[cfg(test)]
    fn gate_at(
        &self,
        base: Result<(), TmRejectReason>,
        plan: &PlanFacts<'_>,
        now: Instant,
    ) -> Result<bool, TmRejectReason> {
        self.gate_at_over(base, plan, &[], now)
    }

    fn gate_at_over(
        &self,
        base: Result<(), TmRejectReason>,
        plan: &PlanFacts<'_>,
        taken_back: &[&str],
        now: Instant,
    ) -> Result<bool, TmRejectReason> {
        use TmRejectReason::{ChallengeInvalid, ConfirmationRequired, ConfirmationUnavailable};
        let Some(offering) = &self.offering else {
            return base.map(|()| false);
        };
        let binding = Binding {
            connection: offering.connection,
            pid: offering.pid,
            start_us: offering.start_us,
            plan_hash: plan.hash_over(taken_back),
        };
        if let Some(token) = offering.token {
            // A presented token is always redeemed: it is consumed however this ends. Where the
            // rule does not offer confirmation it never validates.
            let refusal = if self.rule_allows {
                (offering.eligibility)()
            } else {
                Some(RefusalReason::Unsupported)
            };
            let redeemed = offering.book.redeem(binding, token, refusal, now);
            if let Err(reason) = base
                && reason != ConfirmationRequired
            {
                return Err(reason);
            }
            return match redeemed {
                Ok(()) => Ok(base.is_err()),
                Err(_) => Err(ChallengeInvalid),
            };
        }
        match base {
            Ok(()) => Ok(false),
            Err(reason) if reason != ConfirmationRequired => Err(reason),
            Err(_) => {
                if !self.rule_allows {
                    return Err(ConfirmationUnavailable);
                }
                let mut offer = Offer {
                    challenge: None,
                    cannot_confirm: None,
                    owners: shown_owners(plan.owners),
                    undone_operation_id: plan.undone_id.map(str::to_owned),
                    undone_subtype: plan.undone_subtype.map(str::to_owned),
                };
                match (offering.eligibility)() {
                    Some(why) => offer.cannot_confirm = Some(why),
                    None => {
                        // No randomness: no challenge and no reason (fail closed).
                        if let Ok(token) = offering.book.issue(binding, None, now) {
                            offer.challenge = Some(TmChallenge {
                                token,
                                expires_in_ms: u64::try_from(CHALLENGE_TTL.as_millis())
                                    .unwrap_or(u64::MAX),
                            });
                        }
                    }
                }
                *self.offer.borrow_mut() = Some(offer);
                Err(ConfirmationRequired)
            }
        }
    }
}

/// The agents whose work needs the confirmation, once per session, in plan order.
fn shown_owners(owners: &[Requester]) -> Vec<Actor> {
    let mut seen = BTreeSet::new();
    owners
        .iter()
        .filter(|o| matches!(o, Requester::Agent { .. }))
        .filter(|o| seen.insert(actor_key(o)))
        .take(MAX_CONFIRM_OWNERS)
        .map(recorded_actor)
        .collect()
}

#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;
