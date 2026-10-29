//! C1–C5, C7 and C8 of `docs/dev-briefs/tm-prior-point-binding.md`: the one rule that picks the point
//! before a raw Git event. Points and events are built by hand, in record order (`seq`).

use super::*;

/// An observation (or a manual point) recorded at `seq` whose read state reaches `mark`.
fn obs(seq: i64, mark: i64) -> PointFacts {
    PointFacts {
        seq,
        mark,
        prior: false,
        mid_operation: false,
        anchor_seq: None,
    }
}

/// An observation taken while an operation was in progress.
fn mid(seq: i64, mark: i64) -> PointFacts {
    PointFacts {
        mid_operation: true,
        ..obs(seq, mark)
    }
}

/// A `HookPrior` or `GuaranteedPrior` recorded at `seq` with mark `mark`.
fn prior(seq: i64, mark: i64) -> PointFacts {
    PointFacts {
        seq,
        mark,
        prior: true,
        mid_operation: false,
        anchor_seq: None,
    }
}

/// A `GuaranteedPrior` recorded at `seq` with mark `mark` whose operation left its anchor at
/// record `anchor`.
fn anchored(seq: i64, mark: i64, anchor: i64) -> PointFacts {
    PointFacts {
        anchor_seq: Some(anchor),
        ..prior(seq, mark)
    }
}

// C1 — rebase with a late observation.

#[test]
fn rebase_with_a_late_mid_operation_observation_picks_the_hook_prior() {
    // Obs(m0), HookPrior(m0), event 11, Obs(m1) mid-rebase, event 12.
    let points = [obs(1, 10), prior(2, 10), mid(3, 11)];
    let events = [11, 12];
    for seq in events {
        let picked = before_event(&points, &events, seq, 0);
        assert_eq!(picked.first(), Some(&1), "event {seq}: {picked:?}");
        assert!(!picked.contains(&2), "event {seq}: {picked:?}");
    }
}

#[test]
fn rebase_without_a_prior_keeps_the_mid_operation_observation_eligible() {
    // Declared limitation (TRADEOFF, D2 b / D3): without a prior the mid-operation photo is
    // eligible like any other point.
    let points = [obs(1, 10), mid(2, 11)];
    let events = [11, 12];
    assert_eq!(
        before_event(&points, &events, 12, 0),
        vec![1, 0],
        "event 12"
    );
    assert_eq!(before_event(&points, &events, 11, 0), vec![0], "event 11");
}

// C2 — restore with a late Git event.

#[test]
fn restore_with_a_late_git_event_picks_the_guaranteed_prior_not_the_anchor() {
    // GuaranteedPrior(m2) at seq 5, then the anchor Obs(m2) at seq 6, then event 3 (= 21).
    let points = [prior(5, 20), obs(6, 20)];
    let events = [21];
    assert_eq!(before_event(&points, &events, 21, 0), vec![0]);
}

#[test]
fn restore_anchor_whose_mark_covers_the_event_is_picked_after_it() {
    // The anchor reads the restore's event (mark m3 = 21): it is the point before event 4.
    let points = [prior(5, 20), obs(6, 21)];
    let events = [21, 22];
    assert_eq!(before_event(&points, &events, 22, 0), vec![1, 0]);
}

// C9 — an edit after a GitRaptor operation, then a raw Git event (NFR-01).

#[test]
fn an_edit_captured_after_a_restore_is_the_point_before_a_raw_reset() {
    // GuaranteedPrior(m1) at 12, the restore's anchor Obs(m1) at 28, its follow-up Obs(m1) at
    // 31, the edit Obs(m1) at 34, then the raw `reset --hard` that moves nothing (event 2).
    let points = [anchored(12, 1, 28), obs(28, 1), obs(31, 1), obs(34, 1)];
    let events = [2];
    assert_eq!(before_event(&points, &events, 2, 0), vec![3, 2, 0]);
}

#[test]
fn a_restore_with_a_late_git_event_still_shadows_its_anchor() {
    // C2 with the anchor known: the anchor itself stays the operation's, not a point before
    // the late event.
    let points = [anchored(5, 20, 6), obs(6, 20)];
    let events = [21];
    assert_eq!(before_event(&points, &events, 21, 0), vec![0]);
}

#[test]
fn a_hook_prior_window_still_runs_until_its_first_event() {
    // A hooked command's end is unknown: a later observation stays shadowed (C3).
    let points = [prior(1, 30), obs(2, 30), obs(3, 30)];
    let events = [31];
    assert_eq!(before_event(&points, &events, 31, 0), vec![0]);
}

// C3 — branch deletion with a prior.

#[test]
fn branch_deletion_with_a_late_observation_picks_the_hook_prior() {
    // HookPrior(m30), then Obs(m30) before the engine persists the BranchDelete (31).
    let points = [prior(1, 30), obs(2, 30)];
    let events = [31];
    assert_eq!(before_event(&points, &events, 31, 0), vec![0]);
}

#[test]
fn branch_deletion_with_an_interleaved_mark_bump_picks_the_hook_prior() {
    // Another worktree's event (31) bumps the mark of the late observation; the
    // BranchDelete of this worktree is 32.
    let points = [prior(1, 30), obs(2, 31)];
    let events = [32];
    assert_eq!(before_event(&points, &events, 32, 0), vec![0]);
}

// C4 — two worktrees interleaved.

#[test]
fn interleaved_worktrees_bind_each_prior_to_its_own_events() {
    // Engine: B's event 41, A's event 42, B's event 43. Records: PA 1, PB 2, OA 3, OB 4.
    let a_points = [prior(1, 40), obs(3, 41)];
    let a_events = [42];
    let b_points = [prior(2, 40), obs(4, 42)];
    let b_events = [41, 43];
    // OA was taken before A's command reached the engine: shadowed by PA.
    assert_eq!(before_event(&a_points, &a_events, 42, 0), vec![0]);
    // OB reads B's first event: not shadowed by PB, it is before B's next event.
    assert_eq!(before_event(&b_points, &b_events, 43, 0), vec![1, 0]);
    assert_eq!(before_event(&b_points, &b_events, 41, 0), vec![0]);
}

#[test]
fn interleaved_events_of_another_worktree_do_not_close_the_window() {
    // PA(m40); another worktree's event 41 (not in A's list); OA(m41); A's events 42, 43;
    // OA2(m42) after A's first event.
    let points = [prior(1, 40), obs(2, 41), obs(3, 42)];
    let events = [42, 43];
    assert_eq!(before_event(&points, &events, 42, 0), vec![0]);
    assert_eq!(before_event(&points, &events, 43, 0), vec![2, 0]);
}

// C5 — negative controls.

#[test]
fn control_a_clean_point_after_the_command_is_picked_for_the_next_event() {
    // Prior(m50), the command's first event 51, clean points with mark == 51 and > 51.
    let points = [prior(1, 50), obs(2, 51), obs(3, 52)];
    let events = [51, 53];
    assert_eq!(before_event(&points, &events, 53, 0), vec![2, 1, 0]);
}

#[test]
fn control_points_below_the_floor_or_not_before_the_event_are_never_picked() {
    // Floor 2 drops seq 1; marks 10 and 11 are not before event 10.
    let points = [obs(1, 5), obs(2, 6), obs(3, 10), obs(4, 11)];
    let events = [10];
    assert_eq!(before_event(&points, &events, 10, 2), vec![1]);
    // A prior whose mark is not before the event is not picked either.
    let points = [obs(2, 4), prior(3, 7)];
    assert_eq!(before_event(&points, &[7], 7, 2), vec![0]);
}

#[test]
fn control_a_prior_below_the_floor_casts_no_shadow_on_the_current_generation() {
    // A prior of the previous generation (its mark, 900, is of the old numbering: no event of
    // the new generation is after it) and an observation of the current one (floor 2).
    let points = [prior(1, 900), obs(2, 6)];
    let events = [7];
    assert_eq!(before_event(&points, &events, 7, 2), vec![1]);
}

// C8 — aborted merge (D2 b / D3 amendment).

#[test]
fn abort_of_a_merge_without_a_prior_picks_the_half_done_merge_photo() {
    // Clean Obs(m10), the merge stops (event 11), the half-done photo Mid(m12), the abort (13).
    let points = [obs(1, 10), mid(2, 12)];
    let events = [11, 13];
    assert_eq!(before_event(&points, &events, 13, 0), vec![1, 0]);
}

#[test]
fn abort_of_a_merge_after_an_older_prior_and_a_clean_point_picks_the_half_done_merge_photo() {
    // Prior(m5) and its event 6; a clean Obs(m10) between the prior and the half-done photo.
    let points = [prior(1, 5), obs(2, 10), mid(3, 12)];
    let events = [6, 11, 13];
    assert_eq!(before_event(&points, &events, 13, 0), vec![2, 1, 0]);
}

#[test]
fn abort_of_a_hooked_rebase_after_a_partial_resolution_picks_the_prior() {
    // HookPrior(m10); the rebase stops (event 11); two mid-operation photos with no clean point
    // after the prior; the abort (12) goes back to the prior.
    let points = [prior(1, 10), mid(2, 11), mid(3, 11)];
    let events = [11, 12];
    assert_eq!(before_event(&points, &events, 12, 0), vec![0]);
}

#[test]
fn abort_of_a_hooked_rebase_with_a_late_clean_observation_still_picks_the_prior() {
    // HookPrior(m10); a clean Obs recorded after it with a stale mark (10 < f_P = 11), so the
    // prior shadows it under (a) and it is not a clean point between; a mid-rebase photo; the
    // rebase events 11 and 12. The prior wins, never the late observation nor the photo.
    let points = [prior(1, 10), obs(2, 10), mid(3, 11)];
    let events = [11, 12];
    assert_eq!(before_event(&points, &events, 12, 0), vec![0]);
}

// C7 — the condition recorded by a capture (D3).

#[test]
fn mid_operation_is_true_if_in_progress_at_start_or_at_end() {
    assert!(mid_operation(true, false), "in progress at the start");
    assert!(mid_operation(false, true), "in progress only at the end");
    assert!(mid_operation(true, true), "in progress all along");
    assert!(!mid_operation(false, false), "clean at both ends");
}
