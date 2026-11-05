//! US-TMC-010 end to end over the channel: `timemachine.undo` with `since` takes back, as one
//! undo, the suffix of the worktree's stack whose moment falls in the period. A real daemon in
//! this process, real Git, temporary repos and profile (NFR-01). Debug builds only: the
//! confirmation seam does not exist in release (`TestConfirmation`).
//!
//! Independent of the session that runs the tests: every requester is "unattributed" (an
//! in-process client descends from the daemon) and the work of an agent is written into the oplog
//! with an explicit session id; nothing reads `CLAUDE_*` variables. No raw Git event is relied
//! on (continuous capture is off), so no attribution can leak in.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gitraptor_api::actor::Actor;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{OperationRunResult, TmRejectReason, UndoResult};
use gitraptor_core::channel::TestConfirmation;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OpRef, OperationKind, OperationState, OperationTransition, Requester,
    RequesterOrigin, Scope, SnapshotFilter, StackScope, Target,
};
use gitraptor_core::timemachine::store::SnapshotStore;
use gitraptor_testkit::diff;
use serde_json::{Value, json};
use us_tmc_013_common::*;

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn since_params(wt: &Path, since: &str, token: Option<&str>) -> Value {
    let mut p = params(wt, token);
    p["since"] = json!(since);
    p
}

fn undo_since(
    c: &mut Client,
    wt: &Path,
    since: &str,
    token: Option<&str>,
) -> Result<Value, ClientError> {
    c.call(
        gitraptor_api::methods::TM_UNDO,
        since_params(wt, since, token),
    )
}

fn done(value: Value) -> UndoResult {
    serde_json::from_value(value).expect("an UndoResult")
}

/// An unattributed operation: `a.rs` is dirtied with `content`, then `git reset --hard` runs
/// as a protected operation, whose prior snapshot holds `content`.
fn reset_with(r: &Running, content: &str) -> OperationRunResult {
    std::fs::write(r.worktree.join("a.rs"), content).unwrap();
    r.reset_hard()
}

/// The work of an agent on top of the stack, recorded now (`Running::agent_work` records it at
/// ms 1, always outside any period). Returns the real unattributed reset that gave it its prior
/// and the agent's operation id.
fn agent_work_now(r: &Running, session: &str) -> (OperationRunResult, String) {
    let real = reset_with(r, "fn a() { trabajo_del_agente(); }\n");
    let mut log = r.oplog.lock().unwrap();
    let id = log
        .record_operation(
            &NewOperation {
                kind: OperationKind::Protected,
                subtype: Some("reset-hard".into()),
                scope: Scope {
                    worktrees: vec![r.worktree.to_string_lossy().into_owned()],
                    refs: Vec::new(),
                },
                requester: Requester::Agent {
                    name: "claude-code".into(),
                    origin: RequesterOrigin::Detected,
                    session_id: session.into(),
                },
                channel: Channel::Mcp,
                confirmed: false,
                target: Target::None,
                warnings: Vec::new(),
                // On top of the stack, above every real operation.
                engine_mark: i64::MAX / 2,
            },
            now_ms(),
        )
        .unwrap();
    for step in [
        OperationTransition::PriorSnapshot {
            snapshot_id: &real.prior_snapshot_id,
        },
        OperationTransition::Ready,
        OperationTransition::Applying { step: 1 },
        OperationTransition::Finished,
    ] {
        log.advance_operation(&id, step, now_ms()).unwrap();
    }
    drop(log);
    r.anchor(&id);
    (real, id)
}

fn assert_intact(r: &Running, before: &gitraptor_testkit::fingerprint::Snapshot) {
    let changes = diff(before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "the repo changed: {changes:#?}");
}

fn snapshot_count(r: &Running) -> usize {
    r.oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .len()
}

fn rpc_error(result: Result<Value, ClientError>) -> gitraptor_api::rpc::ErrorObject {
    match result {
        Err(ClientError::Rpc(e)) => e,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

fn oplog_op(id: &str) -> OpRef {
    OpRef::Oplog(id.to_owned())
}

/// T004: `since` is served (here: nothing to undo), `agent` is still US-TMC-011's.
#[test]
fn since_is_served_and_agent_is_still_not_implemented() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    let mut c = r.client();

    let served = rpc_error(undo_since(&mut c, &r.worktree, "10m", None));
    assert_ne!(
        served.code,
        code::NOT_IMPLEMENTED,
        "TMC010-SERVED: since must no longer answer NOT_IMPLEMENTED"
    );
    assert_eq!(
        served.code,
        code::OPERATION_REJECTED,
        "TMC010-SERVED: with nothing in the stack, since is rejected as nothing-to-undo"
    );

    let mut p = params(&r.worktree, None);
    p["agent"] = json!("claude-1");
    let agent = rpc_error(c.call(gitraptor_api::methods::TM_UNDO, p));
    assert_eq!(agent.code, code::NOT_IMPLEMENTED);
    assert_eq!(agent.data.unwrap()["implemented_by"], "US-TMC-011");
}

/// Escenario 1: A outside the period, B and C inside; the undo takes back B and C only.
#[test]
fn since_undoes_the_operations_of_the_period_only() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    let a = reset_with(&r, "dirty-a\n");
    let a_ms = r.recorded().last().unwrap().view.record.recorded_ms;
    // The only wait by clock of the scenario: A must be older than 10 s for `since = 8s`.
    while now_ms() - a_ms <= 10_000 {
        std::thread::sleep(Duration::from_millis(250));
    }
    let b = reset_with(&r, "dirty-b\n");
    let c = reset_with(&r, "dirty-c\n");
    assert_eq!(
        r.a_rs(),
        "fn a() {}\n",
        "the resets left the committed content"
    );
    let mut client = r.client();

    let result = done(undo_since(&mut client, &r.worktree, "8s", None).unwrap());

    assert_eq!(
        result.undone_operation_ids,
        vec![b.operation_id.clone(), c.operation_id.clone()],
        "TMC010-S1-PERIOD: the undone operations, oldest first, are B and C"
    );
    assert_eq!(result.undone_operation_id, b.operation_id);
    assert_eq!(
        r.a_rs(),
        "dirty-b\n",
        "TMC010-S1-PERIOD: the worktree is back to the state before B; A stays applied"
    );
    let prior = &result.prior_snapshot_id;
    let view = r.oplog.lock().unwrap().snapshot(prior).unwrap();
    assert!(
        view.is_some_and(|s| s.state.is_available()),
        "TMC010-S1-PERIOD: the point before the undo exists and is available"
    );
    let repo_id = r.oplog.lock().unwrap().repo_id().to_owned();
    let store = SnapshotStore::open_existing(&r.tp.dirs(), &repo_id)
        .unwrap()
        .expect("the daemon's store");
    assert!(
        store.verify(prior).is_ok(),
        "TMC010-S1-PERIOD: the point before the undo verifies"
    );
    let undos = r.requests(OperationKind::Undo);
    assert_eq!(undos.len(), 1, "one undo, not one per operation");
    assert_eq!(undos[0].view.state, OperationState::Finished);
    assert_eq!(
        undos[0].view.record.target,
        Target::Undo(vec![oplog_op(&b.operation_id), oplog_op(&c.operation_id)]),
        "TMC010-S1-PERIOD: the oplog's undo targets B and C, never A ({})",
        a.operation_id
    );
}

/// Escenario 2: another worktree is never touched.
#[test]
fn since_never_touches_other_worktrees() {
    let (fx, wt) = repo_with_login();
    fx.git(&["branch", "feat-pagos"]);
    let pagos = fx
        .add_worktree("feat-pagos", "feat-pagos")
        .canonicalize()
        .unwrap();
    let r = start(fx, wt, None, None);
    reset_with(&r, "dirty-login\n");
    std::fs::write(pagos.join("pagos.rs"), "fn pagos() {}\n").unwrap();
    r.fx.git_in(&pagos, &["add", "pagos.rs"]);
    r.fx.git_in(&pagos, &["commit", "-q", "-m", "pagos"]);
    std::fs::write(pagos.join("wip.txt"), "uncommitted\n").unwrap();
    let footprint = |r: &Running| {
        (
            r.fx.git_in(&pagos, &["rev-parse", "HEAD", "refs/heads/feat-pagos"]),
            r.fx.git_in(&pagos, &["status", "--porcelain"]),
            std::fs::read_to_string(pagos.join("pagos.rs")).unwrap(),
            std::fs::read_to_string(pagos.join("wip.txt")).unwrap(),
        )
    };
    let before = footprint(&r);
    let mut c = r.client();

    let result = done(undo_since(&mut c, &r.worktree, "20m", None).unwrap());

    assert_eq!(result.undone_operation_ids.len(), 1);
    assert_eq!(
        r.a_rs(),
        "dirty-login\n",
        "feat-login did go back to before its reset"
    );
    assert_eq!(
        footprint(&r),
        before,
        "TMC010-S2-OTHER-WT: feat-pagos (worktree and branch) must not change"
    );
}

/// Escenario 3: an invalid period changes nothing and says why.
#[test]
fn an_invalid_period_changes_nothing() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    reset_with(&r, "dirty\n");
    let before = r.fx.fingerprint();
    let records = r.recorded().len();
    let mut c = r.client();

    let error = rpc_error(undo_since(&mut c, &r.worktree, "veinte", None));

    assert_eq!(error.code, code::INVALID_PARAMS, "TMC010-S3-INVALID");
    assert!(
        error.message.contains("since"),
        "TMC010-S3-INVALID: the answer names the field: {}",
        error.message
    );
    assert_intact(&r, &before);
    assert_eq!(
        r.recorded().len(),
        records,
        "TMC010-S3-INVALID: no new operation in the oplog"
    );
}

/// Escenario 4: nothing in the period is rejected as `nothing-to-undo`, and recorded.
#[test]
fn nothing_in_the_period_changes_nothing() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let (error, data) = rejected(undo_since(&mut c, &r.worktree, "20m", None));

    assert_eq!(error, code::OPERATION_REJECTED, "TMC010-S4-NOTHING");
    assert_eq!(data.unwrap()["reason"], "nothing-to-undo");
    assert_intact(&r, &before);
    let undos = r.requests(OperationKind::Undo);
    assert_eq!(undos.len(), 1, "the rejection is in the oplog");
    assert_eq!(undos[0].view.state, OperationState::Rejected);
    assert_eq!(undos[0].reason.as_deref(), Some("nothing-to-undo"));
}

/// Escenario 5: an unattributed operation and an agent's work in the period; confirmed, one undo
/// takes both back.
#[test]
fn another_actor_in_the_period_is_undone_once_confirmed() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    let own = reset_with(&r, "dirty-own\n");
    let (real, agent) = agent_work_now(&r, AGENT_SESSION);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let first = confirm_data(undo_since(&mut c, &r.worktree, "20m", None));

    assert_eq!(first.reason, TmRejectReason::ConfirmationRequired);
    assert!(
        first
            .owners
            .iter()
            .any(|o| matches!(o, Actor::Agent { .. })),
        "TMC010-S5-CONFIRMED: the challenge lists the agent as an owner"
    );
    assert_eq!(
        first.undone_operation_id.as_deref(),
        Some(own.operation_id.as_str()),
        "the challenge shows the oldest operation of the set"
    );
    assert_intact(&r, &before);

    let result = done(undo_since(&mut c, &r.worktree, "20m", Some(&token_of(&first))).unwrap());

    assert_eq!(
        result.undone_operation_ids,
        vec![own.operation_id.clone(), real.operation_id.clone(), agent],
        "TMC010-S5-CONFIRMED: every operation of the period is taken back, none left out"
    );
    assert_eq!(result.requester.actor, Actor::Unattributed);
    let undos = r.requests(OperationKind::Undo);
    let ran: Vec<_> = undos
        .iter()
        .filter(|u| u.view.state == OperationState::Finished)
        .collect();
    assert_eq!(ran.len(), 1, "one single confirmed undo");
    assert!(ran[0].view.record.confirmed);
}

/// Escenario 6: without the token the request is rejected, and nothing is left out in silence.
#[test]
fn another_actor_in_the_period_without_confirmation_is_rejected() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    reset_with(&r, "dirty-own\n");
    agent_work_now(&r, AGENT_SESSION);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(undo_since(&mut c, &r.worktree, "20m", None));

    assert_eq!(
        data.reason,
        TmRejectReason::ConfirmationRequired,
        "TMC010-S6-UNCONFIRMED"
    );
    assert!(
        data.owners.iter().any(|o| matches!(o, Actor::Agent { .. })),
        "the owners are listed"
    );
    assert_intact(&r, &before);
    let undos = r.requests(OperationKind::Undo);
    assert_eq!(undos.len(), 1);
    assert_eq!(undos[0].view.state, OperationState::Rejected);
    assert_eq!(undos[0].reason.as_deref(), Some("confirmation-required"));
}

/// What a user sees of a worktree and its branches, comparable between two repos: the content
/// of `a.rs`, the tree of `HEAD` and the tree of every branch tip.
fn content(r: &Running) -> (String, String, BTreeMap<String, String>) {
    let tips =
        r.fx.git(&["for-each-ref", "--format=%(refname) %(tree)", "refs/heads"])
            .lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(n, t)| (n.to_owned(), t.to_owned()))
            .collect();
    (
        r.a_rs(),
        r.fx.git_in(&r.worktree, &["rev-parse", "HEAD^{tree}"]),
        tips,
    )
}

/// D1/D2: one `--since` undo equals two consecutive undos.
#[test]
fn since_equals_consecutive_undos() {
    let build = || {
        let (fx, wt) = repo_with_login();
        let r = start(fx, wt, None, None);
        reset_with(&r, "dirty-b\n");
        reset_with(&r, "dirty-c\n");
        r
    };
    let once = build();
    let twice = build();

    let mut c = once.client();
    let result = done(undo_since(&mut c, &once.worktree, "10m", None).unwrap());
    assert_eq!(result.undone_operation_ids.len(), 2, "TMC010-EQ-N-UNDOS");
    let mut c = twice.client();
    for _ in 0..2 {
        c.call::<_, Value>(
            gitraptor_api::methods::TM_UNDO,
            params(&twice.worktree, None),
        )
        .unwrap();
    }

    assert_eq!(
        content(&once),
        content(&twice),
        "TMC010-EQ-N-UNDOS: same worktree content, HEAD tree and branch tips"
    );
    assert_eq!(once.a_rs(), "dirty-b\n");
}

/// The undo of a period leaves the stack ready for a redo of all of it.
#[test]
fn the_undo_records_every_target_for_redo() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    let b = reset_with(&r, "dirty-b\n");
    let c = reset_with(&r, "dirty-c\n");
    let mut client = r.client();

    let result = done(undo_since(&mut client, &r.worktree, "10m", None).unwrap());

    let stack = r
        .oplog
        .lock()
        .unwrap()
        .undo_stack(
            &StackScope::Worktree(r.worktree.to_string_lossy().into_owned()),
            &[],
        )
        .unwrap();
    assert!(
        !stack.done().contains(&oplog_op(&b.operation_id))
            && !stack.done().contains(&oplog_op(&c.operation_id)),
        "TMC010-REDO-READY: neither B nor C is left in the stack: {:?}",
        stack.done()
    );
    assert_eq!(
        stack.next_redo(),
        Some(result.operation_id.as_str()),
        "TMC010-REDO-READY: the next redo is the undo of the period"
    );
}

/// DS-US-TMC-015 D7: a Git operation in progress rejects the undo before its prior is taken.
#[test]
fn since_with_a_rebase_in_progress_is_rejected_without_a_prior() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    reset_with(&r, "dirty\n");
    let gitdir = std::path::PathBuf::from(
        r.fx.git_in(&r.worktree, &["rev-parse", "--absolute-git-dir"])
            .trim(),
    );
    std::fs::create_dir(gitdir.join("rebase-merge")).unwrap();
    let before = r.fx.fingerprint();
    let points = snapshot_count(&r);
    let mut c = r.client();

    let (error, data) = rejected(undo_since(&mut c, &r.worktree, "10m", None));

    assert_eq!(error, code::OPERATION_REJECTED, "TMC010-PRECOND");
    assert_eq!(
        data.unwrap()["reason"],
        "git-operation-in-progress",
        "TMC010-PRECOND: a rebase in progress rejects the undo by period"
    );
    assert_eq!(
        snapshot_count(&r),
        points,
        "TMC010-PRECOND: no prior point was taken"
    );
    assert_intact(&r, &before);
}
