//! Contract tests of TD-GRP-007 (written by the criterion-tester): an event whose S3 window
//! overlaps a write window of the daemon in the repo is foreign (GRP007-WINDOW).
//!
//! The repo of `Rig::new` is `/r`; the agent's session `20:2000` commits in `/wt/feat-login`
//! and no `git` of the daemon is alive, so only the window can make the event ambiguous. Marks
//! are `gitraptor_api::clock::monotonic_ns`, the clock the windows use.

use std::panic::{AssertUnwindSafe, catch_unwind};

use gitraptor_api::clock::monotonic_ns;

use super::*;
use crate::detect::daemon_writes::WINDOW_TAIL;

const WT: &str = "/wt/feat-login";
const FLUSH_NS: u64 = 75_000_000;

fn rig() -> Rig {
    let rig = Rig::new();
    rig.claude(20, 2_000, WT);
    rig.scan();
    git(&rig, 31, 20, Some(WT));
    rig
}

/// Samples at `t_recv` and evaluates the agent's commit with the window
/// `[t_recv - s3_lead, t_flush]`.
fn evaluate(rig: &Rig, t_recv: u64, t_flush: u64) -> S3Evidence {
    rig.detector.sample_now("r", t_recv);
    rig.detector
        .evidence("r", Path::new(WT), S3Scope::Repo, None, t_recv, t_flush)
}

fn assert_attributed_uncounted(e: &S3Evidence, label: &str) {
    match &e.outcome {
        S3Outcome::Attributed(p) => assert_eq!(p.session_id, "20:2000"),
        other => panic!(
            "GRP007-WINDOW: {label}: must be attributed, got {other:?} {:?}",
            e.counts
        ),
    }
    assert_eq!(
        e.counts.daemon_write_window, 0,
        "GRP007-WINDOW: {label}: no window overlapped"
    );
}

fn assert_ambiguous_by_window(e: &S3Evidence, label: &str) {
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-WINDOW: {label}: a daemon write window overlapping the S3 window makes the event foreign: {:?}",
        e.counts
    );
    assert_eq!(
        e.counts.daemon_write_window, 1,
        "GRP007-WINDOW: {label}: daemon_write_window counts the evaluation"
    );
    assert_eq!(
        e.counts.foreign_daemon, 0,
        "{label}: no git of the daemon is alive"
    );
}

/// Nanoseconds past which a closed window no longer covers an S3 window: `WINDOW_TAIL` and a second.
fn beyond_the_tail() -> u64 {
    u64::try_from(WINDOW_TAIL.as_nanos()).unwrap() + 1_000_000_000
}

/// The positive case the negative ones are measured against: an open window on the repo.
fn assert_open_window_is_ambiguous() {
    let rig = rig();
    let _guard = rig.writes.begin(Path::new("/r"));
    let t = monotonic_ns();
    assert_ambiguous_by_window(&evaluate(&rig, t, t + FLUSH_NS), "baseline open window");
}

#[test]
fn daemon_write_window_open_during_the_s3_window_is_ambiguous() {
    let rig = rig();
    let _guard = rig.writes.begin(Path::new("/r"));
    let t = monotonic_ns();
    assert_ambiguous_by_window(&evaluate(&rig, t, t + FLUSH_NS), "open window");
}

#[test]
fn daemon_write_window_closed_inside_the_s3_window_is_ambiguous() {
    let rig = rig();
    let t_start = monotonic_ns();
    drop(rig.writes.begin(Path::new("/r")));
    let t_end = monotonic_ns();
    // [start, end] lies inside [t_start - s3_lead, t_end + 1 ms].
    let e = evaluate(&rig, t_start, t_end + 1_000_000);
    assert_ambiguous_by_window(&e, "window closed inside the S3 window");
}

#[test]
fn daemon_write_window_on_another_repo_does_not_count() {
    assert_open_window_is_ambiguous();
    let rig = rig();
    let _guard = rig.writes.begin(Path::new("/other-repo"));
    let t = monotonic_ns();
    assert_attributed_uncounted(&evaluate(&rig, t, t + FLUSH_NS), "window on another repo");
}

#[test]
fn daemon_write_window_closed_before_the_s3_window_does_not_count() {
    assert_open_window_is_ambiguous();
    let rig = rig();
    drop(rig.writes.begin(Path::new("/r")));
    // The S3 window starts more than WINDOW_TAIL after the window closed.
    let t = monotonic_ns() + beyond_the_tail();
    assert_attributed_uncounted(&evaluate(&rig, t, t + FLUSH_NS), "window closed before");
}

#[test]
fn daemon_write_window_closed_less_than_the_tail_before_the_s3_window_is_ambiguous() {
    let rig = rig();
    drop(rig.writes.begin(Path::new("/r")));
    // The S3 window starts one second after the window closed, inside WINDOW_TAIL.
    let t = monotonic_ns() + 1_000_000_000;
    assert!(WINDOW_TAIL > Duration::from_secs(1));
    let e = evaluate(&rig, t, t + FLUSH_NS);
    assert_ambiguous_by_window(&e, "window closed within WINDOW_TAIL before the S3 window");
}

#[test]
fn daemon_write_window_dropping_the_guard_closes_it() {
    let rig = rig();
    let guard = rig.writes.begin(Path::new("/r"));
    let t1 = monotonic_ns();
    assert_ambiguous_by_window(&evaluate(&rig, t1, t1 + FLUSH_NS), "guard alive");
    drop(guard);
    let t2 = monotonic_ns() + beyond_the_tail();
    assert_attributed_uncounted(
        &evaluate(&rig, t2, t2 + FLUSH_NS),
        "after dropping the guard",
    );
}

#[test]
fn daemon_write_window_a_panic_with_the_guard_alive_closes_it() {
    let rig = rig();
    let t_before = monotonic_ns();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _guard = rig.writes.begin(Path::new("/r"));
        panic!("the applier panics with its window open");
    }));
    assert!(result.is_err());
    // The window was opened: an evaluation around it sees it...
    let e = evaluate(&rig, t_before, monotonic_ns() + FLUSH_NS);
    assert_ambiguous_by_window(&e, "window opened before the panic");
    // ...and closed: a later one does not.
    let t = monotonic_ns() + beyond_the_tail();
    assert_attributed_uncounted(&evaluate(&rig, t, t + FLUSH_NS), "after the panic");
}

// ----- The tail of the applier's window: what it wrote clears or keeps the event --------------------

const OTHER: &str = "3333333333333333333333333333333333333333";
const BRANCH_REF: &str = "refs/heads/feat-login";

/// Evaluates the agent's commit one second after a window closed (inside `WINDOW_TAIL`), with the
/// branch move of the event.
fn evaluate_in_the_tail(rig: &Rig, moved: Option<RefMove<'_>>) -> S3Evidence {
    let t = monotonic_ns() + 1_000_000_000;
    rig.detector.sample_now("r", t);
    rig.detector
        .evidence("r", Path::new(WT), S3Scope::Repo, moved, t, t + FLUSH_NS)
}

fn feat_login(old: Option<&'static str>, new: &'static str) -> RefMove<'static> {
    RefMove {
        branch: "feat-login",
        old,
        new,
    }
}

/// The applier's window over `/r` that wrote these pairs and closed.
fn closed_tracked(rig: &Rig, pairs: &[(&str, &str)]) {
    let window = rig.writes.begin_tracked(Path::new("/r"));
    for (refname, value) in pairs {
        window.wrote(refname, value);
    }
    window.complete();
    drop(window);
}

/// The positive every Attributed case is measured against: the same tail, with the event
/// clearing the window, is attributed; without a branch move it is not.
fn assert_tail_cleared(rig: &Rig, moved: RefMove<'_>, label: &str) {
    let e = evaluate_in_the_tail(rig, Some(moved));
    assert_attributed_uncounted(&e, label);
}

#[test]
fn daemon_write_window_tail_an_event_without_a_branch_move_is_foreign() {
    let rig = rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    let e = evaluate_in_the_tail(&rig, None);
    assert_ambiguous_by_window(&e, "tail, no branch_move");
}

#[test]
fn daemon_write_window_tail_a_ref_the_window_did_not_touch_goes_to_s3() {
    let rig = rig();
    closed_tracked(&rig, &[("refs/heads/other", OLD)]);
    assert_tail_cleared(
        &rig,
        feat_login(Some(OLD), NEW),
        "ref untouched by the window",
    );
}

#[test]
fn daemon_write_window_tail_a_ref_touched_from_the_value_the_daemon_left_goes_to_s3() {
    let rig = rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    assert_tail_cleared(
        &rig,
        feat_login(Some(OLD), NEW),
        "old equals what the daemon left",
    );
}

#[test]
fn daemon_write_window_tail_a_different_or_unreadable_old_is_foreign() {
    // The positive first: the same window, an old that matches.
    let rig_ok = rig();
    closed_tracked(&rig_ok, &[(BRANCH_REF, OLD)]);
    assert_tail_cleared(&rig_ok, feat_login(Some(OLD), NEW), "baseline: old equals");

    for old in [Some(OTHER), None] {
        let rig = rig();
        closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
        let e = evaluate_in_the_tail(&rig, Some(feat_login(old, NEW)));
        assert_ambiguous_by_window(&e, &format!("tail, old {old:?}"));
    }
}

#[test]
fn daemon_write_window_tail_a_new_equal_to_what_the_applier_wrote_is_a_late_echo() {
    let rig_ok = rig();
    closed_tracked(&rig_ok, &[(BRANCH_REF, OLD)]);
    assert_tail_cleared(&rig_ok, feat_login(Some(OLD), NEW), "baseline: new differs");

    let rig = rig();
    closed_tracked(&rig, &[(BRANCH_REF, NEW)]);
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), NEW)));
    assert_ambiguous_by_window(&e, "tail, new equals what the applier wrote");
}

#[test]
fn daemon_write_window_tail_the_executors_window_covers_any_event() {
    let rig_ok = rig();
    closed_tracked(&rig_ok, &[(BRANCH_REF, OLD)]);
    assert_tail_cleared(
        &rig_ok,
        feat_login(Some(OLD), NEW),
        "baseline: tracked window",
    );

    let rig = rig();
    // The executor's window knows no oids.
    drop(rig.writes.begin(Path::new("/r")));
    for moved in [None, Some(feat_login(Some(OLD), NEW))] {
        let e = evaluate_in_the_tail(&rig, moved);
        assert_ambiguous_by_window(&e, &format!("tail, executor window, {moved:?}"));
    }
}

#[test]
fn daemon_write_window_tail_a_commit_on_a_detached_head_is_foreign() {
    let rig = rig();
    // The restore detached HEAD at an oid; the agent's commit there has no branch move.
    closed_tracked(&rig, &[("HEAD", OLD)]);
    let e = evaluate_in_the_tail(&rig, None);
    assert_ambiguous_by_window(&e, "tail, detached HEAD");
}

#[test]
fn daemon_write_window_tail_a_commit_on_the_target_of_a_changed_symbolic_head_goes_to_s3() {
    let rig = rig();
    // The restore pointed HEAD to `dest`, recorded as that ref at its value on closing.
    closed_tracked(&rig, &[("refs/heads/dest", OLD)]);
    let moved = RefMove {
        branch: "dest",
        old: Some(OLD),
        new: NEW,
    };
    assert_tail_cleared(&rig, moved, "commit on the symbolic HEAD's target");
}

/// Two applier windows that wrote the branch ref one after the other: A left `OLD`, then B left
/// `OTHER`.
fn two_sequential_windows(rig: &Rig) {
    closed_tracked(rig, &[(BRANCH_REF, OLD)]);
    closed_tracked(rig, &[(BRANCH_REF, OTHER)]);
}

#[test]
fn daemon_write_window_tail_the_latest_window_of_a_ref_decides_its_old() {
    // The latest window (B) left OTHER: an event from OTHER clears the margin.
    let rig = rig();
    two_sequential_windows(&rig);
    assert_tail_cleared(
        &rig,
        feat_login(Some(OTHER), NEW),
        "old equals the value the latest window left",
    );

    // From the value of the earlier window (A) it does not: B moved the ref after it.
    let rig = self::rig();
    two_sequential_windows(&rig);
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), NEW)));
    assert_ambiguous_by_window(&e, "tail, old equals the earlier window's value");
}

#[test]
fn daemon_write_window_tail_an_executor_window_next_to_a_tracked_one_is_foreign() {
    let rig_ok = rig();
    closed_tracked(&rig_ok, &[(BRANCH_REF, OLD)]);
    assert_tail_cleared(
        &rig_ok,
        feat_login(Some(OLD), NEW),
        "baseline: tracked window alone",
    );

    let rig = rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    drop(rig.writes.begin(Path::new("/r")));
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), NEW)));
    assert_ambiguous_by_window(&e, "tail, tracked window plus an executor window");
}

#[test]
fn daemon_write_window_tail_overlapping_windows_of_the_same_ref_are_foreign() {
    // Baseline: sequential windows, the event from the latest value, is attributed.
    let rig_ok = rig();
    two_sequential_windows(&rig_ok);
    assert_tail_cleared(
        &rig_ok,
        feat_login(Some(OTHER), NEW),
        "baseline: sequential windows",
    );

    // A opens, B opens, A closes, B closes: the order of their writes is unknown.
    let rig = rig();
    let a = rig.writes.begin_tracked(Path::new("/r"));
    let b = rig.writes.begin_tracked(Path::new("/r"));
    a.wrote(BRANCH_REF, OLD);
    b.wrote(BRANCH_REF, OTHER);
    a.complete();
    b.complete();
    drop(a);
    drop(b);
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), NEW)));
    assert_ambiguous_by_window(&e, "tail, overlapping windows, old equals B's value");
}

#[test]
fn daemon_write_window_tail_a_late_echo_of_an_earlier_window_is_foreign() {
    let rig = rig();
    two_sequential_windows(&rig);
    // new equals what A wrote, while B is the latest window of the ref.
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), OLD)));
    assert_ambiguous_by_window(&e, "tail, new equals the earlier window's value");

    // The baseline of this setup: a new that nobody wrote is attributed.
    let rig = self::rig();
    two_sequential_windows(&rig);
    assert_tail_cleared(
        &rig,
        feat_login(Some(OTHER), NEW),
        "baseline: new nobody wrote",
    );
}

// ----- The chain of cleared moves: a cleared event's new is the ref's current value ----------------

const THIRD: &str = "4444444444444444444444444444444444444444";
const FIFTH: &str = "5555555555555555555555555555555555555555";

/// The applier's window left the branch at `OLD`; the agent then commits `OLD -> OTHER` and
/// `OTHER -> THIRD` inside the margin. Returns the rig, after both evaluations.
fn chain_of_two() -> Rig {
    let rig = self::rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    let first = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), OTHER)));
    assert_attributed_uncounted(
        &first,
        "chain: the first commit starts from the window's value",
    );
    let second = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), THIRD)));
    assert_attributed_uncounted(
        &second,
        "chain: the second commit starts from the first one's new",
    );
    rig
}

#[test]
fn daemon_write_window_tail_a_chain_of_two_commits_is_attributed() {
    let _ = chain_of_two();
}

#[test]
fn daemon_write_window_tail_a_missing_middle_event_breaks_the_chain() {
    let _ = chain_of_two();
    let rig = self::rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    // The commit OLD -> OTHER was never evaluated: OTHER is not in the chain.
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), THIRD)));
    assert_ambiguous_by_window(&e, "tail, second commit without the first");
}

#[test]
fn daemon_write_window_tail_a_new_tracked_window_restarts_the_chain() {
    let _ = chain_of_two();
    let rig = self::rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    let first = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), OTHER)));
    assert_attributed_uncounted(&first, "first commit");
    // A new window leaves the branch at FIFTH.
    closed_tracked(&rig, &[(BRANCH_REF, FIFTH)]);
    let stale = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), THIRD)));
    assert_ambiguous_by_window(&stale, "tail, old from the chain before the new window");
    let fresh = evaluate_in_the_tail(&rig, Some(feat_login(Some(FIFTH), THIRD)));
    assert_attributed_uncounted(&fresh, "old equals the new window's value");
}

#[test]
fn daemon_write_window_tail_a_new_that_a_window_wrote_is_foreign_even_inside_the_chain() {
    let _ = chain_of_two();
    let rig = self::rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    let first = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), OTHER)));
    assert_attributed_uncounted(&first, "first commit");
    // OTHER is in the chain, but the new OLD is what the applier wrote: a late echo.
    let echo = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), OLD)));
    assert_ambiguous_by_window(&echo, "tail, new equals a written value");
}

#[test]
fn daemon_write_window_tail_an_event_that_is_not_cleared_does_not_advance_the_chain() {
    let _ = chain_of_two();
    let rig = self::rig();
    closed_tracked(&rig, &[(BRANCH_REF, OLD)]);
    // A foreign event (old unreadable) moved the branch to OTHER: it is not cleared.
    let foreign = evaluate_in_the_tail(&rig, Some(feat_login(None, OTHER)));
    assert_ambiguous_by_window(&foreign, "tail, unreadable old");
    let next = evaluate_in_the_tail(&rig, Some(feat_login(Some(OTHER), THIRD)));
    assert_ambiguous_by_window(
        &next,
        "tail, old equals the new of an event that was not cleared",
    );
}

#[test]
fn daemon_write_window_tail_the_chain_expires_with_the_margin() {
    let rig = chain_of_two();
    // Past WINDOW_TAIL no window covers: S3 as usual, with no window to clear.
    let t = monotonic_ns() + beyond_the_tail();
    let e = evaluate(&rig, t, t + FLUSH_NS);
    assert_attributed_uncounted(&e, "past the margin");
}

// ----- The late echo by value: any ref, and past the margin (retention) ----------------------------

/// Evaluates the agent's commit with the S3 window starting `after_ns` after now.
fn evaluate_later(rig: &Rig, after_ns: u64, moved: RefMove<'_>) -> S3Evidence {
    let t = monotonic_ns() + after_ns;
    rig.detector.sample_now("r", t);
    rig.detector.evidence(
        "r",
        Path::new(WT),
        S3Scope::Repo,
        Some(moved),
        t,
        t + FLUSH_NS,
    )
}

#[test]
fn daemon_write_window_tail_a_new_equal_to_a_value_written_on_another_ref_is_foreign() {
    // The window wrote `refs/heads/a`; the event moves `feat-login`, untouched by it.
    let rig = rig();
    closed_tracked(&rig, &[("refs/heads/a", OTHER)]);
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), OTHER)));
    assert_ambiguous_by_window(&e, "tail, new equals a value written on another ref");

    // The positive of the same setup: a new nobody wrote is attributed.
    let rig = self::rig();
    closed_tracked(&rig, &[("refs/heads/a", OTHER)]);
    assert_tail_cleared(
        &rig,
        feat_login(Some(OLD), NEW),
        "baseline: new nobody wrote",
    );
}

#[test]
fn daemon_write_window_the_late_echo_is_foreign_past_the_margin_while_the_window_is_kept() {
    // Positive first: ten seconds after the window closed (past WINDOW_TAIL, within the
    // retention) an event with a new nobody wrote is attributed.
    let rig_ok = rig();
    closed_tracked(&rig_ok, &[(BRANCH_REF, OTHER)]);
    let ok = evaluate_later(&rig_ok, 10_000_000_000, feat_login(Some(OLD), NEW));
    assert_attributed_uncounted(&ok, "baseline: past the margin, a new nobody wrote");

    let rig = self::rig();
    closed_tracked(&rig, &[(BRANCH_REF, OTHER)]);
    let echo = evaluate_later(&rig, 10_000_000_000, feat_login(Some(OLD), OTHER));
    assert_ambiguous_by_window(
        &echo,
        "past the margin, new equals a value the window wrote",
    );
}

// ----- Completed and failed windows -----------------------------------------------------------------

/// Samples at `t_recv` and evaluates the agent's commit with the branch move `moved`.
fn evaluate_moved(rig: &Rig, t_recv: u64, t_flush: u64, moved: RefMove<'_>) -> S3Evidence {
    rig.detector.sample_now("r", t_recv);
    rig.detector.evidence(
        "r",
        Path::new(WT),
        S3Scope::Repo,
        Some(moved),
        t_recv,
        t_flush,
    )
}

/// A window closed successfully whose `[start, end]` lies inside the S3 window, evaluated with
/// `moved`.
fn completed_in_the_overlap(moved: RefMove<'_>, complete: bool) -> S3Evidence {
    let rig = self::rig();
    let t_start = monotonic_ns();
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote(BRANCH_REF, OLD);
    if complete {
        window.complete();
    }
    drop(window);
    let t_end = monotonic_ns();
    evaluate_moved(&rig, t_start, t_end + 1_000_000, moved)
}

#[test]
fn daemon_write_window_a_completed_window_in_the_overlap_clears_an_event_from_its_value() {
    let e = completed_in_the_overlap(feat_login(Some(OLD), NEW), true);
    assert_attributed_uncounted(&e, "completed window in the overlap, old equals its value");
}

#[test]
fn daemon_write_window_a_window_still_open_when_evaluating_is_foreign() {
    // Positive first: the same sequence with the window closed is attributed.
    let closed = completed_in_the_overlap(feat_login(Some(OLD), NEW), true);
    assert_attributed_uncounted(&closed, "baseline: the window closed with success");

    let rig = self::rig();
    let t_start = monotonic_ns();
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote(BRANCH_REF, OLD);
    window.complete();
    let e = evaluate_moved(
        &rig,
        t_start,
        monotonic_ns() + FLUSH_NS,
        feat_login(Some(OLD), NEW),
    );
    assert_ambiguous_by_window(&e, "window alive while evaluating");
    drop(window);
}

#[test]
fn daemon_write_window_a_failed_window_never_clears_an_event() {
    // Positive first: the same sequence with `complete()` is attributed, in the tail.
    let ok = self::rig();
    closed_tracked(&ok, &[(BRANCH_REF, OLD)]);
    assert_tail_cleared(
        &ok,
        feat_login(Some(OLD), NEW),
        "baseline: completed window",
    );

    let rig = self::rig();
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote(BRANCH_REF, OLD);
    // Dropped without `complete()`: a failed window, its final state unknown.
    drop(window);
    let e = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), NEW)));
    assert_ambiguous_by_window(&e, "tail, failed window, old equals what it wrote");

    // Also in the overlap.
    let e = completed_in_the_overlap(feat_login(Some(OLD), NEW), false);
    assert_ambiguous_by_window(&e, "overlap, failed window");
}

#[test]
fn daemon_write_window_a_ref_written_twice_is_left_at_its_last_value() {
    // V1 = OLD, V2 = OTHER, Y = NEW.
    let write_twice = || {
        let rig = self::rig();
        closed_tracked(&rig, &[(BRANCH_REF, OLD), (BRANCH_REF, OTHER)]);
        rig
    };
    let rig = write_twice();
    assert_tail_cleared(
        &rig,
        feat_login(Some(OTHER), NEW),
        "old equals the final value",
    );

    let rig = write_twice();
    let echo = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), OTHER)));
    assert_ambiguous_by_window(&echo, "tail, V1 -> V2 is the applier's own echo");

    let rig = write_twice();
    let stale = evaluate_in_the_tail(&rig, Some(feat_login(Some(OLD), NEW)));
    assert_ambiguous_by_window(&stale, "tail, old is not the final value");
}

// ----- Windows that open after the batch's flush ----------------------------------------------------

/// The marks of an S3 window, taken before the daemon's window opens (no subtraction from the
/// clock: on Windows it starts with the process). A pause after them makes the window open
/// strictly after `t_flush`.
fn batch_marks() -> (u64, u64) {
    let t_recv = monotonic_ns();
    let t_flush = monotonic_ns();
    std::thread::sleep(Duration::from_millis(2));
    (t_recv, t_flush)
}

/// The event `OLD -> OTHER` of the batch with these marks.
fn evaluate_batch(rig: &Rig, (t_recv, t_flush): (u64, u64)) -> S3Evidence {
    evaluate_moved(rig, t_recv, t_flush, feat_login(Some(OLD), OTHER))
}

#[test]
fn daemon_write_window_a_window_that_opens_after_the_flush_still_catches_the_echo() {
    // Positive first: the same event with no window at all is attributed.
    let none = self::rig();
    let marks = batch_marks();
    assert_attributed_uncounted(&evaluate_batch(&none, marks), "baseline: no window");

    // (a) A window that opened after the event's flush, wrote its new value and closed.
    let rig = self::rig();
    let marks = batch_marks();
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote(BRANCH_REF, OTHER);
    window.complete();
    drop(window);
    let e = evaluate_batch(&rig, marks);
    assert_ambiguous_by_window(
        &e,
        "window opened after the flush, new equals what it wrote",
    );

    // (b) A window that opened after the flush and is still open.
    let rig = self::rig();
    let marks = batch_marks();
    let _open = rig.writes.begin_tracked(Path::new("/r"));
    let e = evaluate_batch(&rig, marks);
    assert_ambiguous_by_window(&e, "window opened after the flush and still open");
}
