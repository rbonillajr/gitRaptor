//! TMC003-CONFIRM (T003 of DS-US-TMC-003): `PlanKind::Redo` has its own tag in the plan hash, so
//! a token of an undo never confirms a redo. Child module of `confirm`, mounted there as
//! `mod redo_tests` (`use super::*`).
//!
//! Needs: `PlanKind::Redo` (not written yet).

use std::time::Instant;

use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::TmRejectReason::{ChallengeInvalid, ConfirmationRequired};

use super::*;
use crate::timemachine::oplog::RequesterOrigin;

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn scope() -> Scope {
    Scope {
        worktrees: vec!["/repo/feat-login".into()],
        refs: vec!["refs/heads/feat-login".into()],
    }
}

fn facts<'a>(
    kind: PlanKind,
    requester: &'a Requester,
    scope: &'a Scope,
    owners: &'a [Requester],
) -> PlanFacts<'a> {
    PlanFacts {
        kind,
        worktree: "/repo/feat-login",
        requester,
        channel: Channel::Cli,
        target_snapshot: "snap-1",
        undone_id: Some("op-1"),
        undone_subtype: Some("undo"),
        scope,
        owners,
    }
}

fn eligible() -> Option<RefusalReason> {
    None
}

#[test]
fn a_redo_plan_never_hashes_like_an_undo_with_the_same_facts() {
    let who = Requester::Unattributed;
    let scope = scope();
    let owners = [agent("4242:1")];
    let undo = facts(PlanKind::Undo, &who, &scope, &owners).hash();
    let redo = facts(PlanKind::Redo, &who, &scope, &owners).hash();
    let restore = facts(PlanKind::Restore, &who, &scope, &owners).hash();
    assert_ne!(
        undo, redo,
        "TMC003-CONFIRM: PlanKind::Redo must have its own tag in the hash"
    );
    assert_ne!(
        restore, redo,
        "TMC003-CONFIRM: a redo does not hash like a restore either"
    );
    assert_eq!(
        redo,
        facts(PlanKind::Redo, &who, &scope, &owners).hash(),
        "TMC003-CONFIRM: the redo hash is stable"
    );
    assert_ne!(
        facts(PlanKind::Redo, &who, &scope, &owners).hash_over(&["x"]),
        redo,
        "TMC003-CONFIRM: taken_back still enters the redo hash"
    );
}

#[test]
fn a_redo_token_does_not_confirm_an_undo() {
    let book = ChallengeBook::default();
    let who = Requester::Unattributed;
    let scope = scope();
    let owners = [agent("4242:1")];
    let now = Instant::now();
    let elig = eligible;

    let asking = Confirmation::offered(&book, 7, 30, 300, None, &elig).with_rule_allows(true);
    let redo = facts(PlanKind::Redo, &who, &scope, &owners);
    assert_eq!(
        asking.gate_at(Err(ConfirmationRequired), &redo, now),
        Err(ConfirmationRequired)
    );
    let token = asking
        .take_offer()
        .and_then(|o| o.challenge)
        .expect("TMC003-CONFIRM: a redo of an agent's work is offered a challenge")
        .token;

    let undo = facts(PlanKind::Undo, &who, &scope, &owners);
    let redeem =
        Confirmation::offered(&book, 7, 30, 300, Some(&token), &elig).with_rule_allows(true);
    assert_eq!(
        redeem.gate_at(Err(ConfirmationRequired), &undo, now),
        Err(ChallengeInvalid),
        "TMC003-CONFIRM: a redo token must not confirm an undo with the same facts"
    );
}
