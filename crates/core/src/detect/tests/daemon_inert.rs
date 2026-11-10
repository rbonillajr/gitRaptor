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
fn daemon_inert_an_exiting_git_with_no_readable_folder_is_foreign() {
    assert_baseline_attributed();
    let rig = rig();
    git(&rig, 50, std::process::id(), None);
    rig.table.call(50, Some(&daemon_line("pack-objects")));
    let e = evaluate(&rig);
    assert_foreign_daemon(&e, "(Daemon, None) with an inert argv", 1);
    assert_eq!(e.counts.daemon_inert, 0);
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
