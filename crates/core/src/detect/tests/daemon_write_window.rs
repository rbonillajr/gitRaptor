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
