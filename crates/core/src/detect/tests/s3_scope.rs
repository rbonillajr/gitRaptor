//! Contract tests of the S3 scope: where a foreign `git` counts for a Git
//! event, and what one evaluation counted.
//!
//! The repo of `Rig::new` is `/r` (main, common dir `/r/.git`), with the
//! worktrees `/wt/feat-login` and the nested `/r/.claude/worktrees/x`.

use super::*;

const WT: &str = "/wt/feat-login";

/// A rig with the session `20:2000` in [`WT`] and its `git` running there.
fn agent_with_git() -> Rig {
    let rig = Rig::new();
    rig.claude(20, 2_000, WT);
    rig.scan();
    git(&rig, 31, 20, Some(WT));
    rig
}

/// The `git` of the daemon itself, in `cwd`.
fn daemon_git(rig: &Rig, pid: u32, cwd: &str) {
    let me = std::process::id();
    rig.table.add(me, 10, 1_000, "/opt/raptor", Some("/"));
    git(rig, pid, me, Some(cwd));
}

/// The dogfooding case: Orca runs `git` in the main worktree while the
/// agent commits in its own one.
#[test]
fn s3_worktree_scope_ignores_a_foreign_git_of_another_worktree() {
    let rig = agent_with_git();
    git(&rig, 41, 10, Some("/r"));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    match &e.outcome {
        S3Outcome::Attributed(p) => assert_eq!(p.session_id, "20:2000"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_other_wt: 1,
            ..S3Counts::default()
        }
    );
}

/// BR-EDGE-004: a person working in the agent's worktree keeps the event
/// ambiguous, with a readable folder or one placed by its ancestor.
#[test]
fn s3_worktree_scope_a_person_in_the_agents_worktree_is_foreign() {
    let rig = agent_with_git();
    git(&rig, 41, 10, Some("/wt/feat-login/src"));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_wt: 1,
            ..S3Counts::default()
        }
    );

    let rig = agent_with_git();
    // An exiting `git` under the person's shell in the worktree.
    rig.table.add(60, 10, 1_500, "/bin/zsh", Some(WT));
    git(&rig, 51, 60, None);
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_by_ancestor: 1,
            ..S3Counts::default()
        }
    );
}

/// The folder of the ancestor says where the shell is, not where its `git`
/// wrote: it counts whatever worktree it is in.
#[test]
fn s3_worktree_scope_a_git_placed_by_its_ancestor_is_foreign_from_any_worktree() {
    let rig = agent_with_git();
    rig.table.add(60, 10, 1_500, "/bin/zsh", Some("/r"));
    git(&rig, 51, 60, None);
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(e.counts.foreign_by_ancestor, 1);
    assert_eq!(e.counts.foreign_other_wt, 0);
    assert_eq!(e.counts.sessions_wt, 1);
}

/// The daemon's `git` (the Time Machine writer) counts anywhere in the
/// repo: in the common dir of the event's worktree or in another worktree.
#[test]
fn s3_worktree_scope_counts_the_daemons_git_anywhere_in_the_repo() {
    let rig = agent_with_git();
    daemon_git(&rig, 50, "/r/.git/worktrees/feat-login");
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_daemon: 1,
            ..S3Counts::default()
        }
    );

    let rig = agent_with_git();
    daemon_git(&rig, 50, "/r");
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(e.counts.foreign_daemon, 1);
    assert_eq!(e.counts.foreign_other_wt, 0);
}

/// `/r/.git` starts with `/r`: the common dir is checked before the
/// worktree, or this `git` would pass for one of another worktree.
#[test]
fn s3_worktree_scope_counts_a_foreign_git_in_the_common_dir() {
    let rig = agent_with_git();
    git(&rig, 41, 10, Some("/r/.git"));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_gitdir: 1,
            ..S3Counts::default()
        }
    );
}

/// Shared refs and a worktree placed by a fallback use the repo scope: a
/// foreign `git` of another worktree still makes the event ambiguous.
#[test]
fn s3_repo_scope_keeps_a_foreign_git_of_another_worktree_ambiguous() {
    let rig = agent_with_git();
    git(&rig, 41, 10, Some("/r"));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Repo, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_other_wt: 1,
            ..S3Counts::default()
        }
    );
}

/// `/r/.claude/worktrees/x` starts with `/r`: a session `git` there is
/// evidence for its own worktree, never for the main one.
#[test]
fn s3_the_worktree_of_a_session_git_is_the_longest_root() {
    const NESTED: &str = "/r/.claude/worktrees/x";
    let rig = Rig::new();
    rig.claude(20, 2_000, NESTED);
    rig.scan();
    git(&rig, 31, 20, Some(NESTED));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in("/r", S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::NoSighting);
    assert_eq!(e.counts.sessions_wt, 0);
    let e = rig.evidence_in(NESTED, S3Scope::Worktree, 1_000);
    assert!(matches!(e.outcome, S3Outcome::Attributed(_)), "{e:?}");
    assert_eq!(e.counts.sessions_wt, 1);

    // A foreign `git` in the nested worktree is of another worktree for an
    // event of the main one. A later window, so only this sample counts.
    git(&rig, 41, 10, Some("/r/.claude/worktrees/x/src"));
    rig.detector.sample_now("r", 900_000_000);
    let e = rig.evidence_in("/r", S3Scope::Worktree, 900_000_000);
    assert_eq!(e.outcome, S3Outcome::NoSighting);
    assert_eq!(e.counts.foreign_other_wt, 1);
    assert_eq!(e.counts.foreign_wt, 0);
}

#[test]
fn s3_worktree_scope_two_sessions_stay_ambiguous_without_hint() {
    let rig = Rig::new();
    rig.claude(20, 2_000, WT);
    rig.claude(21, 2_500, WT);
    rig.scan();
    git(&rig, 31, 20, Some(WT));
    git(&rig, 32, 21, Some(WT));
    git(&rig, 41, 10, Some("/r"));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 2,
            foreign_other_wt: 1,
            ..S3Counts::default()
        }
    );
    assert_eq!(rig.detector.single_session("r", Path::new(WT)), None);
}

/// A foreign `git` of another worktree does not turn "nobody saw it" into
/// "ambiguous": the single-session hint stays available.
#[test]
fn s3_no_sighting_keeps_the_hint_with_git_in_other_worktrees() {
    let rig = Rig::new();
    rig.claude(20, 2_000, WT);
    rig.scan();
    git(&rig, 41, 10, Some("/r"));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::NoSighting);
    assert_eq!(
        e.counts,
        S3Counts {
            foreign_other_wt: 1,
            ..S3Counts::default()
        }
    );
    assert_eq!(
        rig.detector
            .single_session("r", Path::new(WT))
            .map(|p| p.session_id)
            .as_deref(),
        Some("20:2000")
    );
}

#[test]
fn s3_counts_gits_started_after_the_notice() {
    let rig = Rig::new();
    rig.claude(20, 2_000, WT);
    rig.scan();
    rig.table
        .add(31, 20, wall_us() + 60_000_000, "/usr/bin/git", Some(WT));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::NoSighting);
    assert_eq!(
        e.counts,
        S3Counts {
            gits_after_notice: 1,
            ..S3Counts::default()
        }
    );
}

/// A foreign `git` whose folder is in another worktree but that redirects
/// its target (`-C`, `--git-dir`, `--work-tree`, `GIT_DIR`...) may write in
/// the event's worktree: it counts in the whole repo.
#[test]
fn s3_worktree_scope_a_redirected_foreign_git_counts_in_the_whole_repo() {
    let rig = agent_with_git();
    git(&rig, 41, 10, Some("/r"));
    rig.table.redirect(41, Some(true));
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_redirected: 1,
            ..S3Counts::default()
        }
    );
}

/// Fail-safe: when the argv or the environment of a foreign `git` cannot be
/// read, its folder is not trusted and it counts in the whole repo.
#[test]
fn s3_worktree_scope_an_unreadable_foreign_git_counts_in_the_whole_repo() {
    let rig = agent_with_git();
    git(&rig, 41, 10, Some("/r"));
    rig.table.redirect(41, None);
    rig.detector.sample_now("r", 1_000);
    let e = rig.evidence_in(WT, S3Scope::Worktree, 1_000);
    assert_eq!(e.outcome, S3Outcome::Ambiguous);
    assert_eq!(
        e.counts,
        S3Counts {
            sessions_wt: 1,
            foreign_redirected: 1,
            ..S3Counts::default()
        }
    );
}
