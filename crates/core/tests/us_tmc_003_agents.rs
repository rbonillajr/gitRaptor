//! US-TMC-003 with agents: `timemachine.redo` of the undo of an agent's work. A real daemon in this
//! process with the engine, real Git, temporary repos and profile (NFR-01). Debug builds only (the
//! harness uses the confirmation seam).
//!
//! Independent of the Claude Code session that runs the tests: the daemon knows one agent
//! executable, `raptor-fake-agent`; "claude-1" and "claude-2" are two such shell processes running
//! `git` in the worktree (see `us_tmc_011_common`), and each test asserts its actors before relying
//! on an owner. The developer is the test's own client, which the daemon sees as unattributed.
//! The redo is asked through `methods::TM_REDO` with JSON values. Undos of an agent's work are undos
//! by agent: the world has no captured point before a raw commit, which a plain undo needs.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_011_common;

use gitraptor_api::rpc::code;
use gitraptor_api::{Actor, AgentKind, methods};
use gitraptor_core::timemachine::oplog::OperationKind;
use serde_json::Value;
use us_tmc_011_common::*;

fn assert_is_claude_code(w: &World, session: &str) {
    assert!(
        matches!(
            w.session(session).actor,
            Actor::Agent {
                kind: AgentKind::ClaudeCode,
                ..
            }
        ),
        "the session {session} is a Claude Code one"
    );
}

/// An undo by `agent` itself over its own work (`agent` selects its own session): `{ok}` or
/// `{code, data}` as the agent's process saw it.
fn self_undo(w: &World, agent: &mut Agent, since: Option<&str>) -> Value {
    let session = agent.session.clone();
    w.as_agent(
        agent,
        methods::TM_UNDO,
        w.undo_params(Some(&session), since, None),
    )
}

fn redo_as(w: &World, agent: &mut Agent) -> Value {
    w.as_agent(agent, methods::TM_REDO, params(&w.worktree, None))
}

fn ok(answer: &Value, what: &str) -> Value {
    answer
        .get("ok")
        .cloned()
        .unwrap_or_else(|| panic!("{what} must be done: {answer}"))
}

fn reason(answer: &Value) -> &Value {
    assert_eq!(
        answer["code"],
        code::OPERATION_REJECTED,
        "the request must be rejected: {answer}"
    );
    &answer["data"]["reason"]
}

// ----- Escenario 5 -----------------------------------------------------------------------------

/// Escenario 5: claude-1 undid an operation of its own that touched `b.rs` and `c.rs`; then an
/// unattributed change modified `b.rs`. Its redo stops whole on `overlap` (naming `b.rs` and the
/// unattributed actor) and the worktree, `c.rs` included, stays as it was.
#[test]
fn a_later_unattributed_change_stops_the_agents_redo() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(
        &mut claude1,
        &[("b.rs", "fn b() {}"), ("c.rs", "fn c() {}")],
        "claude-1 b and c",
    );
    ok(
        &self_undo(&w, &mut claude1, Some("20m")),
        "the agent's own undo",
    );
    assert!(
        !w.worktree.join("c.rs").exists(),
        "the undo took the agent's work away"
    );
    // After the undo the unattributed developer writes `b.rs` anew.
    w.write("b.rs", "fn b() { developer(); }\n");
    let before = w.fx.fingerprint();

    let answer = redo_as(&w, &mut claude1);

    assert_ne!(
        answer["code"],
        code::NOT_IMPLEMENTED,
        "TMC003-S5: the redo must be served: {answer}"
    );
    assert_eq!(reason(&answer), "overlap", "TMC003-S5: {answer}");
    let overlap = answer["data"]["overlap"].to_string();
    assert!(
        overlap.contains("b.rs"),
        "TMC003-S5: the overlap names b.rs: {overlap}"
    );
    assert!(
        overlap.contains("unattributed"),
        "TMC003-S5: the overlap names the unattributed actor: {overlap}"
    );
    assert_intact(&w, &before);
    assert_eq!(
        w.read("b.rs"),
        "fn b() { developer(); }\n",
        "TMC003-S5: the developer's b.rs is as they left it"
    );
    assert!(
        !w.worktree.join("c.rs").exists(),
        "TMC003-S5: the redo is refused whole: c.rs does not come back"
    );
}

// ----- Escenario 6 -----------------------------------------------------------------------------

/// Escenario 6: claude-2 asks to redo the undo that claude-1 made of its own work: `other-actor`,
/// and the repo does not change.
#[test]
fn an_agent_cannot_redo_another_actors_undo() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let mut claude2 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    assert_is_claude_code(&w, &claude2.session);
    assert_ne!(claude1.session, claude2.session);
    w.agent_commit(&mut claude1, &[("b.rs", "fn b() {}")], "claude-1 b");
    ok(
        &self_undo(&w, &mut claude1, Some("20m")),
        "claude-1's own undo",
    );
    let before = w.fx.fingerprint();

    let answer = redo_as(&w, &mut claude2);

    assert_ne!(
        answer["code"],
        code::NOT_IMPLEMENTED,
        "TMC003-S6: the redo must be served: {answer}"
    );
    assert_eq!(
        reason(&answer),
        "other-actor",
        "TMC003-S6: an agent does not redo another actor's undo: {answer}"
    );
    assert_intact(&w, &before);
}

/// Escenario 6 as the story words it: the undo is the developer's, of claude-2's work; claude-1
/// asks to redo it: `other-actor`.
#[test]
fn an_agent_cannot_redo_an_undo_of_another_agents_work() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let mut claude2 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    assert_is_claude_code(&w, &claude2.session);
    w.agent_commit(&mut claude2, &[("b.rs", "fn b() {}")], "claude-2 b");
    w.confirmed_agent_undo(&claude2.session);
    let before = w.fx.fingerprint();

    let answer = redo_as(&w, &mut claude1);

    assert_ne!(
        answer["code"],
        code::NOT_IMPLEMENTED,
        "TMC003-S6: the redo must be served: {answer}"
    );
    assert_eq!(reason(&answer), "other-actor", "TMC003-S6: {answer}");
    assert_intact(&w, &before);
}

// ----- ADR-TMC-005 § 3 -------------------------------------------------------------------------

/// An unattributed requester redoing the undo of an agent's work must confirm; with the token the
/// redo is done.
#[test]
fn an_unattributed_redo_of_an_agents_work_needs_confirmation() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("b.rs", "fn b() {}")], "claude-1 b");
    let with_work = w.head();
    w.confirmed_agent_undo(&claude1.session);
    let after_undo = w.fx.fingerprint();
    let mut c = w.client();

    let asked = confirm_data(w.redo_call(&mut c, None));

    assert_eq!(
        serde_json::to_value(asked.reason).unwrap(),
        "confirmation-required",
        "TMC003-CONFIRM-AGENT: an unattributed redo of an agent's work asks to confirm"
    );
    assert_intact(&w, &after_undo);
    let token = token_of(&asked);

    let done = w
        .redo_call(&mut c, Some(&token))
        .unwrap_or_else(|e| panic!("TMC003-CONFIRM-AGENT: the confirmed redo failed: {e:?}"));

    assert!(done.get("redone_undo_id").is_some(), "{done}");
    assert_eq!(
        w.head(),
        with_work,
        "TMC003-CONFIRM-AGENT: with the token the redo is done"
    );
    let redos = w.requests(OperationKind::Redo);
    assert!(
        redos
            .iter()
            .any(|r| r.states.last().is_some_and(|s| s == "finished")),
        "TMC003-CONFIRM-AGENT: a finished redo in the oplog"
    );
}

/// The token of an undo's challenge does not confirm a redo.
#[test]
fn a_redo_token_is_not_an_undo_token() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("b.rs", "fn b() {}")], "claude-1 b");
    let mut c = w.client();
    // The challenge of an undo of claude-1's work, left unredeemed.
    let undo_challenge = confirm_data(w.undo_call(&mut c, &claude1.session, Some("20m"), None));
    let undo_token = token_of(&undo_challenge);
    // Another connection takes that work back, so there is a redo pending.
    w.confirmed_agent_undo(&claude1.session);
    let after_undo = w.fx.fingerprint();

    let (error, data) = rejected(w.redo_call(&mut c, Some(&undo_token)));

    assert_ne!(
        error,
        code::NOT_IMPLEMENTED,
        "TMC003-CONFIRM-AGENT: the redo must be served"
    );
    assert_eq!(error, code::OPERATION_REJECTED);
    assert_eq!(
        data.expect("data")["reason"],
        "challenge-invalid",
        "TMC003-CONFIRM-AGENT: an undo token does not confirm a redo"
    );
    assert_intact(&w, &after_undo);
}

// ----- NFR-01: never over another actor's later work --------------------------------------------

/// After claude-1's undo of its own work, the unattributed developer changes `docs.md`, which that
/// undo did not touch: claude-1's redo is done, `docs.md` keeps the developer's change and the
/// result lists it as kept.
#[test]
fn a_redo_keeps_another_actors_later_change() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("b.rs", "fn b() {}")], "claude-1 b");
    let with_work = w.head();
    ok(&self_undo(&w, &mut claude1, None), "the agent's own undo");
    w.write("docs.md", "docs, edited by the developer\n");

    let redone = ok(&redo_as(&w, &mut claude1), "TMC003-KEPT: the redo");

    assert_kept_docs(&w, &redone, &with_work);
}

/// The same after an undo by agent over a period (US-TMC-011), whose redo restores the undo's
/// prior in full: the developer's later change to `docs.md` is not overwritten.
#[test]
fn a_redo_of_an_agent_undo_keeps_another_actors_later_change() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("b.rs", "fn b() {}")], "claude-1 b");
    w.agent_commit(&mut claude1, &[("c.rs", "fn c() {}")], "claude-1 c");
    let with_work = w.head();
    ok(
        &self_undo(&w, &mut claude1, Some("20m")),
        "the agent's own undo",
    );
    w.write("docs.md", "docs, edited by the developer\n");

    let redone = ok(&redo_as(&w, &mut claude1), "TMC003-KEPT: the redo");

    assert_kept_docs(&w, &redone, &with_work);
}

fn assert_kept_docs(w: &World, redone: &Value, with_work: &str) {
    assert_eq!(
        w.read("docs.md"),
        "docs, edited by the developer\n",
        "TMC003-KEPT: docs.md keeps the other actor's change"
    );
    assert_eq!(
        w.head(),
        with_work,
        "TMC003-KEPT: the redo brought the agent's work back"
    );
    let kept = redone["kept"]
        .as_array()
        .unwrap_or_else(|| panic!("TMC003-KEPT: the result lists the kept paths: {redone}"));
    assert!(
        kept.iter()
            .any(|k| k["path"].to_string().contains("docs.md")),
        "TMC003-KEPT: docs.md is in kept: {redone}"
    );
    // `kept_total` is only there when `kept` is cut; either way it is not less than what is listed.
    let total = redone
        .get("kept_total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    assert!(
        total == 0 || total >= kept.len() as u64,
        "TMC003-KEPT: kept_total is 0 (omitted) or the whole count: {redone}"
    );
}
