//! A hook prior is taken while the `git` that ran the hook is alive in the worktree
//! (fix/tm-restore-calm-mark, D17): the calm leaves out the hook's client chain and its own call.
//! With the chain unknown nothing is left out, and the prior ends at its deadline.
//!
//! A testkit fixture and the synthetic process table of `calm_git_alive_tests`; never this repo or
//! the real profile (NFR-01). The deadline is the test's own; nothing sleeps.

use std::sync::Arc;

use super::*;
use crate::daemon::{LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::engine::calm_git_alive_tests::{GIT, Marks, REPO, Rig, rig};
use crate::timemachine::oplog::{RequesterOrigin, SnapshotFilter};
use crate::timemachine::protected::TmRepos;

const T0: i64 = 1_728_000_000_000;

fn rig_deps(r: &Rig) -> (CaptureDeps, Arc<Mutex<Oplog>>) {
    let dirs = ProfileDirs::under_root(&r.f.profile);
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    repos.insert(REPO, &r.common, oplog);
    let oplog = repos.oplog(REPO).unwrap();
    let deps = CaptureDeps {
        repos,
        engine: Arc::new(Marks(Arc::clone(&r.marks))),
        profile: dirs.clone(),
        logger: Logger::open(&dirs.state, LogLimits::default()).unwrap(),
        layer: None,
        free_space_floor: None,
        after_mark: None,
    };
    (deps, oplog)
}

fn ask(r: &Rig) -> HookPriorAsk {
    HookPriorAsk {
        repo_id: REPO.into(),
        worktree: r.wt.clone(),
        common_dir: r.common.clone(),
        requester: Requester::Agent {
            name: "claude-code".into(),
            origin: RequesterOrigin::Detected,
            session_id: "s1".into(),
        },
        git: None,
    }
}

/// C24: with its client chain left out, the prior is sealed with a mark; with the chain unknown,
/// the live `git` holds the calm until the deadline.
#[test]
fn a_hook_prior_with_its_live_git_in_scope_seals_with_a_mark() {
    // The chain is known.
    let r = rig();
    r.procs.add(42, 1, GIT, Some(&r.wt), &["commit"]);
    let (deps, oplog) = rig_deps(&r);
    let hook = deps.engine.hook_begin(REPO, &r.wt);
    let ex = CalmExclusions {
        pids: &[42],
        own_hook: Some(hook.token()),
    };
    let out = capture_excluding(
        &deps,
        &ask(&r),
        &ex,
        T0,
        Instant::now() + Duration::from_secs(25),
    );
    assert!(out.is_ok(), "the hook's own git does not hold it: {out:?}");
    let sealed = oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .into_iter()
        .any(|s| s.record.level == SnapshotLevel::HookPrior && s.record.engine_mark == Some(1));
    assert!(sealed, "the prior carries the engine's mark");
    drop(hook);

    // The chain is unknown: nothing is left out.
    let r = rig();
    r.procs.add(42, 1, GIT, Some(&r.wt), &["commit"]);
    let (deps, _) = rig_deps(&r);
    let hook = deps.engine.hook_begin(REPO, &r.wt);
    let ex = CalmExclusions {
        pids: &[],
        own_hook: Some(hook.token()),
    };
    let deadline = Instant::now() + Duration::from_millis(300);
    let out = capture_excluding(&deps, &ask(&r), &ex, T0, deadline);
    assert!(
        matches!(out, Err(HookPriorError::TimeLimit)),
        "an unknown chain fails closed at the deadline: {out:?}"
    );
}
