//! More of the manual snapshot of the Time Machine (DS-US-MCP-008 T003 and T004), next to the
//! red contract suite of `us_mcp_008.rs`: the guard of the atomic quota, an attempt in flight,
//! an operation in progress, the manual points in the timeline, the undo stack and the label as
//! data.
//!
//! Every test runs on a testkit fixture: a temporary repo, home and profile, never this repo or
//! the real profile (NFR-01). The clock is the `now_ms` the capture takes; the waits are states
//! with a deadline.

mod tm_common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::time::{Duration, Instant};

use gitraptor_core::timemachine::manual::{
    self, ManualAsk, ManualCaptured, ManualError, PER_DAY, PER_WORKTREE_DAY,
};
use gitraptor_core::timemachine::oplog::{
    Channel, OpRef, OperationKind, OperationState, OperationTransition, Requester, RequesterOrigin,
    Scope, SnapshotLevel, StackScope, Target,
};
use gitraptor_core::timemachine::store::snapshot_refs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint::{Scope as FpScope, Snapshot, diff};
use rusqlite::Connection;
use tm_common::{Env, REPO_ID, git};

const T0: i64 = 1_728_000_000_000;
const DEADLINE: Duration = Duration::from_secs(20);

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn ask(worktree: &Path, session: &str) -> ManualAsk {
    ManualAsk {
        repo_id: REPO_ID.into(),
        worktree: worktree.to_path_buf(),
        label: "before the migration".into(),
        requester: agent(session),
        channel: Channel::Mcp,
    }
}

fn take_marked(
    env: &Env,
    ask: &ManualAsk,
    mark: Option<i64>,
    now_ms: i64,
) -> Result<ManualCaptured, ManualError> {
    manual::capture_in_store(
        &env.store,
        &env.oplog,
        ask,
        mark,
        false,
        None,
        now_ms,
        Instant::now() + Duration::from_secs(25),
    )
}

fn take(env: &Env, ask: &ManualAsk, now_ms: i64) -> Result<ManualCaptured, ManualError> {
    take_marked(env, ask, None, now_ms)
}

fn env() -> Env {
    Env::new(Fixture::with_commit(&git()))
}

fn worktree(env: &Env, name: &str) -> PathBuf {
    env.f.git(&["branch", name]);
    env.f.add_worktree(name, name)
}

fn manual_rows(env: &Env) -> i64 {
    Connection::open(env.tm_root().join(REPO_ID).join("oplog.db"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM snapshots WHERE level = 'manual'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn refs(env: &Env) -> BTreeSet<String> {
    snapshot_refs(&env.store).unwrap().into_keys().collect()
}

// ------------------------------------------------------------------------- the atomic quota

/// ⛔3.2: the quota and the recording of the attempt go under one lock. Two requests at the edge
/// of the worktree ceiling never both pass: the sixtieth is recorded, the sixty-first never is.
#[test]
fn two_requests_at_the_edge_never_record_the_twenty_first() {
    let env = env();
    let wt = env.f.repo.clone();
    for i in 0..PER_WORKTREE_DAY - 1 {
        take(&env, &ask(&wt, &format!("session-{i}")), T0 + i as i64).expect("below the ceiling");
    }
    let barrier = Barrier::new(2);
    let outcomes: Vec<Result<ManualCaptured, ManualError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ["racer-a", "racer-b"]
            .into_iter()
            .map(|session| {
                let (env, wt, barrier) = (&env, &wt, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    take(env, &ask(wt, session), T0 + 1_000)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let taken = outcomes.iter().filter(|o| o.is_ok()).count();
    let refused = outcomes
        .iter()
        .filter(|o| matches!(o, Err(ManualError::Quota(_))))
        .count();
    assert_eq!((taken, refused), (1, 1), "{outcomes:?}");
    assert_eq!(manual_rows(&env), PER_WORKTREE_DAY as i64);
}

#[test]
fn two_connections_of_the_same_requester_never_pass_the_edge_together() {
    let env = env();
    let wt = env.f.repo.clone();
    for i in 0..PER_DAY as i64 - 1 {
        take(&env, &ask(&wt, "s1"), T0 + i * 61_000).expect("below the quota");
    }
    let now = T0 + PER_DAY as i64 * 61_000;
    let barrier = Barrier::new(2);
    let outcomes: Vec<Result<ManualCaptured, ManualError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (env, wt, barrier) = (&env, &wt, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    take(env, &ask(wt, "s1"), now)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        outcomes.iter().filter(|o| o.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(manual_rows(&env), PER_DAY as i64, "never the twenty-first");
}

#[test]
fn a_full_ceiling_never_blocks_observation_or_priors() {
    let env = env();
    let wt = env.f.repo.clone();
    for i in 0..PER_WORKTREE_DAY {
        take(&env, &ask(&wt, &format!("session-{i}")), T0 + i as i64).unwrap();
    }
    assert!(matches!(
        take(&env, &ask(&wt, "late"), T0 + 100),
        Err(ManualError::Quota(_))
    ));
    let observation = env.capture(SnapshotLevel::Observation, None);
    let prior = env.prior();
    let known = refs(&env);
    assert!(known.iter().any(|r| r.ends_with(&observation.snapshot_id)));
    assert!(known.iter().any(|r| r.ends_with(&prior.snapshot_id)));
}

#[test]
fn the_quota_is_per_requester_and_worktree() {
    let env = env();
    let a = env.f.repo.clone();
    let b = worktree(&env, "other");
    for i in 0..5 {
        take(&env, &ask(&a, "s1"), T0 + i).unwrap();
    }
    assert!(matches!(
        take(&env, &ask(&a, "s1"), T0 + 6),
        Err(ManualError::Quota(_))
    ));
    // Another worktree of the same requester, and another requester in the same worktree.
    take(&env, &ask(&b, "s1"), T0 + 7).expect("the minute quota is per worktree");
    take(&env, &ask(&a, "s2"), T0 + 8).expect("the minute quota is per requester");
}

// --------------------------------------------------------------------- in flight and in progress

/// A worktree heavy enough for a capture to last a moment.
fn heavy(env: &Env) {
    for i in 0..30u64 {
        let mut bytes = vec![0u8; 1024 * 1024];
        let mut x = 0x9E37_79B9_7F4A_7C15u64 ^ (i + 1);
        for chunk in bytes.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let word = x.to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        std::fs::write(env.f.repo.join(format!("blob-{i}.bin")), bytes).unwrap();
    }
}

#[test]
fn a_second_snapshot_of_the_same_requester_in_flight_is_refused() {
    let env = env();
    heavy(&env);
    let wt = env.f.repo.clone();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| take(&env, &ask(&wt, "s1"), T0));
        // The attempt is on the oplog once the capture is under way.
        let start = Instant::now();
        while manual_rows(&env) == 0 {
            assert!(start.elapsed() < DEADLINE, "the capture never started");
            assert!(!first.is_finished(), "it ended without ever recording");
            std::thread::yield_now();
        }
        let asked = Instant::now();
        let second = take(&env, &ask(&wt, "s1"), T0 + 1);
        assert!(
            matches!(second, Err(ManualError::InFlight)),
            "the same requester in flight is refused, got {second:?}"
        );
        assert!(
            asked.elapsed() < Duration::from_secs(2),
            "refused at once, not after waiting for the first"
        );
        first.join().unwrap().ok();
    });
    // The refused one left no row of its own.
    assert_eq!(manual_rows(&env), 1);
}

#[test]
fn a_rebase_in_progress_refuses_the_capture_and_records_nothing() {
    let env = env();
    let head = env.f.git(&["rev-parse", "HEAD"]);
    std::fs::write(
        env.f.repo.join(".git").join("MERGE_HEAD"),
        format!("{head}\n"),
    )
    .unwrap();
    let before = refs(&env);
    let result = take(&env, &ask(&env.f.repo, "s1"), T0);
    assert!(matches!(result, Err(ManualError::InProgress)), "{result:?}");
    assert_eq!(
        manual_rows(&env),
        0,
        "a refusal before capture is no attempt"
    );
    assert_eq!(refs(&env), before);
}

#[test]
fn an_unattributed_requester_never_captures() {
    let env = env();
    let mut request = ask(&env.f.repo, "s1");
    request.requester = Requester::Unattributed;
    assert!(take(&env, &request, T0).is_err());
    assert_eq!(manual_rows(&env), 0);
}

#[test]
fn an_invalid_label_never_reaches_the_oplog() {
    let env = env();
    let mut request = ask(&env.f.repo, "s1");
    request.label = "bad\u{202e}label".into();
    assert!(take(&env, &request, T0).is_err());
    request.label = String::new();
    assert!(take(&env, &request, T0).is_err());
    assert_eq!(manual_rows(&env), 0);
}

// ------------------------------------------------------------------------------------- NFR-01

#[test]
fn a_manual_snapshot_leaves_the_repo_byte_identical() {
    let env = Env::busy();
    let linked = worktree(&env, "side");
    // `git status` refreshes the index of a stat-dirty repo: take the "before" after it.
    env.f.git(&["status", "--porcelain=v2", "--branch"]);
    env.f
        .git_in(&linked, &["status", "--porcelain=v2", "--branch"]);
    let scopes = [
        FpScope::new("main", env.f.repo.clone()),
        FpScope::new("side", linked.clone()),
    ];
    let before = Snapshot::take(&scopes, &BTreeSet::new());
    take(&env, &ask(&env.f.repo, "s1"), T0).unwrap();
    take(&env, &ask(&linked, "s1"), T0 + 1).unwrap();
    let changes = diff(&before, &Snapshot::take(&scopes, &BTreeSet::new()));
    assert!(
        changes.is_empty(),
        "the repo and its .git changed: {changes:#?}"
    );
}

#[test]
fn the_label_never_reaches_a_ref_path_or_log() {
    let env = env();
    let label = "../../etc/passwd; $(touch pwned) refs/heads/main";
    let mut request = ask(&env.f.repo, "s1");
    request.label = label.into();
    let point = take(&env, &request, T0).unwrap();
    // The store lists its refs by the id after `refs/tm/snap/`: always a bare uuid.
    for id in refs(&env) {
        assert!(id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
    }
    assert!(!env.f.repo.join("pwned").exists());
    let store_root = env.tm_root().join(REPO_ID);
    let named = std::fs::read_dir(&store_root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("passwd"));
    assert!(!named, "the label never names a file of the store");
    assert!(env.store.meta(&point.snapshot_id).is_ok());
}

// ------------------------------------------------------------------------------ the undo stack

#[test]
fn the_undo_stack_ignores_manual_snapshots() {
    let env = env();
    let root = env
        .f
        .repo
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let prior = env.prior().snapshot_id;
    let op = {
        let mut log = env.oplog.lock().unwrap();
        let id = log
            .record_operation(
                &gitraptor_core::timemachine::oplog::NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![root.clone()],
                        refs: vec![],
                    },
                    requester: agent("s1"),
                    channel: Channel::Mcp,
                    confirmed: false,
                    target: Target::None,
                    warnings: vec![],
                    engine_mark: 1,
                },
                10,
            )
            .unwrap();
        log.advance_operation(
            &id,
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior,
            },
            11,
        )
        .unwrap();
        log.advance_operation(&id, OperationTransition::Ready, 12)
            .unwrap();
        log.advance_operation(&id, OperationTransition::Applying { step: 1 }, 13)
            .unwrap();
        log.advance_operation(&id, OperationTransition::Finished, 14)
            .unwrap();
        assert_eq!(
            log.operation(&id).unwrap().unwrap().state,
            OperationState::Finished
        );
        id
    };
    // A manual snapshot after the operation: the next undo is still the operation.
    take(&env, &ask(&env.f.repo, "s1"), T0).unwrap();
    let stack = env
        .oplog
        .lock()
        .unwrap()
        .undo_stack(&StackScope::Worktree(root), &[])
        .unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::Oplog(op)));
}
