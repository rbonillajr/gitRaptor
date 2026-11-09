//! TMC003-OWNERS (T004 guard 4.1 of DS-US-TMC-003): whose work a redo takes back. Child module of
//! `timemachine::redo`, mounted there as `mod tests`.
//!
//! Needs: `redo::redo_owners(&OperationView, &RawSide, &Oplog) -> Vec<Requester>` (not written
//! yet). Temporary profile only (NFR-01).

use gitraptor_api::messages::GitEventKind;
use gitraptor_api::timemachine::TmRejectReason;

use super::*;
use crate::profile::ProfileDirs;
use crate::timemachine::engine::RawGitEvent;
use crate::timemachine::oplog::{
    Channel, NewOperation, OpRef, OperationKind, OperationView, Oplog, Requester, RequesterOrigin,
    Scope, Target,
};
use crate::timemachine::restore::restore_permission;
use crate::timemachine::undo::RawSide;

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";
const WT: &str = "/repo/feat-login";

fn agent(name: &str, session: &str) -> Requester {
    Requester::Agent {
        name: name.into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn open() -> (tempfile::TempDir, Oplog) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (log, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
    (tmp, log)
}

fn scope() -> Scope {
    Scope {
        worktrees: vec![WT.into()],
        refs: vec!["refs/heads/feat-login".into()],
    }
}

fn record(log: &mut Oplog, kind: OperationKind, who: Requester, target: Target) -> OperationView {
    let id = log
        .record_operation(
            &NewOperation {
                kind,
                subtype: None,
                scope: scope(),
                requester: who,
                channel: Channel::Cli,
                confirmed: false,
                target,
                warnings: vec![],
                engine_mark: 1,
            },
            2_000,
        )
        .unwrap();
    log.operation(&id).unwrap().unwrap()
}

fn raw_event(seq: i64, actor: Requester) -> RawGitEvent {
    RawGitEvent {
        seq,
        kind: GitEventKind::Commit,
        branch: Some("feat-login".into()),
        actor,
        observed_ms: 3_000,
        in_gap: false,
        inferred: false,
        old_commit: None,
        new_commit: None,
    }
}

fn raw(events: Vec<RawGitEvent>) -> RawSide {
    RawSide { events, floor: 0 }
}

fn has(owners: &[Requester], who: &Requester) -> bool {
    owners.iter().any(|o| o == who)
}

#[test]
fn the_owners_of_a_redo_are_the_undo_and_what_it_undid() {
    let (_tmp, mut log) = open();
    let claude_1 = agent("claude-1", "100:1");
    let claude_2 = agent("claude-2", "200:1");
    // claude-2's commit, as a raw event, and one of its operations in the oplog.
    let by_claude_2 = record(
        &mut log,
        OperationKind::Protected,
        claude_2.clone(),
        Target::None,
    );
    let OperationView { record: op, .. } = by_claude_2;
    let undo = record(
        &mut log,
        OperationKind::Undo,
        Requester::Unattributed,
        Target::Undo(vec![
            OpRef::GitEvent(7),
            OpRef::Oplog(op.operation_id.clone()),
        ]),
    );
    let raw = raw(vec![raw_event(7, claude_2.clone())]);

    let owners = redo_owners(&undo, &raw, &log);

    assert!(
        has(&owners, &Requester::Unattributed),
        "TMC003-OWNERS: the requester of the undo owns the redo: {owners:?}"
    );
    assert!(
        has(&owners, &claude_2),
        "TMC003-OWNERS: the owner of what the undo took back owns the redo: {owners:?}"
    );
    assert!(
        !has(&owners, &claude_1),
        "TMC003-OWNERS: nobody else owns it: {owners:?}"
    );

    // claude-1 is not an owner of all: other-actor.
    assert_eq!(
        restore_permission(&claude_1, Channel::Cli, &owners),
        Err(TmRejectReason::OtherActor),
        "TMC003-OWNERS: an agent that does not own every part cannot redo (other-actor)"
    );
    // claude-2 cannot redo over the unattributed requester that undid it either.
    assert_eq!(
        restore_permission(&claude_2, Channel::Cli, &owners),
        Err(TmRejectReason::OtherActor),
        "TMC003-OWNERS: claude-2 cannot redo over the unattributed undo"
    );
    // Unattributed redoing an agent's work needs the confirmation.
    assert_eq!(
        restore_permission(&Requester::Unattributed, Channel::Cli, &owners),
        Err(TmRejectReason::ConfirmationRequired),
        "TMC003-OWNERS: an unattributed redo of an agent's work asks to confirm"
    );
}

#[test]
fn an_event_that_is_gone_counts_as_unattributed() {
    let (_tmp, mut log) = open();
    let claude_2 = agent("claude-2", "200:1");
    let undo = record(
        &mut log,
        OperationKind::Undo,
        claude_2.clone(),
        Target::Undo(vec![OpRef::GitEvent(7)]),
    );
    // The engine no longer has event 7.
    let owners = redo_owners(&undo, &raw(vec![]), &log);
    assert!(
        has(&owners, &claude_2),
        "TMC003-OWNERS: the requester of the undo still owns it: {owners:?}"
    );
    assert!(
        has(&owners, &Requester::Unattributed),
        "TMC003-OWNERS: an event the engine no longer has is Unattributed: {owners:?}"
    );
    assert_eq!(
        restore_permission(&claude_2, Channel::Cli, &owners),
        Err(TmRejectReason::OtherActor),
        "TMC003-OWNERS: the agent does not own the part that lost its event"
    );
}

#[test]
fn an_agent_that_owns_everything_may_redo() {
    let (_tmp, mut log) = open();
    let claude_1 = agent("claude-1", "100:1");
    let undo = record(
        &mut log,
        OperationKind::Undo,
        claude_1.clone(),
        Target::Undo(vec![OpRef::GitEvent(7)]),
    );
    let owners = redo_owners(&undo, &raw(vec![raw_event(7, claude_1.clone())]), &log);
    assert!(
        owners.iter().all(|o| *o == claude_1) && !owners.is_empty(),
        "TMC003-OWNERS: the undo and its target are all claude-1's: {owners:?}"
    );
    assert_eq!(restore_permission(&claude_1, Channel::Cli, &owners), Ok(()));
}
