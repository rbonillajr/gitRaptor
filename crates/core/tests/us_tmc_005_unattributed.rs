//! The global quota windows of the `hook-prior` snapshot (per worktree, per repo) count and
//! apply only to agents: an unattributed requester answers to its own bucket alone, so it
//! neither spends the agents' ceilings nor is stopped by them.
//!
//! Every test runs on a testkit fixture: a temporary repo, home and profile. The quota clock is
//! the `now_ms` the functions take: nothing waits.

mod tm_common;

use std::path::Path;
use std::time::{Duration, Instant};

use gitraptor_core::timemachine::hook_prior::{
    self, HookPriorAsk, HookPriorError, HookPriorTaken, PER_WORKTREE_DAY,
};
use gitraptor_core::timemachine::manual::resolve_worktree;
use gitraptor_core::timemachine::oplog::{
    HookPriorMeta, NewSnapshot, Requester, RequesterOrigin, SnapshotLevel,
};
use gitraptor_testkit::Fixture;
use tm_common::{Env, REPO_ID, git};

const T0: i64 = 1_728_000_000_000;

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn take(
    env: &Env,
    worktree: &Path,
    requester: Requester,
    pid: u32,
    now_ms: i64,
) -> Result<HookPriorTaken, HookPriorError> {
    hook_prior::capture_in_store(
        &env.store,
        &env.oplog,
        &HookPriorAsk {
            repo_id: REPO_ID.into(),
            worktree: worktree.to_path_buf(),
            common_dir: gitraptor_core::observe::locate(worktree).unwrap(),
            requester,
            git: Some((pid, 1)),
        },
        None,
        false,
        None,
        now_ms,
        Instant::now() + Duration::from_secs(25),
    )
}

#[test]
fn unattributed_requests_neither_spend_nor_meet_the_agents_ceilings() {
    let env = Env::new(Fixture::with_commit(&git()));
    let wt = env.f.repo.clone();
    let (_, key) = resolve_worktree(&wt).unwrap();

    // Two unattributed points: they share one bucket of their own.
    take(&env, &wt, Requester::Unattributed, 1, T0).expect("first unattributed");
    take(&env, &wt, Requester::Unattributed, 2, T0 + 1).expect("second unattributed");
    // One agent point.
    take(&env, &wt, agent("s1"), 3, T0 + 2).expect("agent");

    {
        let log = env.oplog.lock().unwrap();
        let unattributed = log.hook_prior_quota_input(None, &key, T0 + 10).unwrap();
        assert_eq!(unattributed.requester_ms.len(), 2, "its own bucket");
        assert_eq!(unattributed.worktree_ms.len(), 1, "only the agent's row");
        assert_eq!(unattributed.repo_ms.len(), 1, "only the agent's row");
        let agent_input = log
            .hook_prior_quota_input(Some("s1"), &key, T0 + 10)
            .unwrap();
        assert_eq!(agent_input.requester_ms.len(), 1);
    }

    // Fill the agents' ceiling of the worktree, one session each.
    {
        let mut log = env.oplog.lock().unwrap();
        for i in 0..PER_WORKTREE_DAY {
            log.begin_hook_prior_snapshot(
                &NewSnapshot {
                    level: SnapshotLevel::HookPrior,
                    worktrees: vec!["main".into()],
                    engine_mark: None,
                    cause_operation: None,
                    cause_event_seq: None,
                },
                &HookPriorMeta {
                    requester: agent(&format!("full-{i}")),
                    worktree_key: key.clone(),
                    requested_ms: T0 + 100 + i as i64,
                },
            )
            .unwrap();
        }
    }
    // A new agent meets the ceiling...
    match take(&env, &wt, agent("late"), 10, T0 + 1_000_000) {
        Err(HookPriorError::Quota(hit)) => {
            assert_eq!(hit.window, gitraptor_api::methods::QuotaWindow::WorktreeDay);
        }
        other => panic!("expected the worktree ceiling, got {other:?}"),
    }
    // ...and an unattributed requester does not.
    let taken = take(&env, &wt, Requester::Unattributed, 11, T0 + 1_000_001);
    assert!(taken.is_ok(), "{taken:?}");
}
