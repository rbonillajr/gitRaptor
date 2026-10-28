//! US-TMC-023: `restore` takes an id prefix of 6 or more characters, and the timeline tells the CLI how long a short id
//! must be. A real daemon (in-process) over a temporary repo and profile, as the other restore
//! tests; never this repo or the real profile (NFR-01). No test waits on time.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId};
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{RestoreResult, TimelineResult};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause, TmCapture};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::oplog::{OperationFilter, Oplog};
use gitraptor_core::timemachine::oplog::{
    OperationKind, OperationState, SnapshotLevel, SnapshotRecord, SnapshotState, SnapshotView,
    Target,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput,
    StepScope,
};
use gitraptor_core::timemachine::restore::{RestoreError, resolve_point_prefix};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::diff;
use serde_json::{Value, json};
use us_tmc_013_common::*;

const DIRTY: &str = "fn a() { antes_del_punto(); }\n";

/// A daemon over a repo whose `a.rs` held uncommitted work when the point was taken; a reset has
/// since thrown it away. Returns the daemon and the point (the reset's prior snapshot).
fn with_a_point() -> (Running, String) {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    std::fs::write(r.worktree.join("a.rs"), DIRTY).unwrap();
    let point = r.reset_hard().prior_snapshot_id;
    assert_ne!(r.a_rs(), DIRTY, "the reset discarded the work");
    (r, point)
}

fn view(id: &str, worktree: &str, recorded_ms: i64) -> SnapshotView {
    SnapshotView {
        record: SnapshotRecord {
            snapshot_id: id.into(),
            seq: 1,
            level: SnapshotLevel::HookPrior,
            worktrees: vec![worktree.into()],
            store_ref: format!("refs/gitraptor/{id}"),
            engine_mark: None,
            cause_operation: None,
            cause_event_seq: None,
            recorded_ms,
            manual: None,
            hook_prior: None,
        },
        state: SnapshotState::Complete,
        complete: None,
        tampered: false,
    }
}

const FIRST: &str = "47bfc884-0000-4000-8000-000000000001";
const SECOND: &str = "47bfc8e1-0000-4000-8000-000000000002";
const OTHER: &str = "9a1c0d22-0000-4000-8000-000000000003";

fn points() -> Vec<SnapshotView> {
    vec![
        view(FIRST, "/w/uxtl", 1_792_228_800_000),
        view(SECOND, "/w/main", 1_792_190_460_000),
        view(OTHER, "/w/uxtl", 1_792_230_000_000),
    ]
}

/// TMC023-RESTORE-PREFIX: the prefix restores as the full id does, same record and same prior.
#[test]
fn a_unique_prefix_restores_like_the_full_id() {
    let (r, point) = with_a_point();
    let mut c = r.client();

    let done: RestoreResult = serde_json::from_value(
        c.call(
            methods::TM_RESTORE,
            restore_params(&r.worktree, &point[..6], None),
        )
        .unwrap_or_else(|e| panic!("the restore by prefix failed: {e:?}")),
    )
    .unwrap();

    assert_eq!(r.a_rs(), DIRTY);
    // The record names the full id, as a restore by full id does, and keeps its prior state.
    assert_eq!(done.target_snapshot_id, point);
    let restores = r.requests(OperationKind::Restore);
    let ran = restores.last().expect("the restore is recorded");
    assert_eq!(ran.view.state, OperationState::Finished);
    assert_eq!(ran.view.record.target, Target::Snapshot(point.clone()));
    assert_eq!(
        ran.view.prior_snapshot.as_deref(),
        Some(done.prior_snapshot_id.as_str())
    );
}

/// TMC023-RESTORE-AMBIGUOUS: two points share the prefix, so it names none; under 6 characters it
/// is refused too, and neither records an operation nor touches the worktree.
#[test]
fn an_ambiguous_prefix_is_refused_with_its_candidates() {
    let points = points();

    match resolve_point_prefix(&points, "47bfc8") {
        Err(RestoreError::Ambiguous(data)) => {
            assert_eq!(data.total, 2);
            let mut ids: Vec<&str> = data
                .candidates
                .iter()
                .map(|c| c.snapshot_id.as_str())
                .collect();
            ids.sort_unstable();
            assert_eq!(ids, [FIRST, SECOND]);
            let first = data
                .candidates
                .iter()
                .find(|c| c.snapshot_id == FIRST)
                .unwrap();
            assert_eq!(first.occurred_utc_ms, 1_792_228_800_000);
            assert_eq!(first.worktrees.len(), 1);
            assert_eq!(first.worktrees[0].raw(), "/w/uxtl");
        }
        other => panic!("expected an ambiguous prefix, got {other:?}"),
    }
    // A unique prefix, one with the dash, and the full id name their point.
    assert_eq!(
        resolve_point_prefix(&points, "47bfc88").as_deref().ok(),
        Some(FIRST)
    );
    assert_eq!(
        resolve_point_prefix(&points, "47bfc884-00").as_deref().ok(),
        Some(FIRST)
    );
    assert_eq!(
        resolve_point_prefix(&points, FIRST).as_deref().ok(),
        Some(FIRST)
    );
    // Too short, or no point at all.
    assert!(
        resolve_point_prefix(&points, "9a1c0").is_err(),
        "5 characters are too few even when unique"
    );
    assert!(resolve_point_prefix(&points, "9a1").is_err());
    assert!(matches!(
        resolve_point_prefix(&points, "ffff"),
        Err(RestoreError::NotFound)
    ));

    // Through the daemon: a prefix under 6 characters is a usage error that changes nothing.
    let (r, _point) = with_a_point();
    let before = r.fx.fingerprint();
    let mut c = r.client();
    let (error, _) = rejected(c.call(
        methods::TM_RESTORE,
        restore_params(&r.worktree, "abcde", None),
    ));
    assert_eq!(error, code::INVALID_PARAMS);
    assert!(r.requests(OperationKind::Restore).is_empty());
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
}

/// TMC023-SHORT-ID: the length comes only when asked, covers each point and never ends in a dash.
#[test]
fn the_engine_returns_the_unique_short_id_length_only_when_asked() {
    let (r, _point) = with_a_point();
    let mut c = r.client();
    let wt = r.worktree.to_str().unwrap();

    let asked: Value = c
        .call(
            methods::TM_TIMELINE,
            json!({ "worktree": wt, "short_ids": true }),
        )
        .unwrap();
    let mut with_a_point_seen = false;
    for entry in asked["entries"].as_array().unwrap() {
        let Some(id) = entry["protection"]["snapshot_id"].as_str() else {
            assert!(
                entry.get("short_id_len").is_none(),
                "no point, no length: {entry}"
            );
            continue;
        };
        with_a_point_seen = true;
        let len = entry["short_id_len"]
            .as_u64()
            .unwrap_or_else(|| panic!("an entry with a point has a short_id_len: {entry}"))
            as usize;
        assert!((8..=36).contains(&len), "{len}");
        assert!(!id[..len].ends_with('-'), "{id} cut at {len}");
    }
    assert!(with_a_point_seen, "the timeline has an entry with a point");

    // Not asked: the answer has no such field, so `--json` does not change.
    let plain: Value = c
        .call(methods::TM_TIMELINE, json!({ "worktree": wt }))
        .unwrap();
    assert!(
        plain["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e.get("short_id_len").is_none()),
        "{plain}"
    );
    // The same answer still reads as the typed result.
    serde_json::from_value::<TimelineResult>(plain).unwrap();
}

// ----- Two observed repos in one daemon (TMC023-SCOPE) -------------------------------------------

struct TwoCatalog(Arc<Fixture>);

struct ResetIn {
    fx: Arc<Fixture>,
    cwd: PathBuf,
}

impl OperationCatalog for TwoCatalog {
    fn plan_op(
        &self,
        operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        if operation != OperationId::AbortInProgress {
            return Err(PlanError::NotImplemented);
        }
        Ok(OpPlan {
            expected: json!({ "head": facts.head_commit }),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
            governed: None,
        })
    }

    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        Ok(Box::new(ResetIn {
            fx: Arc::clone(&self.0),
            cwd: plan.repo.worktree.clone(),
        }))
    }
}

impl ProtectedStep for ResetIn {
    fn subtype(&self) -> &str {
        "reset-hard"
    }

    fn scope(&self) -> StepScope {
        StepScope::default()
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        let mut cmd = self.fx.git_command(&self.cwd, &["reset", "-q", "--hard"]);
        cmd.stdout(std::process::Stdio::null());
        let child = ctx
            .spawn(&mut cmd)
            .map_err(|e| StepError::new(e.to_string()))?;
        let status = ctx.wait(child).map_err(|e| StepError::new(e.to_string()))?;
        if status.success() {
            Ok(StepOutput::default())
        } else {
            Err(StepError::new("git reset failed".to_owned()))
        }
    }
}

struct AllowEverything;

impl GuardrailsGate for AllowEverything {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::Allow
    }
    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}

/// `git reset --hard` as an unattributed protected operation in `worktree`; returns its prior
/// snapshot, the point to come back to.
fn reset_in(c: &mut gitraptor_core::client::Client, worktree: &Path) -> String {
    let plan: gitraptor_api::catalog::PrepareResult = c
        .call(
            methods::OPERATION_PREPARE,
            json!({ "operation": "abort-in-progress", "worktree": worktree.to_str().unwrap(), "surface": "tui" }),
        )
        .unwrap();
    let out: Value = c
        .call(
            methods::OPERATION_RUN,
            json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
        )
        .unwrap();
    serde_json::from_value::<gitraptor_api::timemachine::OperationRunResult>(out)
        .unwrap()
        .prior_snapshot_id
}

/// TMC023-SCOPE: a prefix is looked up only among the points of the requester's repo. The id of a
/// point of another observed repo is never restored and never listed through it.
#[test]
fn a_prefix_never_resolves_or_lists_ids_of_another_repo() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.write("a.rs", "fn a() {}\n");
    fx.git(&["add", "a.rs"]);
    fx.git(&["commit", "-q", "-m", "a"]);
    std::fs::write(fx.other_repo.join("b.rs"), "fn b() {}\n").unwrap();
    fx.git_in(&fx.other_repo, &["add", "b.rs"]);
    fx.git_in(&fx.other_repo, &["commit", "-q", "-m", "b"]);
    let (repo_a, repo_b) = (
        fx.repo.canonicalize().unwrap(),
        fx.other_repo.canonicalize().unwrap(),
    );
    let fx = Arc::new(fx);

    let tp = TempProfile::new();
    let mut profile = tp.open();
    let entry_a = profile.add_repo(&repo_a.join(".git"), None, 1).unwrap().0;
    let entry_b = profile.add_repo(&repo_b.join(".git"), None, 1).unwrap().0;
    drop(profile);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let daemon = Daemon::start(DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: Some(OperationsWiring {
            catalog: Arc::new(TwoCatalog(Arc::clone(&fx))),
            gate: Arc::new(AllowEverything),
            test_layer_override: Some(Layer::Cockpit),
            prior_deadline: Duration::from_secs(30),
            prior_layer: None,
        }),
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: false,
                ..CaptureConfig::default()
            },
            ..Default::default()
        },
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
    })
    .unwrap();
    let oplog_a: Arc<Mutex<Oplog>> = daemon.oplog(&entry_a.repo_id).unwrap();
    let oplog_b: Arc<Mutex<Oplog>> = daemon.oplog(&entry_b.repo_id).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let stop = || {
        handle.request(StopCause::Signal("TERM"));
    };

    let mut c = connect(&tp, gitraptor_api::messages::ClientKind::Cli);
    // A point in each repo, with work in each that the reset throws away.
    std::fs::write(repo_a.join("a.rs"), "fn a() { dirty_a(); }\n").unwrap();
    std::fs::write(repo_b.join("b.rs"), "fn b() { dirty_b(); }\n").unwrap();
    let point_a = reset_in(&mut c, &repo_a);
    let point_b = reset_in(&mut c, &repo_b);
    assert_ne!(point_a[..6], point_b[..6], "random ids collided: rerun");

    // From A, B's prefix names nothing: not found, nothing restored, nothing recorded anywhere.
    let before_b = std::fs::read_to_string(repo_b.join("b.rs")).unwrap();
    let before_a = std::fs::read_to_string(repo_a.join("a.rs")).unwrap();
    let (error, data) = us_tmc_013_common::rejected(c.call(
        methods::TM_RESTORE,
        restore_params(&repo_a, &point_b[..6], None),
    ));
    assert_eq!(error, code::NOT_FOUND, "{data:?}");
    assert!(
        !format!("{data:?}").contains(&point_b),
        "B's id leaked: {data:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repo_b.join("b.rs")).unwrap(),
        before_b
    );
    assert_eq!(
        std::fs::read_to_string(repo_a.join("a.rs")).unwrap(),
        before_a
    );
    for log in [&oplog_a, &oplog_b] {
        let restores = log
            .lock()
            .unwrap()
            .operations(&OperationFilter::default())
            .unwrap()
            .into_iter()
            .filter(|o| o.record.kind == OperationKind::Restore)
            .count();
        assert_eq!(restores, 0);
    }

    // Control: A's own prefix still resolves in A.
    let done = c.call::<_, Value>(
        methods::TM_RESTORE,
        restore_params(&repo_a, &point_a[..6], None),
    );
    assert!(done.is_ok(), "A's own prefix must restore: {done:?}");
    assert_eq!(
        std::fs::read_to_string(repo_a.join("a.rs")).unwrap(),
        "fn a() { dirty_a(); }\n"
    );
    stop();
    let _ = join.join();
}
