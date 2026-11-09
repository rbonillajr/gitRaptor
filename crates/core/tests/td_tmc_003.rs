//! TD-TMC-003 end to end over the channel: the branch rule of `undo --agent` does not count the
//! anchored echo of GitRaptor's own undo or redo of the agent's work as a move of another actor.
//! It takes back the original guard 4.2 of US-TMC-003 (undo → redo → undo by agent). A real
//! daemon in this process, real Git, temporary repos and profile (NFR-01). Debug builds only.
//!
//! Independent of the Claude Code session that runs the tests: "claude-1" and "claude-2" are
//! `raptor-fake-agent` shell processes of the `us_tmc_011_common` world, and their session ids come
//! from `sessions.list`. No test waits on time: each step waits on the daemon's answer or on the
//! timeline.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_011_common;

use gitraptor_api::methods;
use serde_json::Value;
use us_tmc_011_common::*;

const SINCE: &str = "20m";

/// An undo by `agent` itself over its own work, as the agent's process saw it.
fn self_undo(w: &World, agent: &mut Agent) -> Value {
    let session = agent.session.clone();
    w.as_agent(
        agent,
        methods::TM_UNDO,
        w.undo_params(Some(&session), Some(SINCE), None),
    )
}

fn redo_as(w: &World, agent: &mut Agent) -> Value {
    w.as_agent(agent, methods::TM_REDO, params(&w.worktree, None))
}

fn done(answer: &Value, what: &str) -> Value {
    answer
        .get("ok")
        .cloned()
        .unwrap_or_else(|| panic!("{what} must be done: {answer}"))
}

fn undone_ids(result: &Value) -> Value {
    result["undone_operation_ids"].clone()
}

/// The criterion (guard 4.2 of US-TMC-003, original): claude-1 undoes its own work by agent, redoes
/// it and undoes it by agent again. The second undo is done, takes back the same operations and
/// leaves `HEAD` without the work.
#[test]
fn an_agent_undoes_its_work_again_after_redoing_it() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let without_work = w.head();
    w.agent_commit(&mut claude1, &[("one.rs", "fn one() {}")], "claude-1 one");
    let with_work = w.head();

    let first = done(
        &self_undo(&w, &mut claude1),
        "TDTMC003-GUARD-4.2: the first undo",
    );
    assert_eq!(
        w.head(),
        without_work,
        "the first undo took the commit away"
    );
    done(&redo_as(&w, &mut claude1), "TDTMC003-GUARD-4.2: the redo");
    assert_eq!(w.head(), with_work, "the redo brought the commit back");

    let second = done(
        &self_undo(&w, &mut claude1),
        "TDTMC003-GUARD-4.2: the second undo by agent, after its own redo",
    );

    assert_eq!(
        undone_ids(&second),
        undone_ids(&first),
        "TDTMC003-GUARD-4.2: the second undo takes back the same operations"
    );
    assert_eq!(
        w.head(),
        without_work,
        "TDTMC003-GUARD-4.2: HEAD is back without the work"
    );
}

/// Guard: after the agent's own undo and redo, a commit of another actor on the branch still stops
/// the undo by agent with `overlap` on the branch, and the repo does not change (D-TMC-34).
#[test]
fn another_actors_commit_after_an_own_echo_still_stops_the_undo() {
    let w = world();
    let mut claude1 = w.spawn_agent();
    let mut claude2 = w.spawn_agent();
    w.agent_commit(&mut claude1, &[("one.rs", "fn one() {}")], "claude-1 one");
    done(&self_undo(&w, &mut claude1), "the undo");
    done(&redo_as(&w, &mut claude1), "the redo");
    w.agent_commit(
        &mut claude2,
        &[("other.rs", "fn other() {}")],
        "claude-2 between",
    );
    w.agent_commit(&mut claude1, &[("two.rs", "fn two() {}")], "claude-1 two");
    let before = w.fx.fingerprint();

    let answer = self_undo(&w, &mut claude1);

    assert_eq!(
        answer["data"]["reason"], "overlap",
        "TDTMC003-OTHER-ACTOR: the interleaved commit still stops the undo: {answer}"
    );
    assert_intact(&w, &before);
}
