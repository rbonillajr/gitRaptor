//! The confirmation gate against a real `ChallengeBook`, with the clock and the eligibility
//! injected: no test sleeps.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::{MAX_CONFIRM_OWNERS, TmRejectReason};

use super::*;
use crate::channel::AgentMatcher;
use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::peer::{ProcError, ProcInfo, ProcSource};
use crate::channel::requester::confirmation_refusal;
use crate::timemachine::oplog::RequesterOrigin;
use crate::timemachine::protected::CHALLENGE_TTL;
use crate::timemachine::timeline::recorded_actor;

use TmRejectReason::{ChallengeInvalid, ConfirmationRequired, ConfirmationUnavailable, OtherActor};

const CONNECTION: u64 = 7;
const PID: u32 = 30;
const START: u64 = 300;

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

/// An owned plan, to take `PlanFacts` from and to change one field at a time.
#[derive(Clone)]
struct Plan {
    kind: PlanKind,
    worktree: String,
    requester: Requester,
    channel: Channel,
    target: String,
    undone_id: Option<String>,
    undone_subtype: Option<String>,
    scope: Scope,
    owners: Vec<Requester>,
}

impl Plan {
    fn undo() -> Self {
        Self {
            kind: PlanKind::Undo,
            worktree: "/repo/feat-login".into(),
            requester: Requester::Unattributed,
            channel: Channel::Cli,
            target: "snap-1".into(),
            undone_id: Some("op-1".into()),
            undone_subtype: Some("reset-hard".into()),
            scope: Scope {
                worktrees: vec!["/repo/feat-login".into()],
                refs: vec!["refs/heads/feat-login".into()],
            },
            owners: vec![agent("4242:1")],
        }
    }

    fn facts(&self) -> PlanFacts<'_> {
        PlanFacts {
            kind: self.kind,
            worktree: &self.worktree,
            requester: &self.requester,
            channel: self.channel,
            target_snapshot: &self.target,
            undone_id: self.undone_id.as_deref(),
            undone_subtype: self.undone_subtype.as_deref(),
            scope: &self.scope,
            owners: &self.owners,
        }
    }
}

/// A confirmation that offers, with the rule of the business forced to "foreign work may be
/// confirmed": on Windows the real rule (`FOREIGN_WORK_CONFIRMABLE`) refuses it (BR-TMC-AUTH-001),
/// and these tests are about the challenge mechanics, so they run the same on every OS. The real
/// rule of each OS is checked by `the_platform_rule_decides_what_the_gate_offers`.
fn offering<'a>(
    book: &'a ChallengeBook,
    connection: u64,
    pid: u32,
    start_us: u64,
    token: Option<&'a str>,
    eligibility: &'a dyn Fn() -> Option<RefusalReason>,
) -> Confirmation<'a> {
    Confirmation::offered(book, connection, pid, start_us, token, eligibility)
        .with_rule_allows(true)
}

fn eligible() -> Option<RefusalReason> {
    None
}

/// Asks the gate for a challenge of `plan` and returns its token.
fn issue(book: &ChallengeBook, plan: &Plan, now: Instant) -> String {
    let elig = eligible;
    let c = offering(book, CONNECTION, PID, START, None, &elig);
    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ConfirmationRequired)
    );
    c.take_offer()
        .and_then(|o| o.challenge)
        .expect("a challenge was offered")
        .token
}

#[test]
fn unavailable_is_the_base_rule_alone() {
    let plan = Plan::undo();
    let c = Confirmation::unavailable();
    let now = Instant::now();
    assert!(!c.is_offered());
    assert_eq!(c.gate_at(Ok(()), &plan.facts(), now), Ok(false));
    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ConfirmationRequired)
    );
    assert_eq!(c.take_offer(), None, "nothing is offered");
    assert_eq!(
        c.gate_at(Err(OtherActor), &plan.facts(), now),
        Err(OtherActor)
    );
}

#[test]
fn an_eligible_caller_gets_a_challenge_bound_to_the_plan() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let elig = eligible;
    let c = offering(&book, CONNECTION, PID, START, None, &elig);
    assert!(c.is_offered());

    let r = c.gate_at(Err(ConfirmationRequired), &plan.facts(), Instant::now());

    assert_eq!(r, Err(ConfirmationRequired));
    let offer = c.take_offer().expect("an offer");
    let challenge = offer.challenge.expect("a challenge");
    assert_eq!(challenge.token.len(), 32);
    assert!(challenge.token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(challenge.expires_in_ms, 60_000);
    assert_eq!(offer.cannot_confirm, None);
    assert_eq!(offer.owners, vec![recorded_actor(&agent("4242:1"))]);
    assert_eq!(offer.undone_operation_id.as_deref(), Some("op-1"));
    assert_eq!(offer.undone_subtype.as_deref(), Some("reset-hard"));
}

#[test]
fn an_ineligible_caller_gets_why_and_no_challenge() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let elig = || Some(RefusalReason::NoControllingTerminal);
    let c = offering(&book, CONNECTION, PID, START, None, &elig);

    let r = c.gate_at(Err(ConfirmationRequired), &plan.facts(), now);

    assert_eq!(r, Err(ConfirmationRequired));
    let offer = c.take_offer().expect("an offer");
    assert_eq!(offer.challenge, None);
    assert_eq!(
        offer.cannot_confirm,
        Some(RefusalReason::NoControllingTerminal)
    );
    assert!(!offer.owners.is_empty(), "the owners are still listed");
    // Nothing is alive in the book: any token is invalid.
    let ok = eligible;
    let token = "0123456789abcdef0123456789abcdef";
    let redeemer = offering(&book, CONNECTION, PID, START, Some(token), &ok);
    assert_eq!(
        redeemer.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
}

#[test]
fn a_redeemed_challenge_confirms_once() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let token = issue(&book, &plan, now);
    let elig = eligible;

    let first = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        first.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Ok(true)
    );
    let again = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        again.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
}

#[test]
fn a_presented_token_must_match_the_current_plan() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let elig = eligible;

    // Another operation is on top of the stack now.
    let token = issue(&book, &plan, now);
    let mut moved = plan.clone();
    moved.undone_id = Some("op-2".into());
    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &moved.facts(), now),
        Err(ChallengeInvalid)
    );

    // The base rule no longer asks for a confirmation: the token still has to match.
    let token = issue(&book, &plan, now);
    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        c.gate_at(Ok(()), &moved.facts(), now),
        Err(ChallengeInvalid)
    );

    // Same plan, base now ok (the agent's work is gone): the token is spent and the answer is
    // that nothing needed confirming.
    let token = issue(&book, &plan, now);
    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(c.gate_at(Ok(()), &plan.facts(), now), Ok(false));
}

#[test]
fn another_actor_wins_over_the_token() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let elig = eligible;
    let token = issue(&book, &plan, now);

    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        c.gate_at(Err(OtherActor), &plan.facts(), now),
        Err(OtherActor)
    );

    // And the token was consumed.
    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
}

#[test]
fn an_expired_challenge_is_invalid() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let token = issue(&book, &plan, now);
    let elig = eligible;

    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    let later = now + CHALLENGE_TTL + Duration::from_secs(1);

    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), later),
        Err(ChallengeInvalid)
    );
}

#[test]
fn the_recheck_on_redeem_refuses_a_caller_that_became_ineligible() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let token = issue(&book, &plan, now);
    let elig = || Some(RefusalReason::AgentAncestry);

    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);

    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
}

#[test]
fn eligibility_is_not_checked_when_nothing_needs_confirming() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let calls = Cell::new(0u32);
    let elig = || {
        calls.set(calls.get() + 1);
        None
    };
    let c = offering(&book, CONNECTION, PID, START, None, &elig);

    assert_eq!(c.gate_at(Ok(()), &plan.facts(), Instant::now()), Ok(false));
    assert_eq!(
        c.gate_at(Err(OtherActor), &plan.facts(), Instant::now()),
        Err(OtherActor)
    );

    assert_eq!(calls.get(), 0, "the checks ran for nothing");
}

#[test]
fn the_plan_hash_is_canonical_and_sensitive() {
    let base = Plan::undo();
    let hash = |p: &Plan| p.facts().hash();
    let h = hash(&base);

    // The same plan said differently: owners, refs and worktrees in another order or twice, a
    // different display subtype, another agent name.
    let mut same = base.clone();
    same.owners = vec![agent("9:9"), agent("4242:1"), agent("9:9")];
    let mut reference = base.clone();
    reference.owners = vec![agent("4242:1"), agent("9:9")];
    assert_eq!(hash(&same), hash(&reference));
    let mut a = base.clone();
    a.scope = Scope {
        worktrees: vec!["/b".into(), "/a".into(), "/b".into()],
        refs: vec!["refs/heads/y".into(), "refs/heads/x".into()],
    };
    let mut b = base.clone();
    b.scope = Scope {
        worktrees: vec!["/a".into(), "/b".into()],
        refs: vec![
            "refs/heads/x".into(),
            "refs/heads/y".into(),
            "refs/heads/x".into(),
        ],
    };
    assert_eq!(hash(&a), hash(&b));
    let mut renamed = base.clone();
    renamed.undone_subtype = Some("something else".into());
    renamed.owners = vec![Requester::Agent {
        name: "renamed".into(),
        origin: RequesterOrigin::Detected,
        session_id: "4242:1".into(),
    }];
    assert_eq!(hash(&renamed), h);

    // What changes the plan changes the hash.
    let mut variants: Vec<(&str, Plan)> = Vec::new();
    let mut v = base.clone();
    v.kind = PlanKind::Restore;
    variants.push(("kind", v));
    let mut v = base.clone();
    v.worktree = "/repo/other".into();
    variants.push(("worktree", v));
    let mut v = base.clone();
    v.requester = agent("1:1");
    variants.push(("requester", v));
    let mut v = base.clone();
    v.channel = Channel::Mcp;
    variants.push(("channel", v));
    let mut v = base.clone();
    v.target = "snap-2".into();
    variants.push(("target", v));
    let mut v = base.clone();
    v.undone_id = Some("op-2".into());
    variants.push(("undone id", v));
    let mut v = base.clone();
    v.undone_id = None;
    variants.push(("no undone id", v));
    let mut v = base.clone();
    v.scope.refs.push("refs/heads/more".into());
    variants.push(("scope refs", v));
    let mut v = base.clone();
    v.scope.worktrees.push("/repo/more".into());
    variants.push(("scope worktrees", v));
    let mut v = base.clone();
    v.owners = vec![agent("4243:1")];
    variants.push(("owner session", v));
    let mut v = base.clone();
    v.owners.push(Requester::Unattributed);
    variants.push(("an unattributed owner", v));
    for (what, v) in &variants {
        assert_ne!(hash(v), h, "{what} does not change the hash");
    }
    assert_eq!(hash(&base), h, "the hash is stable");
}

#[test]
fn owners_shown_are_agents_once_and_capped() {
    let book = ChallengeBook::default();
    let mut plan = Plan::undo();
    plan.owners = vec![Requester::Unattributed, agent("1:1"), agent("1:1")];
    let elig = eligible;
    let c = offering(&book, CONNECTION, PID, START, None, &elig);
    let _ = c.gate_at(Err(ConfirmationRequired), &plan.facts(), Instant::now());
    assert_eq!(
        c.take_offer().expect("an offer").owners,
        vec![recorded_actor(&agent("1:1"))],
        "no unattributed owner, one entry per session"
    );

    let mut plan = Plan::undo();
    plan.owners = (0..MAX_CONFIRM_OWNERS + 5)
        .map(|i| agent(&format!("{i}:1")))
        .collect();
    let c = offering(&book, CONNECTION, PID, START, None, &elig);
    let _ = c.gate_at(Err(ConfirmationRequired), &plan.facts(), Instant::now());
    assert_eq!(
        c.take_offer().expect("an offer").owners.len(),
        MAX_CONFIRM_OWNERS
    );
}

#[test]
fn the_rule_matches_the_platform() {
    assert_eq!(FOREIGN_WORK_CONFIRMABLE, cfg!(unix));
}

/// The rule as built, not forced: Unix offers a challenge, Windows says `confirmation-unavailable`
/// without evaluating the eligibility (BR-TMC-AUTH-001, TQ-14).
#[test]
fn the_platform_rule_decides_what_the_gate_offers() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let elig = eligible;
    let c = Confirmation::offered(&book, CONNECTION, PID, START, None, &elig);

    let r = c.gate_at(Err(ConfirmationRequired), &plan.facts(), Instant::now());

    let has_challenge = c.take_offer().is_some_and(|o| o.challenge.is_some());
    if cfg!(unix) {
        assert_eq!(r, Err(ConfirmationRequired));
        assert!(has_challenge);
    } else {
        assert_eq!(r, Err(ConfirmationUnavailable));
        assert!(!has_challenge);
    }
}

#[test]
fn the_rule_forbids_confirming_foreign_work_where_it_is_not_offered() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let calls = Cell::new(0u32);
    let elig = || {
        calls.set(calls.get() + 1);
        None
    };
    let now = Instant::now();
    let c = offering(&book, CONNECTION, PID, START, None, &elig).with_rule_allows(false);

    let r = c.gate_at(Err(ConfirmationRequired), &plan.facts(), now);

    assert_eq!(r, Err(ConfirmationUnavailable));
    assert_eq!(calls.get(), 0, "the eligibility is not evaluated");
    let offered = c.take_offer();
    assert!(
        offered
            .as_ref()
            .and_then(|o| o.challenge.as_ref())
            .is_none(),
        "no challenge: {offered:?}"
    );
    // Nothing is alive in the book either.
    let probe = offering(
        &book,
        CONNECTION,
        PID,
        START,
        Some("0123456789abcdef0123456789abcdef"),
        &elig,
    );
    assert_eq!(
        probe.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
    // Where nothing needs confirming the rule does not matter.
    let c = offering(&book, CONNECTION, PID, START, None, &elig).with_rule_allows(false);
    assert_eq!(c.gate_at(Ok(()), &plan.facts(), now), Ok(false));
    assert_eq!(
        c.gate_at(Err(OtherActor), &plan.facts(), now),
        Err(OtherActor)
    );
}

#[test]
fn a_token_where_the_rule_forbids_is_invalid() {
    let book = ChallengeBook::default();
    let plan = Plan::undo();
    let now = Instant::now();
    let token = issue(&book, &plan, now);
    let elig = eligible;

    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig).with_rule_allows(false);
    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
    // It was consumed all the same.
    let c = offering(&book, CONNECTION, PID, START, Some(&token), &elig);
    assert_eq!(
        c.gate_at(Err(ConfirmationRequired), &plan.facts(), now),
        Err(ChallengeInvalid)
    );
}

// ----- A caller without a terminal never gets to confirm -------------------------------------

/// A synthetic process tree: `launchd` (another user's) -> zsh(20) -> raptor(30).
struct Tree(HashMap<u32, ProcInfo>);

impl ProcSource for Tree {
    fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
        if pid == 10 {
            return Err(ProcError::Denied);
        }
        self.0.get(&pid).cloned().ok_or(ProcError::Gone)
    }
    fn foreign_to(&self, pid: u32, _uid: u32) -> Option<bool> {
        (pid == 10).then_some(true)
    }
    fn pids_of(&self, _uid: u32) -> Option<Vec<u32>> {
        Some(self.0.keys().copied().collect())
    }
}

fn process(pid: u32, ppid: u32, exe: &str, start: u64, tty: bool, session: u32) -> ProcInfo {
    ProcInfo {
        pid,
        ppid,
        uid: 501,
        start_us: start,
        exe: Some(PathBuf::from(exe)),
        controlling_terminal: tty,
        session,
        pgid: pid,
        desktop_session: None,
    }
}

/// With the terminal proof injected as available (what Windows now has), a caller with no
/// console or terminal is still refused: the rule is the proof, not the platform.
#[test]
fn a_caller_without_a_terminal_is_refused_even_where_the_proof_is_available() {
    let mut procs = HashMap::new();
    procs.insert(20, process(20, 10, "/bin/zsh", 200, false, 10));
    procs.insert(30, process(30, 20, "/usr/local/bin/raptor", 300, false, 30));
    let tree = Tree(procs);
    let matcher = AgentMatcher::default();
    let checks = Checks {
        uid: 501,
        procs: &tree,
        matcher: &matcher,
        daemon: Some((70, 700)),
        marks: None,
        terminal_proof: true,
        orphans_marked: true,
    };
    let peer = AcceptedPeer {
        pid: 30,
        start_us: 300,
        accepted_us: 1_000_000,
    };

    assert!(confirmation_refusal(peer, &checks, None).is_some());
}
