//! Contract tests of TD-GRP-007 (written by the criterion-tester): a `git` of the daemon that
//! cannot make a Git event does not make the agent's commit ambiguous (GRP007-CLASSIFY).
//!
//! The repo of `Rig::new` is `/r` (common dir `/r/.git`); the agent's session `20:2000` works in
//! `/wt/feat-login`. The daemon is this very process.

use super::*;

const WT: &str = "/wt/feat-login";
const ENGINE_GIT: &str = "/usr/bin/git";

/// The real argv of the daemon's `git` whose subcommand starts with `first_word`, as the line
/// `Table::call` splits (the placeholder paths have no spaces).
fn daemon_line(first_word: &str) -> String {
    let argv = gitraptor_git::daemon_git_argvs()
        .into_iter()
        .find(|(word, _)| *word == first_word)
        .unwrap_or_else(|| panic!("daemon_git_argvs lists `{first_word}`"))
        .1;
    argv.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A rig with the agent's writing `git` in [`WT`], the daemon in the table and the engine's Git
/// fixed.
fn rig() -> Rig {
    let rig = Rig::new();
    rig.claude(20, 2_000, WT);
    rig.scan();
    git(&rig, 31, 20, Some(WT));
    rig.table
        .add(std::process::id(), 10, 1_000, "/opt/raptor", Some("/"));
    rig.detector.set_engine_git(Some(PathBuf::from(ENGINE_GIT)));
    rig
}

/// A direct child of the daemon running `line`, with the engine's Git, in `/r`.
fn inert_child(rig: &Rig, pid: u32, line: &str) {
    git(rig, pid, std::process::id(), Some("/r"));
    rig.table.call(pid, Some(line));
}

fn evaluate(rig: &Rig) -> S3Evidence {
    rig.detector.sample_now("r", 1_000);
    rig.evidence_in(WT, S3Scope::Repo, 1_000)
}

/// What the evaluation of the agent's commit with one inert child of the daemon gives.
fn assert_attributed_with_inert(e: &S3Evidence, label: &str) {
    match &e.outcome {
        S3Outcome::Attributed(p) => assert_eq!(p.session_id, "20:2000"),
        other => panic!(
            "GRP007-CLASSIFY: {label}: the agent's commit must be attributed, got {other:?} {:?}",
            e.counts
        ),
    }
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            daemon_inert: 1,
            ..S3Counts::default()
        },
        "GRP007-CLASSIFY: {label}: the inert git counts daemon_inert, not foreign_daemon"
    );
}

/// The positive case every negative one is measured against.
fn assert_baseline_attributed() {
    let rig = rig();
    inert_child(&rig, 50, &daemon_line("pack-objects"));
    assert_attributed_with_inert(&evaluate(&rig), "baseline pack-objects");
}

fn assert_foreign_daemon(e: &S3Evidence, label: &str, foreign: u32) {
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-CLASSIFY: {label}: must stay ambiguous: {:?}",
        e.counts
    );
    assert_eq!(
        e.counts.foreign_daemon, foreign,
        "GRP007-CLASSIFY: {label}: foreign_daemon"
    );
}

#[test]
fn daemon_inert_git_next_to_the_agents_commit_is_attributed() {
    for first_word in [
        "pack-objects",
        "update-index",
        "index-pack",
        "rev-parse",
        "log",
    ] {
        let rig = rig();
        inert_child(&rig, 50, &daemon_line(first_word));
        assert_attributed_with_inert(&evaluate(&rig), first_word);
    }
}

#[test]
fn daemon_inert_a_different_exe_than_the_engine_git_is_foreign() {
    assert_baseline_attributed();
    let rig = rig();
    rig.table.add(
        50,
        std::process::id(),
        wall_us() - 1_000,
        "/opt/homebrew/bin/git",
        Some("/r"),
    );
    rig.table.call(50, Some(&daemon_line("pack-objects")));
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "another exe", 1);
    assert_eq!(e.counts.daemon_inert, 0);
}

#[test]
fn daemon_inert_without_an_engine_git_is_foreign() {
    assert_baseline_attributed();
    let rig = rig();
    rig.detector.set_engine_git(None);
    inert_child(&rig, 50, &daemon_line("pack-objects"));
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "engine_git unset", 1);
    assert_eq!(e.counts.daemon_inert, 0);
}

#[test]
fn daemon_inert_an_argv_without_a_template_is_foreign() {
    assert_baseline_attributed();
    for line in ["log -1", "status", "update-index -z --index-info"] {
        let rig = rig();
        inert_child(&rig, 50, line);
        let e = evaluate(&rig);
        assert_foreign_daemon(&e, line, 1);
        assert_eq!(e.counts.daemon_inert, 0, "{line}");
    }
}

#[test]
fn daemon_inert_a_grandchild_of_the_daemon_is_foreign() {
    assert_baseline_attributed();
    let rig = rig();
    // daemon -> a program that is not a git -> an inert git: only a direct child counts.
    rig.table.add(
        60,
        std::process::id(),
        wall_us() - 2_000,
        "/bin/sh",
        Some("/r"),
    );
    git(&rig, 61, 60, Some("/r"));
    rig.table.call(61, Some(&daemon_line("pack-objects")));
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "grandchild", 1);
    assert_eq!(e.counts.daemon_inert, 0);
}

#[test]
fn daemon_inert_a_git_past_the_read_cap_is_foreign() {
    assert_baseline_attributed();
    let rig = rig();
    // The agent's git is the first read; 31 harmless ones fill the cap.
    let fill = u32::try_from(MAX_REDIRECT_READS).unwrap() - 1;
    for pid in 100..100 + fill {
        git(&rig, pid, 10, Some("/r"));
        rig.table.call(pid, Some("log --oneline"));
    }
    inert_child(&rig, 50, &daemon_line("pack-objects"));
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "past MAX_REDIRECT_READS", 1);
    assert_eq!(e.counts.daemon_inert, 0);
}

#[test]
fn daemon_inert_next_to_an_update_ref_of_the_daemon_stays_ambiguous() {
    assert_baseline_attributed();
    let rig = rig();
    inert_child(&rig, 50, &daemon_line("pack-objects"));
    inert_child(&rig, 51, &daemon_line("update-ref"));
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "inert + update-ref", 1);
    assert_eq!(
        e.counts.daemon_inert, 1,
        "GRP007-CLASSIFY: the inert git still counts daemon_inert next to an update-ref"
    );
}

#[test]
fn daemon_inert_next_to_a_foreign_git_that_writes_in_the_worktree_stays_ambiguous() {
    assert_baseline_attributed();
    let rig = rig();
    inert_child(&rig, 50, &daemon_line("pack-objects"));
    // A person's `git commit` (the default argv) in the event's worktree, outside the daemon.
    git(&rig, 41, 10, Some(WT));
    let e = evaluate(&rig);
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-CLASSIFY (NFR-01): a foreign git that writes keeps the event ambiguous: {:?}",
        e.counts
    );
    assert_eq!(e.counts.foreign_wt, 1);
    assert_eq!(
        e.counts.daemon_inert, 1,
        "the inert git still counts daemon_inert"
    );
}

// ----- The daemon's own git that is exiting (folder unreadable) -------------------------------------

use gitraptor_api::clock::monotonic_ns;

const MOVED_OLD: &str = "1111111111111111111111111111111111111111";
const MOVED_NEW: &str = "2222222222222222222222222222222222222222";

/// A direct child of the daemon, with the engine's Git, whose folder cannot be read.
fn exiting_child(rig: &Rig, pid: u32) {
    git(rig, pid, std::process::id(), None);
}

/// Evaluates the agent's commit one second from now (inside `WINDOW_TAIL` of a window that
/// closed now), without a branch move.
fn evaluate_in_the_tail(rig: &Rig) -> S3Evidence {
    let t = monotonic_ns() + 1_000_000_000;
    rig.detector.sample_now("r", t);
    rig.detector
        .evidence("r", Path::new(WT), S3Scope::Repo, None, t, t + 75_000_000)
}

fn assert_exiting_baseline() {
    let rig = rig();
    exiting_child(&rig, 50);
    let e = evaluate(&rig);
    match &e.outcome {
        S3Outcome::Attributed(p) => assert_eq!(p.session_id, "20:2000"),
        other => panic!(
            "GRP007-CLASSIFY: baseline: an exiting git of the daemon outside any window must not make the commit ambiguous, got {other:?} {:?}",
            e.counts
        ),
    }
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            daemon_exiting: 1,
            ..S3Counts::default()
        },
        "GRP007-CLASSIFY: baseline: the exiting git counts daemon_exiting, not foreign_daemon"
    );
}

#[test]
fn daemon_exiting_git_outside_any_window_is_attributed() {
    assert_exiting_baseline();
}

#[test]
fn daemon_exiting_git_inside_a_completed_window_margin_is_foreign() {
    assert_exiting_baseline();
    let rig = rig();
    exiting_child(&rig, 50);
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote("refs/heads/other", MOVED_OLD);
    window.complete();
    drop(window);
    let e = evaluate_in_the_tail(&rig);
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-CLASSIFY: an event without branch_move in a window's margin is foreign: {:?}",
        e.counts
    );
}

#[test]
fn daemon_exiting_git_next_to_a_failed_window_is_foreign() {
    assert_exiting_baseline();
    let rig = rig();
    exiting_child(&rig, 50);
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote("refs/heads/other", MOVED_OLD);
    // Dropped without `complete()`.
    drop(window);
    let e = evaluate_in_the_tail(&rig);
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-CLASSIFY: a failed window in the margin keeps the event foreign: {:?}",
        e.counts
    );
}

#[test]
fn daemon_exiting_git_that_is_a_grandchild_is_foreign() {
    assert_exiting_baseline();
    // daemon -> a program that is not a git -> an exiting git.
    let rig = rig();
    rig.table.add(
        60,
        std::process::id(),
        wall_us() - 2_000,
        "/bin/sh",
        Some("/r"),
    );
    git(&rig, 61, 60, None);
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "exiting grandchild", 1);
    assert_eq!(e.counts.daemon_exiting, 0);

    // daemon -> an exiting git -> an exiting git: the child is exiting, the grandchild is foreign.
    let rig = self::rig();
    exiting_child(&rig, 50);
    git(&rig, 51, 50, None);
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "exiting git under an exiting git of the daemon", 1);
    assert_eq!(
        e.counts.daemon_exiting, 1,
        "only the direct child is daemon_exiting"
    );
}

#[test]
fn daemon_exiting_git_with_another_exe_than_the_engine_git_is_foreign() {
    assert_exiting_baseline();
    let rig = rig();
    rig.table.add(
        50,
        std::process::id(),
        wall_us() - 1_000,
        "/opt/homebrew/bin/git",
        None,
    );
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "exiting git, another exe", 1);
    assert_eq!(e.counts.daemon_exiting, 0);
}

#[test]
fn daemon_exiting_git_without_an_engine_git_is_foreign() {
    assert_exiting_baseline();
    let rig = rig();
    rig.detector.set_engine_git(None);
    exiting_child(&rig, 50);
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "exiting git, engine_git unset", 1);
    assert_eq!(e.counts.daemon_exiting, 0);
}

#[test]
fn daemon_exiting_git_whose_parent_pid_was_reused_is_not_counted() {
    assert_exiting_baseline();
    // The child is older than the process that now has the daemon's pid: not its child.
    let rig = rig();
    rig.table
        .add(50, std::process::id(), 500, "/usr/bin/git", None);
    let e = evaluate(&rig);
    assert_eq!(
        e.counts.daemon_exiting, 0,
        "GRP007-CLASSIFY: a git older than its parent is not a child of the daemon: {:?}",
        e.counts
    );
}

#[test]
fn daemon_exiting_git_next_to_a_window_that_opened_after_the_flush_is_foreign() {
    assert_exiting_baseline();
    // (1) A window that opened after the event's flush and is still open.
    let rig = rig();
    exiting_child(&rig, 50);
    let t_recv = monotonic_ns();
    let t_flush = monotonic_ns();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _open = rig.writes.begin_tracked(Path::new("/r"));
    rig.detector.sample_now("r", t_recv);
    let e = rig
        .detector
        .evidence("r", Path::new(WT), S3Scope::Repo, None, t_recv, t_flush);
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-CLASSIFY: a window opened after the flush and still open is foreign: {:?}",
        e.counts
    );

    // (2) A window that opened after the flush, closed with success and wrote the event's new.
    let rig = self::rig();
    exiting_child(&rig, 50);
    let t_recv = monotonic_ns();
    let t_flush = monotonic_ns();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let window = rig.writes.begin_tracked(Path::new("/r"));
    window.wrote("refs/heads/feat-login", MOVED_NEW);
    window.complete();
    drop(window);
    rig.detector.sample_now("r", t_recv);
    let moved = RefMove {
        branch: "feat-login",
        old: Some(MOVED_OLD),
        new: MOVED_NEW,
    };
    let e = rig.detector.evidence(
        "r",
        Path::new(WT),
        S3Scope::Repo,
        Some(moved),
        t_recv,
        t_flush,
    );
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "GRP007-CLASSIFY: the event's new is among the values the window wrote: {:?}",
        e.counts
    );
}
