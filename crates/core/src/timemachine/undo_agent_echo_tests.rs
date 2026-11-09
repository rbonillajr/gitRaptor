//! TD-TMC-003: the events the branch rule of an undo by agent reads. The anchored echo of a
//! finished undo or redo that only moved the set's own work is GitRaptor's, not a move of another
//! actor; every other echo counts with the requester of its operation, and anything not anchored
//! or not finished counts as it is (NFR-01: undo less, never more). Pure: no repo, no engine.
//! Child module of `timemachine::undo_agent`.

use super::*;
use crate::timemachine::oplog::{
    Channel, OperationRecord, OperationState, OperationView, RequesterOrigin, SnapshotLevel,
    SnapshotRecord, SnapshotState, SnapshotView, Target,
};

const KEY: &str = "/repo/feat-login";
const BRANCH: &str = "feat-login";

fn agent(name: &str) -> Requester {
    Requester::Agent {
        name: name.to_owned(),
        origin: RequesterOrigin::Detected,
        session_id: format!("session-{name}"),
    }
}

fn event(seq: i64, branch: &str, actor: Requester) -> RawGitEvent {
    RawGitEvent {
        seq,
        kind: GitEventKind::Reset,
        branch: Some(branch.to_owned()),
        actor,
        observed_ms: 0,
        in_gap: false,
        inferred: false,
        old_commit: None,
        new_commit: None,
    }
}

fn op(
    id: &str,
    kind: OperationKind,
    target: Target,
    mark: i64,
    state: OperationState,
    requester: Requester,
) -> OperationView {
    OperationView {
        record: OperationRecord {
            operation_id: id.to_owned(),
            seq: mark,
            kind,
            subtype: None,
            scope: Scope {
                worktrees: vec![KEY.to_owned()],
                refs: vec![format!("refs/heads/{BRANCH}")],
            },
            requester,
            channel: Channel::Cli,
            confirmed: false,
            target,
            warnings: Vec::new(),
            engine_mark: mark,
            recorded_ms: 0,
        },
        state,
        step: None,
        prior_snapshot: None,
        tampered: false,
    }
}

/// The observation that anchors `cause`'s echo at `mark`.
fn anchor(cause: &str, mark: i64) -> SnapshotView {
    SnapshotView {
        record: SnapshotRecord {
            snapshot_id: format!("snap-{cause}"),
            seq: mark,
            level: SnapshotLevel::Observation,
            worktrees: vec!["wt".to_owned()],
            store_ref: String::new(),
            engine_mark: Some(mark),
            cause_operation: Some(cause.to_owned()),
            cause_event_seq: None,
            recorded_ms: 0,
            manual: None,
            hook_prior: None,
        },
        state: SnapshotState::Complete,
        complete: None,
        tampered: false,
    }
}

fn undo_of(id: &str, refs: &[i64], mark: i64, state: OperationState) -> OperationView {
    op(
        id,
        OperationKind::Undo,
        Target::Undo(refs.iter().map(|s| OpRef::GitEvent(*s)).collect()),
        mark,
        state,
        Requester::Unattributed,
    )
}

/// The sessions the selector picks: claude-1's.
fn own() -> BTreeSet<String> {
    BTreeSet::from(["session-claude-1".to_owned()])
}

fn seqs(events: &[RawGitEvent]) -> Vec<i64> {
    events.iter().map(|e| e.seq).collect()
}

/// claude-1's commit (10) is the set; the undo U took it back (echo 21, anchored at 22).
fn own_undo_world() -> (
    Vec<RawGitEvent>,
    Vec<OperationView>,
    Vec<SnapshotView>,
    Vec<OpRef>,
) {
    let events = vec![
        event(10, BRANCH, agent("claude-1")),
        event(21, BRANCH, Requester::Unattributed),
    ];
    let ops = vec![undo_of("U", &[10], 20, OperationState::Finished)];
    let snaps = vec![anchor("U", 22)];
    (events, ops, snaps, vec![OpRef::GitEvent(10)])
}

#[test]
fn the_anchored_echo_of_an_undo_of_the_sets_own_work_is_dropped() {
    let (events, ops, snaps, set) = own_undo_world();

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10],
        "TDTMC003-ECHO-OWN: the undo's echo is GitRaptor's"
    );
}

#[test]
fn the_anchored_echo_of_a_redo_of_that_undo_is_dropped_too() {
    let (mut events, mut ops, mut snaps, set) = own_undo_world();
    events.push(event(31, BRANCH, Requester::Unattributed));
    ops.push(op(
        "R",
        OperationKind::Redo,
        Target::Redo("U".to_owned()),
        30,
        OperationState::Finished,
        agent("claude-1"),
    ));
    snaps.push(anchor("R", 32));

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10],
        "TDTMC003-ECHO-OWN: the redo's echo is GitRaptor's"
    );
}

#[test]
fn an_echo_of_another_agents_work_counts_with_its_requester() {
    let (mut events, _, snaps, set) = own_undo_world();
    events.insert(1, event(15, BRANCH, agent("claude-2")));
    // The undo took back claude-2's commit (15), not the set's: its echo is a move of claude-2.
    let ops = vec![op(
        "U",
        OperationKind::Undo,
        Target::Undo(vec![OpRef::GitEvent(15)]),
        20,
        OperationState::Finished,
        agent("claude-2"),
    )];

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10, 15, 21],
        "TDTMC003-ECHO-OTHER: it still counts"
    );
    assert_eq!(
        read[2].actor,
        agent("claude-2"),
        "TDTMC003-ECHO-OTHER: the echo counts with the requester of its operation"
    );
}

#[test]
fn an_echo_of_an_undo_that_also_took_back_foreign_work_counts() {
    let (mut events, _, snaps, set) = own_undo_world();
    events.insert(1, event(15, BRANCH, agent("claude-2")));
    let ops = vec![undo_of("U", &[10, 15], 20, OperationState::Finished)];

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(seqs(&read), vec![10, 15, 21], "TDTMC003-ECHO-MIXED");
}

#[test]
fn an_echo_without_anchor_counts_as_a_move() {
    let (events, ops, _, set) = own_undo_world();

    let read = branch_rule_events(&events, &ops, &[], 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10, 21],
        "TDTMC003-ECHO-UNANCHORED: in doubt, a move"
    );
    assert_eq!(read[1].actor, Requester::Unattributed);
}

#[test]
fn the_echo_of_an_unfinished_undo_counts_as_a_move() {
    for state in [OperationState::Applying, OperationState::Interrupted] {
        let (events, _, snaps, set) = own_undo_world();
        let ops = vec![undo_of("U", &[10], 20, state)];

        let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

        assert_eq!(
            seqs(&read),
            vec![10, 21],
            "TDTMC003-ECHO-UNFINISHED: {state:?}"
        );
    }
}

#[test]
fn a_move_after_the_anchor_counts_as_it_is() {
    let (mut events, ops, snaps, set) = own_undo_world();
    events.push(event(23, BRANCH, Requester::Unattributed));

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10, 23],
        "TDTMC003-ECHO-AFTER: outside the window it counts"
    );
}

#[test]
fn a_redo_of_an_undo_of_foreign_work_counts_with_its_requester() {
    let (mut events, _, mut snaps, set) = own_undo_world();
    events.insert(1, event(15, BRANCH, agent("claude-2")));
    events.push(event(31, BRANCH, Requester::Unattributed));
    let ops = vec![
        undo_of("U", &[15], 20, OperationState::Finished),
        op(
            "R",
            OperationKind::Redo,
            Target::Redo("U".to_owned()),
            30,
            OperationState::Finished,
            agent("claude-2"),
        ),
    ];
    snaps.push(anchor("R", 32));

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(seqs(&read), vec![10, 15, 21, 31], "TDTMC003-ECHO-OTHER");
    assert_eq!(read[3].actor, agent("claude-2"));
}

#[test]
fn an_operation_of_another_worktree_explains_nothing_here() {
    let (events, mut ops, snaps, set) = own_undo_world();
    ops[0].record.scope.worktrees = vec!["/repo/other".to_owned()];

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(seqs(&read), vec![10, 21], "TDTMC003-ECHO-WORKTREE");
}

#[test]
fn another_agents_commit_inside_an_own_echo_window_counts() {
    let (mut events, ops, snaps, set) = own_undo_world();
    events.push(event(22, BRANCH, agent("claude-2")));

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10, 22],
        "TDTMC003-ECHO-RAW-ACTOR: claude-2 is not an echo"
    );
    assert_eq!(read[1].actor, agent("claude-2"));
}

#[test]
fn an_echo_also_in_the_window_of_another_operation_counts() {
    let (events, mut ops, snaps, set) = own_undo_world();
    // A restore running alongside, unanchored: its window reaches past the echo.
    ops.push(op(
        "S",
        OperationKind::Restore,
        Target::Snapshot("p".to_owned()),
        19,
        OperationState::Finished,
        agent("claude-2"),
    ));

    let read = branch_rule_events(&events, &ops, &snaps, 0, KEY, &set, &own());

    assert_eq!(
        seqs(&read),
        vec![10, 21],
        "TDTMC003-ECHO-MIXED-WINDOW: mixed is other"
    );
}

#[test]
fn an_operation_below_the_generation_floor_explains_nothing() {
    let (events, ops, snaps, set) = own_undo_world();

    let read = branch_rule_events(&events, &ops, &snaps, 100, KEY, &set, &own());

    assert_eq!(seqs(&read), vec![10, 21], "TDTMC003-ECHO-FLOOR");
}
