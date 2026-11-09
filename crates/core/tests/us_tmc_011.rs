//! US-TMC-011 end to end over the channel: `timemachine.undo` with `agent` (and `since`) takes
//! back only what that agent did in the period. A real daemon in this process, real Git, temporary
//! repos and profile (NFR-01). Debug builds only: the confirmation seam does not exist in release.
//!
//! Independent of the Claude Code session that runs the tests: the daemon knows one agent
//! executable, `raptor-fake-agent`; "claude-1" and "claude-2" are two such shell processes running
//! `git` in the worktree, "codex-1" a registered other agent, and their session ids come from
//! `sessions.list` (see `us_tmc_011_common`). The requester is the test's own client, which the
//! daemon sees as unattributed. Each test that relies on an owner asserts it first.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_011_common;

use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::UndoResult;
use gitraptor_api::{Actor, AgentKind, methods};
use gitraptor_core::timemachine::oplog::{OperationKind, OperationState, Target};
use gitraptor_core::timemachine::store::SnapshotStore;
use gitraptor_testkit::fingerprint::Snapshot;
use serde_json::Value;
use us_tmc_011_common::*;

const SINCE: &str = "20m";

/// The outcome of an undo by `agent` over `since`, confirmed when the daemon asks for it: the
/// result, or the code and `data` of the rejection that stopped it.
fn agent_undo(w: &World, agent: &str, since: &str) -> Result<UndoResult, (i64, Value)> {
    let mut c = w.client();
    let mut token: Option<String> = None;
    for _ in 0..2 {
        match w.undo_call(&mut c, agent, Some(since), token.as_deref()) {
            Ok(v) => return Ok(serde_json::from_value(v).expect("an UndoResult")),
            Err(e) => {
                let (error, data) = rejected(Err::<Value, _>(e));
                let data = data.unwrap_or(Value::Null);
                if data["reason"] == "confirmation-required" && token.is_none() {
                    token = Some(
                        data["challenge"]["token"]
                            .as_str()
                            .expect("a challenge to confirm with")
                            .to_owned(),
                    );
                    continue;
                }
                return Err((error, data));
            }
        }
    }
    unreachable!("the second call carries the token")
}

fn undone(w: &World, agent: &str) -> UndoResult {
    agent_undo(w, agent, SINCE).unwrap_or_else(|(error, data)| {
        panic!(
            "the agent undo (agent with since) was rejected, it should be served: {error} {data}"
        )
    })
}

fn rejection(w: &World, agent: &str) -> (i64, Value) {
    match agent_undo(w, agent, SINCE) {
        Err(r) => r,
        Ok(done) => panic!("the undo was served, it should be rejected: {done:?}"),
    }
}

fn assert_prior_verifies(w: &World, result: &UndoResult) {
    let prior = &result.prior_snapshot_id;
    let view = w.oplog().lock().unwrap().snapshot(prior).unwrap();
    assert!(
        view.is_some_and(|s| s.state.is_available()),
        "the point before the undo exists and is available"
    );
    let store = SnapshotStore::open_existing(&w.tp.dirs(), &w.repo_id)
        .unwrap()
        .expect("the daemon's store");
    assert!(
        store.verify(prior).is_ok(),
        "the point before the undo verifies"
    );
}

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

fn last_undo_target(w: &World) -> Target {
    w.requests(OperationKind::Undo)
        .pop()
        .expect("an undo in the oplog")
        .view
        .record
        .target
}

fn keys(ids: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = ids.iter().map(|i| key_of(i)).collect();
    keys.sort();
    keys
}

/// The repo before a request that must change nothing, and the proof that it did not.
fn intact(w: &World) -> Snapshot {
    w.fx.fingerprint()
}

/// T008: `agent` is served, `operation_id` is not (it stays assigned to no story).
#[test]
fn agent_is_served_and_operation_id_is_still_not_implemented() {
    let w = world();
    let mut c = w.client();

    let (error, data) = rejected(w.undo_call(&mut c, "claude-code", Some(SINCE), None));
    let known_operation = data.as_ref().expect("data")["operation_id"].clone();

    assert_ne!(
        error,
        code::NOT_IMPLEMENTED,
        "TMC011-SERVED: agent must no longer answer NOT_IMPLEMENTED"
    );
    assert_eq!(
        error,
        code::OPERATION_REJECTED,
        "TMC011-SERVED: agent together with since must be served (nothing-to-undo here), not refused"
    );
    assert_eq!(
        data.unwrap()["reason"],
        "nothing-to-undo",
        "TMC011-SERVED: with no work of that agent, the request is nothing-to-undo"
    );

    let mut p = params(&w.worktree, None);
    p["operation_id"] = known_operation;
    let (error, data) = rejected(c.call(methods::TM_UNDO, p));
    assert_eq!(error, code::NOT_IMPLEMENTED, "TMC011-SERVED: operation_id");
    assert_eq!(data.unwrap()["implemented_by"], "unassigned");
}

/// Escenario 1: two commits of claude-1 (one with its edit of `api.rs`) and an unattributed edit
/// of `README.md`; confirmed by an unattributed requester.
#[test]
fn undoes_the_agents_commits_and_edits_only() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    let base = w.head();
    w.agent_commit(&mut claude1, &[("feature.txt", "one")], "claude-1 one");
    w.agent_commit(
        &mut claude1,
        &[("api.rs", "pub fn api() { v2 }")],
        "claude-1 api",
    );
    w.write("README.md", "# demo, edited by hand\n");
    let agent_events = w.events(true);
    assert_eq!(
        agent_events.len(),
        2,
        "TMC011-S1: the timeline shows claude-1's two commits"
    );
    let before_files = (w.read("README.md"), w.read("api.rs"));
    assert_eq!(before_files.1, "pub fn api() { v2 }\n");

    let result = undone(&w, &claude1.session);

    assert_eq!(
        w.head(),
        base,
        "TMC011-S1: the branch is back before claude-1's first commit"
    );
    assert_eq!(
        w.read("api.rs"),
        "pub fn api() {}\n",
        "TMC011-S1: api.rs file"
    );
    assert_eq!(
        w.staged("api.rs"),
        "pub fn api() {}\n",
        "TMC011-S1: api.rs index"
    );
    assert!(
        !w.worktree.join("feature.txt").exists(),
        "TMC011-S1: the file claude-1 added is gone"
    );
    assert_eq!(
        w.read("README.md"),
        "# demo, edited by hand\n",
        "TMC011-S1: the unattributed edit stays"
    );
    assert_eq!(
        keys(&result.undone_operation_ids),
        keys(
            &agent_events
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>()
        ),
        "TMC011-S1: undone_operation_ids are the two commits"
    );
    assert_prior_verifies(&w, &result);
}

/// Escenario 2, `antes`: claude-2's commit precedes claude-1's first one and stays applied.
#[test]
fn another_agents_commit_stays() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let mut claude2 = w.spawn_agent();
    assert_ne!(claude1.session, claude2.session, "two sessions, two owners");
    assert_is_claude_code(&w, &claude1.session);
    assert_is_claude_code(&w, &claude2.session);
    let theirs = w.agent_commit(
        &mut claude2,
        &[("other.txt", "claude-2 file")],
        "claude-2 work",
    );
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    w.agent_commit(&mut claude1, &[("two.txt", "2")], "claude-1 two");

    let result = undone(&w, &claude1.session);

    assert_eq!(
        result.undone_operation_ids.len(),
        2,
        "TMC011-S2: claude-1's two commits"
    );
    assert_eq!(
        w.head(),
        theirs,
        "TMC011-S2: the branch is back at claude-2's commit"
    );
    assert!(
        w.log().contains(&"claude-2 work".to_owned()),
        "TMC011-S2: claude-2's commit stays"
    );
    assert_eq!(
        w.read("other.txt"),
        "claude-2 file\n",
        "TMC011-S2: its files are intact"
    );
    assert!(
        !w.worktree.join("one.txt").exists() && !w.worktree.join("two.txt").exists(),
        "TMC011-S2: claude-1's files are gone"
    );
}

/// Escenario 2, `entre`: claude-2's commit lands between claude-1's two, so the undo stops on
/// the branch (D-TMC-34) and the repo does not change.
#[test]
fn another_agents_commit_after_stops_on_the_branch() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let mut claude2 = w.spawn_agent();
    assert_ne!(claude1.session, claude2.session);
    assert_is_claude_code(&w, &claude2.session);
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    w.agent_commit(
        &mut claude2,
        &[("other.txt", "claude-2 file")],
        "claude-2 between",
    );
    w.agent_commit(&mut claude1, &[("two.txt", "2")], "claude-1 two");
    let before = intact(&w);

    let (error, data) = rejection(&w, &claude1.session);

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-S2");
    assert_eq!(
        data["reason"], "overlap",
        "TMC011-S2: the interleaved commit stops the undo"
    );
    assert!(
        data["overlap"]["conflicts"]
            .to_string()
            .contains("feat-login"),
        "TMC011-S2: the conflict names the branch: {data}"
    );
    assert_intact(&w, &before);
}

/// Escenario 3, `sigue vigente`: a session detected as Claude Code and corrected to codex-1.
#[test]
fn a_corrected_session_is_not_its_detected_agent() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "session work");
    let answer = w.correct(&claude1.session, "codex-1");
    assert!(
        answer.get("ok").is_some(),
        "the correction was accepted: {answer}"
    );
    assert!(
        matches!(&w.session(&claude1.session).actor, Actor::Agent { kind: AgentKind::Other, name: Some(n), .. } if n.raw() == "codex-1"),
        "the session is codex-1 now: {:?}",
        w.session(&claude1.session).actor
    );
    let before = intact(&w);

    let (error, data) = rejection(&w, "claude-code");

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-S3");
    assert_eq!(
        data["reason"], "nothing-to-undo",
        "TMC011-S3: the corrected session is no longer Claude Code's"
    );
    assert_intact(&w, &before);

    let result = undone(&w, "codex-1");
    assert_eq!(
        result.undone_operation_ids.len(),
        1,
        "TMC011-S3: codex-1 picks the session"
    );
    assert!(
        !w.log().contains(&"session work".to_owned()),
        "TMC011-S3: its commit is taken back"
    );
}

/// Escenario 3, `se retiró`: the correction is withdrawn before the undo.
#[test]
fn a_withdrawn_correction_brings_the_session_back() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "session work");
    let answer = w.correct(&claude1.session, "codex-1");
    assert!(
        answer.get("ok").is_some(),
        "the correction was accepted: {answer}"
    );
    let answer = w.withdraw(&claude1.session);
    assert!(
        answer.get("ok").is_some(),
        "the correction was withdrawn: {answer}"
    );
    assert_is_claude_code(&w, &claude1.session);

    let result = undone(&w, "claude-code");

    assert_eq!(
        result.undone_operation_ids.len(),
        1,
        "TMC011-S3: the session is Claude Code's again and its work is taken back"
    );
    assert!(
        !w.log().contains(&"session work".to_owned()),
        "TMC011-S3: its commit is gone"
    );
}

/// Escenario 4: commits made while the engine was stopped never enter the set.
#[test]
fn gap_commits_never_enter() {
    let mut w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.stop_daemon();
    w.agent_commit_unobserved(&mut claude1, &[("gap.txt", "in the gap")], "gap work");
    w.restart_daemon();
    let session = w.present_agent_session();
    assert_is_claude_code(&w, &session);
    w.agent_commit(&mut claude1, &[("later.txt", "after")], "agent work");

    let result = undone(&w, &session);

    assert_eq!(
        result.undone_operation_ids.len(),
        1,
        "TMC011-S4: only the commit made with the engine watching"
    );
    assert!(
        w.log().contains(&"gap work".to_owned()),
        "TMC011-S4: the gap's commit stays applied"
    );
    assert!(
        !w.log().contains(&"agent work".to_owned()),
        "TMC011-S4: the observed one is gone"
    );
    assert_eq!(w.read("gap.txt"), "in the gap\n");
}

/// Escenario 5, `other-actor`: an agent cannot undo another agent's work.
#[test]
fn an_agent_undoing_another_agent_is_rejected() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let mut claude2 = w.spawn_agent();
    assert_ne!(claude1.session, claude2.session);
    assert_is_claude_code(&w, &claude1.session);
    assert_is_claude_code(&w, &claude2.session);
    w.agent_commit(&mut claude2, &[("two.txt", "2")], "claude-2 work");
    let before = intact(&w);
    let session2 = claude2.session.clone();

    let answer = w.as_agent(
        &mut claude1,
        methods::TM_UNDO,
        w.undo_params(Some(&session2), Some(SINCE), None),
    );

    assert_eq!(
        answer["code"],
        code::OPERATION_REJECTED,
        "TMC011-S5-OTHER-ACTOR: {answer}"
    );
    assert_eq!(
        answer["data"]["reason"], "other-actor",
        "TMC011-S5-OTHER-ACTOR: an agent cannot undo another's work: {answer}"
    );
    assert_intact(&w, &before);
    let undos = w.requests(OperationKind::Undo);
    assert_eq!(
        undos.len(),
        1,
        "TMC011-S5-OTHER-ACTOR: the rejection is in the oplog"
    );
    assert_eq!(undos[0].view.state, OperationState::Rejected);
    assert_eq!(undos[0].reason.as_deref(), Some("other-actor"));
    assert!(
        matches!(&undos[0].view.record.target, Target::Undo(ids) if ids.is_empty()),
        "TMC011-S5-OTHER-ACTOR: the rejection is recorded with an empty set: {:?}",
        undos[0].view.record.target
    );
}

/// Escenario 5, `nothing-to-undo`: claude-9 has no operations in the period.
#[test]
fn an_agent_without_operations_changes_nothing() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let claude9 = w.spawn_agent();
    assert_is_claude_code(&w, &claude9.session);
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 work");
    let before = intact(&w);

    let (error, data) = rejection(&w, &claude9.session);

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-S5-NOTHING");
    assert_eq!(data["reason"], "nothing-to-undo", "TMC011-S5-NOTHING");
    assert_intact(&w, &before);
    let undos = w.requests(OperationKind::Undo);
    assert_eq!(undos.len(), 1, "the rejection is in the oplog");
    assert_eq!(undos[0].reason.as_deref(), Some("nothing-to-undo"));
}

/// D2: the set of `undo --agent X --since D` is what `timeline --agent X --since D` shows.
#[test]
fn undo_takes_what_the_timeline_shows_for_the_agent() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    w.agent_commit(&mut claude1, &[("two.txt", "2")], "claude-1 two");
    let shown = w.timeline(Some("claude-code"), SINCE);
    assert!(
        shown.entries.len() >= 2
            && shown
                .entries
                .iter()
                .all(|e| matches!(e.actor, Actor::Agent { .. })),
        "TMC011-TIMELINE-EQ: the timeline shows the agent's work: {:#?}",
        shown.entries
    );

    let result = undone(&w, "claude-code");

    assert_eq!(
        keys(&result.undone_operation_ids),
        keys(
            &shown
                .entries
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>()
        ),
        "TMC011-TIMELINE-EQ: the undo takes exactly the operations and events the timeline shows"
    );
}

/// Overlap: an edit after the agent's last commit stops the undo (no clean point exists after
/// it, since `timemachine.snapshot` is not served yet, so the test cannot force phase 1 apart).
#[test]
fn an_edit_after_the_agents_last_commit_stops() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(
        &mut claude1,
        &[("api.rs", "pub fn api() { v2 }")],
        "claude-1 api",
    );
    w.write("api.rs", "pub fn api() { by hand }\n");
    let before = intact(&w);

    let (error, data) = rejection(&w, &claude1.session);

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-OVERLAP");
    assert_eq!(
        data["reason"], "overlap",
        "TMC011-OVERLAP: the later edit stops the undo: {data}"
    );
    assert!(
        data["overlap"]["conflicts"].to_string().contains("api.rs"),
        "{data}"
    );
    assert_intact(&w, &before);
}

/// Overlap, phase 2: an edit no point captured, just before the undo (`ready` then `rejected`).
#[test]
fn an_uncaptured_edit_just_before_stops_in_phase_2() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("docs.md", "docs v2")], "claude-1 docs");
    w.write("docs.md", "docs, edited just now\n");
    let before = intact(&w);

    let (error, data) = rejection(&w, &claude1.session);

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-OVERLAP");
    assert_eq!(
        data["reason"], "overlap",
        "TMC011-OVERLAP: the uncaptured edit stops the undo: {data}"
    );
    assert_intact(&w, &before);
    let undos = w.requests(OperationKind::Undo);
    let states = &undos.last().expect("the confirmed undo").states;
    let ready = states.iter().position(|s| s == "ready");
    let rejected = states.iter().position(|s| s == "rejected");
    assert!(
        ready.is_some() && rejected.is_some() && ready < rejected,
        "TMC011-OVERLAP: phase 2 goes ready, then rejected: {states:?}"
    );
}

/// Escenario 6: a plain undo cannot go below an active selective undo.
#[test]
fn plain_undo_below_a_selective_undo_is_rejected() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.unattributed_commit(&[("mine.txt", "mine")], "unattributed work");
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    w.agent_commit(&mut claude1, &[("two.txt", "2")], "claude-1 two");
    undone(&w, &claude1.session);
    let before = intact(&w);
    let mut c = w.client();

    let (error, data) = rejected(w.plain_undo_call(&mut c, None));

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-S6-BARRIER");
    assert_eq!(
        data.expect("data")["reason"],
        "selective-undo-below",
        "TMC011-S6-BARRIER: a plain undo does not return what the agent undo took away"
    );
    assert_intact(&w, &before);
}

/// The redo by the test's own (unattributed) client, confirmed when the daemon asks for it, as
/// the interactive seam does: the challenge first, then its token. Returns the `RedoResult` as
/// read from the wire.
fn confirmed_redo(w: &World) -> Value {
    let mut c = w.client();
    let first = w.redo_call(&mut c, None);
    let token = match first {
        Ok(done) => return done,
        Err(e) => {
            let (error, data) = rejected(Err::<Value, _>(e));
            assert_eq!(
                error,
                code::OPERATION_REJECTED,
                "the redo was rejected: {data:?}"
            );
            let data = data.unwrap_or(Value::Null);
            assert_eq!(
                data["reason"], "confirmation-required",
                "TMC003-SELECTIVE: the redo of an agent's undo asks to confirm, nothing else: {data}"
            );
            data["challenge"]["token"]
                .as_str()
                .expect("a challenge to confirm with")
                .to_owned()
        }
    };
    w.redo_call(&mut c, Some(&token))
        .unwrap_or_else(|e| panic!("TMC003-SELECTIVE: the confirmed redo failed: {e:?}"))
}

/// A plain undo of the test's client, confirmed when the daemon asks for it.
fn confirmed_plain_undo(w: &World) -> Value {
    let mut c = w.client();
    let token = match w.plain_undo_call(&mut c, None) {
        Ok(done) => return done,
        Err(e) => {
            let (_, data) = rejected(Err::<Value, _>(e));
            let data = data.unwrap_or(Value::Null);
            assert_eq!(
                data["reason"], "confirmation-required",
                "TMC003-SELECTIVE: after the redo the plain undo is not held by the barrier: {data}"
            );
            data["challenge"]["token"]
                .as_str()
                .expect("a challenge")
                .to_owned()
        }
    };
    w.plain_undo_call(&mut c, Some(&token))
        .unwrap_or_else(|e| panic!("TMC003-SELECTIVE: the confirmed plain undo failed: {e:?}"))
}

/// Escenario 6: the redo returns the set and lifts the barrier: the paths go back to the state after
/// the agent's work, the redo is recorded as `Target::Redo(U)`, U keeps its set and the next plain
/// undo is no longer held by the barrier.
#[test]
fn redo_restores_the_set_and_lifts_the_barrier() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.unattributed_commit(&[("mine.txt", "mine")], "unattributed work");
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    w.agent_commit(&mut claude1, &[("two.txt", "2")], "claude-1 two");
    let head_with_work = w.head();
    let log_with_work = w.log();
    let result = undone(&w, &claude1.session);
    let recorded = format!("{:?}", last_undo_target(&w));
    assert_ne!(
        w.head(),
        head_with_work,
        "the undo took the agent's commits away"
    );

    let redone = confirmed_redo(&w);

    assert_eq!(
        redone["redone_undo_id"], result.operation_id,
        "TMC003-SELECTIVE: the redo redoes the agent undo: {redone}"
    );
    assert_eq!(
        w.head(),
        head_with_work,
        "TMC003-SELECTIVE: the branch is back after the agent's work"
    );
    assert_eq!(
        w.log(),
        log_with_work,
        "TMC003-SELECTIVE: the agent's commits are back"
    );
    assert_eq!(
        w.read("one.txt"),
        "1\n",
        "TMC003-SELECTIVE: one.txt is back"
    );
    assert_eq!(
        w.read("two.txt"),
        "2\n",
        "TMC003-SELECTIVE: two.txt is back"
    );
    assert_eq!(
        w.read("mine.txt"),
        "mine",
        "TMC003-SELECTIVE: the unattributed work is untouched"
    );
    let redos = w.requests(OperationKind::Redo);
    let redo = redos
        .last()
        .expect("TMC003-SELECTIVE: the redo is in the oplog");
    assert_eq!(redo.view.state, OperationState::Finished);
    assert!(
        matches!(&redo.view.record.target, Target::Redo(u) if *u == result.operation_id),
        "TMC003-SELECTIVE: the redo records Target::Redo(U): {:?}",
        redo.view.record.target
    );
    // U keeps the set it took: the redo does not rewrite it.
    let stored = w
        .requests(OperationKind::Undo)
        .into_iter()
        .find(|r| r.view.record.operation_id == result.operation_id)
        .expect("the agent undo with its set")
        .view
        .record
        .target;
    assert!(
        matches!(&stored, Target::Undo(ids) if ids.len() == result.undone_operation_ids.len()),
        "TMC003-SELECTIVE: the set the undo took stays in the oplog: {stored:?}"
    );
    assert_eq!(
        format!("{stored:?}"),
        recorded,
        "TMC003-SELECTIVE: the redo does not touch the stored set"
    );

    // The barrier is lifted: a plain undo is not `selective-undo-below` any more.
    let mut c = w.client();
    if let Err(e) = w.plain_undo_call(&mut c, None) {
        let (_, data) = rejected(Err::<Value, _>(e));
        assert_ne!(
            data.expect("data")["reason"],
            "selective-undo-below",
            "TMC003-SELECTIVE: the redo lifts the barrier"
        );
    }
}

/// Escenario 6: after the redo, the next plain undo takes the latest operation (`two.txt` of
/// claude-1), not one under the redone set.
#[test]
fn plain_undo_after_redo_takes_the_latest_operation() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.unattributed_commit(&[("mine.txt", "mine")], "unattributed work");
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    w.agent_commit(&mut claude1, &[("two.txt", "2")], "claude-1 two");
    undone(&w, &claude1.session);
    confirmed_redo(&w);
    assert_eq!(
        w.log()[0],
        "claude-1 two",
        "TMC003-SELECTIVE: the redo brought the commits back"
    );

    confirmed_plain_undo(&w);

    assert_eq!(
        w.log()[0],
        "claude-1 one",
        "TMC003-SELECTIVE: the plain undo took only the latest commit: {:?}",
        w.log()
    );
    assert!(
        !w.worktree.join("two.txt").exists(),
        "TMC003-SELECTIVE: two.txt, the latest operation, is the one taken back"
    );
    assert_eq!(w.read("one.txt"), "1\n", "TMC003-SELECTIVE: one.txt stays");
    assert_eq!(
        w.read("mine.txt"),
        "mine",
        "TMC003-SELECTIVE: nothing under the set is touched"
    );
}

/// D7: the agent edited a credential file and then took it out of the index (it stays in the
/// worktree, untracked): the prior leaves it out as a credential, so it has no copy for the redo
/// and the undo is `not-selectable` from `ready`.
#[test]
fn an_excluded_path_in_the_target_is_not_selectable() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[(".env", "TOKEN=new")], "claude-1 env");
    w.agent_forget(&mut claude1, ".env", "claude-1 stops tracking env");
    w.write(".env.local", "LOCAL=secret\n");
    let before = intact(&w);
    let env_after = w.read(".env");

    let (error, data) = rejection(&w, &claude1.session);

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-EXCLUDED");
    assert_eq!(data["reason"], "not-selectable", "TMC011-EXCLUDED: {data}");
    assert_intact(&w, &before);
    assert_eq!(
        w.read(".env.local"),
        "LOCAL=secret\n",
        "TMC011-EXCLUDED: an unversioned .env stays untouched"
    );
    assert_eq!(
        w.read(".env"),
        env_after,
        "TMC011-EXCLUDED: the untracked credential file stays untouched"
    );
    let undos = w.requests(OperationKind::Undo);
    let states = &undos.last().expect("the confirmed undo").states;
    assert!(
        states.iter().any(|s| s == "ready") && states.last().is_some_and(|s| s == "rejected"),
        "TMC011-EXCLUDED: rejected from ready: {states:?}"
    );
}

/// D1: the attribution is the one in force when the undo is planned; a later correction changes
/// neither the recorded set nor what the redo gives back.
#[test]
fn a_correction_after_the_undo_does_not_change_what_it_undid() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    assert_is_claude_code(&w, &claude1.session);
    w.agent_commit(&mut claude1, &[("one.txt", "1")], "claude-1 one");
    let head_with_work = w.head();
    let result = undone(&w, "claude-code");
    let recorded = last_undo_target(&w);
    assert!(
        matches!(&recorded, Target::Undo(ids) if ids.len() == result.undone_operation_ids.len() && !ids.is_empty()),
        "TMC011-ATTR-AT-PLAN: the record keeps the ids: {recorded:?}"
    );

    let answer = w.correct(&claude1.session, "codex-1");
    assert!(
        answer.get("ok").is_some(),
        "the correction was accepted: {answer}"
    );

    assert_eq!(
        format!("{:?}", last_undo_target(&w)),
        format!("{recorded:?}"),
        "TMC011-ATTR-AT-PLAN: the later correction does not change what the undo took"
    );
    let redone = confirmed_redo(&w);
    assert_eq!(
        redone["redone_undo_id"], result.operation_id,
        "TMC003-SELECTIVE: the redo redoes that undo: {redone}"
    );
    assert_eq!(
        w.head(),
        head_with_work,
        "TMC003-SELECTIVE: the redo gives back exactly what the undo took, whoever owns it now"
    );
    assert_eq!(
        w.read("one.txt"),
        "1\n",
        "TMC003-SELECTIVE: the same file is back"
    );
    assert_eq!(
        format!("{:?}", last_undo_target(&w)),
        format!("{recorded:?}"),
        "TMC003-SELECTIVE: the redo leaves the recorded set as it was"
    );
}

/// D5: without the engine's attribution nothing is chosen (fail closed).
#[test]
fn without_attribution_the_agent_undo_is_rejected() {
    let w = world_without_engine();
    let before = intact(&w);
    let mut c = w.client();

    let (error, data) = rejected(w.undo_call(&mut c, "claude-code", Some(SINCE), None));

    assert_eq!(error, code::OPERATION_REJECTED, "TMC011-NO-ENGINE");
    assert_eq!(
        data.expect("data")["reason"],
        "attribution-unavailable",
        "TMC011-NO-ENGINE: no attribution, no agent undo"
    );
    assert_intact(&w, &before);
}
