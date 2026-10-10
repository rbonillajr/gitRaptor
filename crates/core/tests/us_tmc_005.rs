//! The `hook-prior` snapshot of the Time Machine (Brief US-TMC-005): which hook operations ask
//! for it, its deadline, its quota, one point per `git` command and no hook run by the capture.
//!
//! Every test runs on a testkit fixture: a temporary repo, home and profile, never this repo or
//! the real profile. The quota clock is the `now_ms` the functions take: nothing waits.
//!
//! Written test-first, against the contract of the Brief.

mod tm_common;

use std::path::Path;
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{CommitStage, Operation, PushUpdate, RefUpdate, RefValue};
use gitraptor_api::methods::QuotaWindow;
use gitraptor_core::timemachine::hook_prior::{
    self, HOOK_PRIOR_DEADLINE, HookPriorAsk, HookPriorError, HookPriorTaken, PER_MINUTE,
    PER_REPO_DAY,
};
use gitraptor_core::timemachine::manual::QuotaInput;
use gitraptor_core::timemachine::oplog::{
    Requester, RequesterOrigin, SnapshotFilter, SnapshotLevel, SnapshotState, SnapshotView,
};
use gitraptor_testkit::Fixture;
use tm_common::{Env, REPO_ID, git};

/// 2024-10-04, in ms: far from any real clock the code could fall back to.
const T0: i64 = 1_728_000_000_000;
const OID_A: &str = "1111111111111111111111111111111111111111";
const OID_B: &str = "2222222222222222222222222222222222222222";

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn ask(worktree: &Path, requester: Requester, git: Option<(u32, u64)>) -> HookPriorAsk {
    HookPriorAsk {
        repo_id: REPO_ID.into(),
        worktree: worktree.to_path_buf(),
        common_dir: gitraptor_core::observe::locate(worktree).unwrap(),
        requester,
        git,
    }
}

fn budget() -> Instant {
    Instant::now() + Duration::from_secs(25)
}

fn take(env: &Env, ask: &HookPriorAsk, now_ms: i64) -> Result<HookPriorTaken, HookPriorError> {
    hook_prior::capture_in_store(
        &env.store,
        &env.oplog,
        ask,
        None,
        false,
        None,
        now_ms,
        budget(),
    )
}

/// Every `hook-prior` row of the oplog, whatever its state.
fn hook_prior_rows(env: &Env) -> Vec<SnapshotView> {
    env.oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter {
            level: Some(SnapshotLevel::HookPrior),
            ..SnapshotFilter::default()
        })
        .unwrap()
}

fn complete_rows(env: &Env) -> usize {
    hook_prior_rows(env)
        .iter()
        .filter(|s| s.state == SnapshotState::Complete)
        .count()
}

fn ref_tx(lines: &[(&str, RefValue, RefValue)]) -> Operation {
    Operation::RefTransaction {
        updates: lines
            .iter()
            .map(|(name, old, new)| RefUpdate {
                refname: (*name).into(),
                old: old.clone(),
                new: new.clone(),
            })
            .collect(),
        orphan_head: None,
    }
}

// ---------------------------------------------------------------------- trigger policy (D2)

#[test]
fn trigger_policy_only_rebase_and_branch_deletion() {
    let oid = |s: &str| RefValue::Oid(s.into());
    // Moment A: a rebase (also `pull --rebase`) and the deletion of a branch.
    assert!(hook_prior::wants_prior(&Operation::Rebase {
        upstream: Some(Untrusted::new("main".to_owned())),
        branch: None,
    }));
    assert!(hook_prior::wants_prior(&ref_tx(&[(
        "refs/heads/feat-x",
        oid(OID_A),
        RefValue::Zero
    )])));
    // Git 2.50 sends `0 0 ref` for `branch -D`: the deletion is told by the new value alone.
    assert!(hook_prior::wants_prior(&ref_tx(&[(
        "refs/heads/feat-x",
        RefValue::Zero,
        RefValue::Zero
    )])));
    // A deletion among other lines still asks.
    assert!(hook_prior::wants_prior(&ref_tx(&[
        ("refs/heads/a", oid(OID_A), oid(OID_B)),
        ("refs/heads/b", oid(OID_A), RefValue::Zero),
    ])));

    // Creations and moves: the same hook arrives after `checkout -B` or `reset --hard` rewrote
    // the working tree, so a point there would mix two states.
    assert!(!hook_prior::wants_prior(&ref_tx(&[(
        "refs/heads/feat-x",
        RefValue::Zero,
        oid(OID_A)
    )])));
    assert!(!hook_prior::wants_prior(&ref_tx(&[(
        "refs/heads/feat-x",
        oid(OID_A),
        oid(OID_B)
    )])));
    // A deleted ref that is not a branch is not a branch deletion.
    assert!(!hook_prior::wants_prior(&ref_tx(&[(
        "refs/tags/v1",
        oid(OID_A),
        RefValue::Zero
    )])));
    // A push changes nothing local; a commit destroys nothing.
    assert!(!hook_prior::wants_prior(&Operation::Push {
        remote: Untrusted::new("origin".to_owned()),
        updates: vec![PushUpdate {
            local_ref: None,
            local: RefValue::Zero,
            remote_ref: "refs/heads/feat-x".into(),
            remote: oid(OID_A),
        }],
    }));
    for stage in [
        CommitStage::PreCommit,
        CommitStage::CommitMsg,
        CommitStage::SecondLine,
    ] {
        assert!(!hook_prior::wants_prior(&Operation::Commit { stage }));
    }
}

// ---------------------------------------------------------------------- deadline (D5)

#[test]
fn timeout_fails_and_records_no_point() {
    let env = Env::new(Fixture::with_commit(&git()));
    env.f.write("api.rs", "fn api() {}\n");
    let wt = env.f.repo.clone();
    // A deadline that is already over: the answer is `failed` by time, never a point.
    let result = hook_prior::capture_in_store(
        &env.store,
        &env.oplog,
        &ask(&wt, agent("s1"), Some((4242, 7))),
        None,
        false,
        None,
        T0,
        Instant::now(),
    );
    assert!(
        matches!(result, Err(HookPriorError::TimeLimit)),
        "expected a time limit, got {result:?}"
    );
    assert_eq!(
        complete_rows(&env),
        0,
        "a late capture never becomes a point"
    );
}

#[test]
fn timeout_deadline_fits_in_the_client_call_timeout() {
    // The hook client waits 10 s for `guard.evaluate` (`CALL_TIMEOUT`); past it, the hook denies
    // with an internal error. The deadline leaves at least half of it to the evaluation.
    assert_eq!(hook_prior::deadline(), HOOK_PRIOR_DEADLINE);
    assert!(HOOK_PRIOR_DEADLINE * 2 <= Duration::from_secs(10));
}

// ---------------------------------------------------------------------- quota (D6)

#[test]
fn quota_refuses_the_eleventh_in_a_minute_and_counts_per_repo() {
    let env = Env::new(Fixture::with_commit(&git()));
    let wt = env.f.repo.clone();
    for i in 0..PER_MINUTE as i64 {
        // A different `git` each time: separate commands, no reuse.
        let git = Some((1000 + i as u32, 1));
        let taken = take(&env, &ask(&wt, agent("s1"), git), T0 + i * 1_000);
        assert!(taken.is_ok(), "attempt {i}: {taken:?}");
    }
    let refused = take(&env, &ask(&wt, agent("s1"), Some((2000, 1))), T0 + 30_000);
    match refused {
        Err(HookPriorError::Quota(hit)) => {
            assert_eq!(hit.window, QuotaWindow::Minute);
            assert_eq!(hit.release_at_ms, T0 + 60_000);
        }
        other => panic!("expected a quota refusal, got {other:?}"),
    }
    // Another client is not refused by s1's minute.
    let other = take(&env, &ask(&wt, agent("s2"), Some((2001, 1))), T0 + 31_000);
    assert!(other.is_ok(), "{other:?}");
    // Nothing was deleted to make room: every point taken is still complete.
    assert_eq!(complete_rows(&env), PER_MINUTE + 1);

    // The repo's ceiling, pure: everybody together.
    let full = QuotaInput {
        requester_ms: Vec::new(),
        worktree_ms: Vec::new(),
        repo_ms: (0..PER_REPO_DAY as i64).map(|i| T0 + i).collect(),
    };
    let hit = hook_prior::quota(&full, T0 + 3_600_000).unwrap_err();
    assert_eq!(hit.window, QuotaWindow::RepoDay);
    assert!(hook_prior::quota(&QuotaInput::default(), T0).is_ok());
}

// ---------------------------------------------------------------------- untrusted .git (#226, D9)

/// The worktree a hook prior reads goes through the same gate as every Time Machine capture
/// (`observe::open_registered_worktree` → `open_worktree`): a folder whose `.git` names a
/// worktree of the repo without being it, and a registered worktree whose `.git` was rewritten
/// to point at another repo, are refused before anything is recorded or read behind them.
#[test]
fn untrusted_worktree_takes_no_hook_prior() {
    use gitraptor_core::timemachine::store::CaptureError;
    use gitraptor_git::ReadError;
    let env = Env::new(Fixture::with_commit(&git()));
    let common = gitraptor_core::observe::locate(&env.f.repo).unwrap();
    env.f.git(&["branch", "a"]);
    let a = gitraptor_core::observe::canonical(&env.f.add_worktree("a", "a"));
    let refs_before = env.store_git(&["for-each-ref"]);

    // (1) A folder the repo does not register, whose `.git` points into the repo's admin dir.
    let posing = env.f.root.join("posing");
    std::fs::create_dir_all(&posing).unwrap();
    let admin = std::fs::read_to_string(a.join(".git")).unwrap();
    std::fs::write(posing.join(".git"), &admin).unwrap();
    std::fs::write(posing.join("api.rs"), "stolen\n").unwrap();
    // (2) The registered worktree, its `.git` rewritten to point at another repo.
    let theirs = Fixture::with_commit(&git());
    theirs.git(&["branch", "x"]);
    theirs.add_worktree("x", "x");
    let their_admin = gitraptor_core::observe::locate(&theirs.repo)
        .unwrap()
        .join("worktrees")
        .join("wt-x");
    std::fs::write(
        a.join(".git"),
        format!("gitdir: {}\n", their_admin.to_str().unwrap()),
    )
    .unwrap();

    for (what, root) in [("posing folder", &posing), ("rewritten .git", &a)] {
        let ask = HookPriorAsk {
            repo_id: REPO_ID.into(),
            worktree: root.clone(),
            common_dir: common.clone(),
            requester: agent("s1"),
            git: Some((7, 7)),
        };
        let result = take(&env, &ask, T0);
        assert!(
            matches!(
                result,
                Err(HookPriorError::NoWorktree)
                    | Err(HookPriorError::Capture(CaptureError::Read(
                        ReadError::Untrusted(_)
                    )))
            ),
            "{what}: {result:?}"
        );
    }
    assert!(
        hook_prior_rows(&env).is_empty(),
        "a refused worktree left a row"
    );
    assert_eq!(
        env.store_git(&["for-each-ref"]),
        refs_before,
        "a point was recorded"
    );
}

// ---------------------------------------------------------------------- one per command, no recursion (D3)

/// Unix only: the testkit canary (SEC-09) is not ported to Windows yet (XP-08).
#[cfg(unix)]
#[test]
fn one_point_per_git_command_and_no_hook_runs() {
    use gitraptor_testkit::canary::Canary;
    let c = Canary::arm(Fixture::busy(&git()));
    let markers = c.markers.clone();
    let env = Env::new(c.f);
    let wt = env.f.repo.clone();
    // The setup's own `git` commands ran the armed programs: only the capture counts.
    for m in std::fs::read_dir(&markers).unwrap() {
        std::fs::remove_file(m.unwrap().path()).unwrap();
    }
    let git = Some((4242, 99));
    let first = take(&env, &ask(&wt, agent("s1"), git), T0).expect("first hook prior");
    // The second `prepared` of the same deletion (packed-refs, then the loose ref).
    let second = take(&env, &ask(&wt, agent("s1"), git), T0 + 5).expect("same command");
    assert!(!first.reused);
    assert!(second.reused);
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert_eq!(hook_prior_rows(&env).len(), 1, "one point per git command");
    // The capture reads with gitoxide and writes only the store: no hook, filter or program of
    // the user's repo runs, so a snapshot never asks for another one.
    let fired: Vec<String> = std::fs::read_dir(&markers)
        .unwrap()
        .map(|m| m.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(fired.is_empty(), "programs run by the capture: {fired:?}");
}
