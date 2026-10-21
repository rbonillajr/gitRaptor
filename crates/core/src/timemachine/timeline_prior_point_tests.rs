//! C6 of `docs/dev-briefs/tm-prior-point-binding.md`: the timeline (`Points::with_events`) and
//! the undo (`raw_candidates`) pick the point before an event with the same rule.

use std::collections::HashMap;

use super::super::oplog::{
    CompleteInfo, SnapshotLevel, SnapshotRecord, SnapshotState, SnapshotView,
};
use super::super::undo::raw_candidates;
use super::Points;

const KEY: &str = "/repo";

/// A complete, intact snapshot of `KEY` recorded at `seq` with mark `mark`.
fn view(id: &str, seq: i64, level: SnapshotLevel, mark: i64, mid: Option<bool>) -> SnapshotView {
    SnapshotView {
        record: SnapshotRecord {
            snapshot_id: id.into(),
            seq,
            level,
            worktrees: vec![KEY.into()],
            store_ref: format!("refs/gitraptor/snapshots/{id}"),
            engine_mark: Some(mark),
            cause_operation: None,
            cause_event_seq: None,
            recorded_ms: 1_000 + seq,
            manual: None,
            hook_prior: None,
        },
        state: SnapshotState::Complete,
        complete: Some(CompleteInfo {
            mid_operation: mid,
            ..CompleteInfo::default()
        }),
        tampered: false,
    }
}

fn events(seqs: &[i64]) -> HashMap<String, Vec<i64>> {
    HashMap::from([(KEY.to_owned(), seqs.to_vec())])
}

#[test]
fn timeline_and_undo_pick_the_same_point_for_an_event() {
    // HookPrior(m30); an observation whose mark another worktree bumped to 31; the branch
    // deletion of this worktree is event 32.
    let all = [
        view("prior", 1, SnapshotLevel::HookPrior, 30, Some(false)),
        view("late", 2, SnapshotLevel::Observation, 31, Some(false)),
    ];
    let ev = events(&[32]);
    let points = Points::with_events(&all, &ev);
    let timeline = points
        .before_event(KEY, 32, 0)
        .map(|s| s.record.snapshot_id.clone());
    let undo = raw_candidates(&all, KEY, &ev[KEY], 32, 0);
    assert_eq!(timeline.as_deref(), Some("prior"));
    assert_eq!(undo.first().map(String::as_str), Some("prior"));
    assert_eq!(timeline.as_ref(), undo.first());
}

#[test]
fn undo_never_targets_a_mid_operation_observation_that_follows_a_prior() {
    let all = [
        view("clean", 1, SnapshotLevel::Observation, 10, Some(false)),
        view("prior", 2, SnapshotLevel::HookPrior, 10, Some(false)),
        view("mid", 3, SnapshotLevel::Observation, 11, Some(true)),
    ];
    let ev = events(&[11, 12]);
    let undo = raw_candidates(&all, KEY, &ev[KEY], 12, 0);
    assert_eq!(undo, vec!["prior".to_owned(), "clean".to_owned()]);
    let points = Points::with_events(&all, &ev);
    assert_eq!(
        points
            .before_event(KEY, 12, 0)
            .map(|s| s.record.snapshot_id.as_str()),
        Some("prior")
    );
}

#[test]
fn timeline_and_undo_pick_the_half_done_merge_photo_for_a_merge_abort() {
    // C8 through the wiring: no prior, the merge stops (11), the half-done photo, the abort (13).
    let all = [
        view("clean", 1, SnapshotLevel::Observation, 10, Some(false)),
        view("mid", 2, SnapshotLevel::Observation, 12, Some(true)),
    ];
    let ev = events(&[11, 13]);
    let undo = raw_candidates(&all, KEY, &ev[KEY], 13, 0);
    assert_eq!(undo, vec!["mid".to_owned(), "clean".to_owned()]);
    let points = Points::with_events(&all, &ev);
    assert_eq!(
        points
            .before_event(KEY, 13, 0)
            .map(|s| s.record.snapshot_id.as_str()),
        Some("mid")
    );
}

#[test]
fn an_unavailable_prior_still_shadows_and_an_older_point_or_none_wins() {
    // An older observation; a HookPrior(m30) since purged; a late observation (mark bumped to
    // 31 by another worktree) before the branch deletion, event 32.
    let mut purged = view("prior", 2, SnapshotLevel::HookPrior, 30, Some(false));
    purged.state = SnapshotState::Purged;
    let older = view("older", 1, SnapshotLevel::Observation, 29, Some(false));
    let late = view("late", 3, SnapshotLevel::Observation, 31, Some(false));
    let ev = events(&[32]);

    let all = [older.clone(), purged.clone(), late.clone()];
    let offerable = [older.clone(), late.clone()];
    let points = Points::with_events(&all, &ev).offerable(&offerable);
    assert_eq!(
        points
            .before_event(KEY, 32, 0)
            .map(|s| s.record.snapshot_id.as_str()),
        Some("older")
    );
    assert_eq!(
        raw_candidates(&all, KEY, &ev[KEY], 32, 0),
        vec!["older".to_owned()]
    );

    // Without the older point nobody is left: no point before it, `TargetUnavailable`.
    let all = [purged, late.clone()];
    let offerable = [late];
    let points = Points::with_events(&all, &ev).offerable(&offerable);
    assert!(points.before_event(KEY, 32, 0).is_none());
    assert!(raw_candidates(&all, KEY, &ev[KEY], 32, 0).is_empty());
}
