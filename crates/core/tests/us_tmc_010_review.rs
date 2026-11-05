//! Review of US-TMC-010 over the channel: an undo by period never keeps, half undone, what a
//! later operation of the period changed and another actor touched afterwards (it is the same as
//! N consecutive undos), and a challenge is bound to the set it showed. A real daemon in this
//! process, real Git, temporary repos and profile (NFR-01). Debug builds only (the confirmation
//! seam).
//!
//! Independent of the session that runs the tests: the requester is "unattributed" (an
//! in-process client descends from the daemon) and the agent's work is written into the oplog
//! with an explicit session id, its prior and its anchor captured by the test.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{TmConfirmData, TmRejectReason};
use gitraptor_core::channel::TestConfirmation;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationState, OperationTransition, Requester,
    RequesterOrigin, Scope, SnapshotFilter, SnapshotLevel, Target,
};
use gitraptor_core::timemachine::store::{CaptureRequest, SnapshotStore, WorktreeScope};
use gitraptor_testkit::diff;
use serde_json::{Value, json};
use us_tmc_013_common::*;

/// The key of the `feat-login` worktree in a snapshot.
const KEY: &str = "wt-wt-feat-login";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn since_params(wt: &Path, token: Option<&str>) -> Value {
    let mut p = params(wt, token);
    p["since"] = json!("1h");
    p
}

fn undo_since(c: &mut Client, wt: &Path, token: Option<&str>) -> Result<Value, ClientError> {
    c.call(gitraptor_api::methods::TM_UNDO, since_params(wt, token))
}

/// The greatest engine mark of the oplog's points, at least `i64::MAX / 2`: the agent's work goes
/// above every point the daemon took.
fn next_mark(r: &Running) -> i64 {
    r.oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .iter()
        .filter_map(|s| s.record.engine_mark)
        .max()
        .unwrap_or(0)
        .max(i64::MAX / 2)
        + 1
}

fn capture(r: &Running, mark: i64, cause: Option<&str>) -> String {
    let repo_id = r.oplog.lock().unwrap().repo_id().to_owned();
    let (store, _) = SnapshotStore::open_or_create(&r.tp.dirs(), &repo_id).unwrap();
    let req = CaptureRequest {
        level: SnapshotLevel::Observation,
        echo_settled: None,
        common_dir: gitraptor_core::observe::locate(&r.fx.repo).unwrap(),
        worktrees: vec![WorktreeScope {
            key: KEY.into(),
            path: r.worktree.clone(),
            hint: None,
        }],
        engine_mark: Some(mark),
        cause_operation: cause.map(str::to_owned),
        cause_event_seq: None,
        include_credentials: false,
        still_valid: None,
        give_way: None,
    };
    store.capture(&r.oplog, &req).unwrap().snapshot_id
}

/// An operation of the agent recorded now: the state before it, then `effect` (what it did),
/// then its anchor.
fn agent_op(r: &Running, effect: impl FnOnce()) -> String {
    let mark = next_mark(r);
    let id = r
        .oplog
        .lock()
        .unwrap()
        .record_operation(
            &NewOperation {
                kind: OperationKind::Protected,
                subtype: Some("script".into()),
                scope: Scope {
                    worktrees: vec![r.worktree.to_string_lossy().into_owned()],
                    refs: Vec::new(),
                },
                requester: Requester::Agent {
                    name: "claude-code".into(),
                    origin: RequesterOrigin::Detected,
                    session_id: AGENT_SESSION.into(),
                },
                channel: Channel::Mcp,
                confirmed: false,
                target: Target::None,
                warnings: Vec::new(),
                engine_mark: mark,
            },
            now_ms(),
        )
        .unwrap();
    // The state before it, as an observation: an operation with no Git of its own leaves no raw
    // event to close a guaranteed prior's window, and its anchor would read as a lagging photo.
    let prior = capture(r, mark, None);
    {
        let mut log = r.oplog.lock().unwrap();
        log.advance_operation(
            &id,
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior,
            },
            now_ms(),
        )
        .unwrap();
        log.advance_operation(&id, OperationTransition::Ready, now_ms())
            .unwrap();
        log.advance_operation(&id, OperationTransition::Applying { step: 1 }, now_ms())
            .unwrap();
    }
    effect();
    r.oplog
        .lock()
        .unwrap()
        .advance_operation(&id, OperationTransition::Finished, now_ms())
        .unwrap();
    capture(r, mark + 1, Some(&id));
    id
}

/// An unattributed change of the worktree, captured as the continuous observation would.
fn person_edits(r: &Running, path: &str, content: &str) {
    std::fs::write(r.worktree.join(path), content).unwrap();
    let mark = next_mark(r);
    let repo_id = r.oplog.lock().unwrap().repo_id().to_owned();
    let (store, _) = SnapshotStore::open_or_create(&r.tp.dirs(), &repo_id).unwrap();
    let req = CaptureRequest {
        level: SnapshotLevel::Observation,
        echo_settled: None,
        common_dir: gitraptor_core::observe::locate(&r.fx.repo).unwrap(),
        worktrees: vec![WorktreeScope {
            key: KEY.into(),
            path: r.worktree.clone(),
            hint: None,
        }],
        engine_mark: Some(mark),
        cause_operation: None,
        cause_event_seq: None,
        include_credentials: false,
        still_valid: None,
        give_way: None,
    };
    store.capture(&r.oplog, &req).unwrap();
}

fn confirm_rejection(result: Result<Value, ClientError>) -> TmConfirmData {
    match result {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, code::OPERATION_REJECTED, "{e:?}");
            serde_json::from_value(e.data.expect("data")).expect("TmConfirmData")
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn running() -> Running {
    let (fx, wt) = repo_with_login();
    start(fx, wt, Some(TestConfirmation::Eligible), None)
}

/// H-01: op1 of the agent edits a.rs, op2 edits b.rs, then a person edits b.rs. Two consecutive
/// undos would stop at op2 with an overlap; the undo of the period must stop the same way and
/// leave everything as it is, never take a.rs back and keep b.rs half undone.
#[test]
fn a_later_operation_of_the_period_touched_by_another_actor_stops_the_undo() {
    let r = running();
    let wt = r.worktree.clone();
    agent_op(&r, || {
        std::fs::write(wt.join("a.rs"), "fn a() { op1(); }\n").unwrap()
    });
    agent_op(&r, || {
        std::fs::write(wt.join("b.rs"), "fn b() { op2(); }\n").unwrap()
    });
    person_edits(&r, "b.rs", "fn b() { op2(); person(); }\n");
    let before = r.fx.fingerprint();
    let undos_before = r.requests(OperationKind::Undo).len();
    let mut c = r.client();

    let first = confirm_rejection(undo_since(&mut c, &r.worktree, None));
    assert_eq!(first.reason, TmRejectReason::ConfirmationRequired);
    let token = token_of(&first);
    let data = confirm_rejection(undo_since(&mut c, &r.worktree, Some(&token)));

    assert_eq!(
        data.reason,
        TmRejectReason::Overlap,
        "what op2 changed and a person touched afterwards is a conflict, never kept"
    );
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "the repo changed: {changes:#?}");
    let requests = r.requests(OperationKind::Undo);
    assert_eq!(requests.len(), undos_before + 2);
    let last = requests.last().unwrap();
    assert_eq!(last.view.state, OperationState::Rejected);
    assert_eq!(last.reason.as_deref(), Some("overlap"));
}

/// A challenge is bound to the set it showed: one more operation in the period after it is
/// another plan, and its token undoes nothing.
#[test]
fn an_operation_that_enters_the_period_after_the_challenge_invalidates_it() {
    let r = running();
    let wt = r.worktree.clone();
    agent_op(&r, || {
        std::fs::write(wt.join("a.rs"), "fn a() { op1(); }\n").unwrap()
    });
    let mut c = r.client();
    let first = confirm_rejection(undo_since(&mut c, &r.worktree, None));
    assert_eq!(first.reason, TmRejectReason::ConfirmationRequired);
    assert_eq!(first.undone_count, 1);
    let token = token_of(&first);

    agent_op(&r, || {
        std::fs::write(wt.join("b.rs"), "fn b() { op2(); }\n").unwrap()
    });
    let before = r.fx.fingerprint();
    let snaps_before = r
        .oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .len();

    let stale = confirm_rejection(undo_since(&mut c, &r.worktree, Some(&token)));

    assert_eq!(
        stale.reason,
        TmRejectReason::ChallengeInvalid,
        "the token of a smaller set does not confirm a bigger one"
    );
    let changes = diff(&before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "the repo changed: {changes:#?}");
    let snaps_after = r
        .oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .len();
    assert_eq!(snaps_after, snaps_before, "no prior was taken");
}

/// The challenge of a period says how many operations it takes back.
#[test]
fn the_challenge_of_a_period_counts_its_operations() {
    let r = running();
    let wt = r.worktree.clone();
    agent_op(&r, || {
        std::fs::write(wt.join("a.rs"), "fn a() { op1(); }\n").unwrap()
    });
    agent_op(&r, || {
        std::fs::write(wt.join("b.rs"), "fn b() { op2(); }\n").unwrap()
    });
    let mut c = r.client();

    let data = confirm_rejection(undo_since(&mut c, &r.worktree, None));
    assert_eq!(data.reason, TmRejectReason::ConfirmationRequired);
    assert_eq!(data.undone_count, 2);
    assert_eq!(data.owners.len(), 1, "one agent, shown once");

    // The undo of the last operation keeps today's shape: no count.
    let last =
        confirm_rejection(c.call(gitraptor_api::methods::TM_UNDO, params(&r.worktree, None)));
    assert_eq!(last.undone_count, 0);
}
