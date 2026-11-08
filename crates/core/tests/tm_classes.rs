//! TS-GRP-005 T010 (CLS1, D11, D18): the classes of the threads of the prior-snapshot path and
//! of the continuous capture, read back from the OS (`work_class::current()`).
//!
//! Seams that exist today, no new production code:
//! - the `abort` closure of `SnapshotStore::capture_hook_prior_until`, which every hashing
//!   thread of a prior calls from `should_yield`, and the capturing thread itself;
//! - `work_class::pools()` for the continuous capture's pool;
//! - `OperationCatalog::plan_op`, which `operation.prepare` runs on `raptor-conn`;
//! - `TmPriorLayer`, a wrapper of the `PriorSnapshotter` that `raptor-prior` calls.
//!
//! `raptor-tm-seed` (continuous.rs:311) has no hook that runs on it: its class cannot be read
//! without a new production seam (reported, not tested). Temporary repos and profiles (NFR-01).
#![cfg(target_os = "macos")]

mod common;
mod tm_common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, PrepareResult};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::resources::{Pool, PoolClass};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TmCapture,
    TmPriorLayer,
};
use gitraptor_core::executor::{
    Affected, GateDecision, GateRequest, GuardrailsGate, OpPlan, PlanClose, PlanError, RepoFacts,
    StepPlan,
};
use gitraptor_core::timemachine::oplog::{
    Channel, HookPriorMeta, ManualMeta, Requester, RequesterOrigin, SnapshotLevel,
};
use gitraptor_core::timemachine::protected::{
    OperationCatalog, OperationsWiring, PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter,
    ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput, StepScope,
};
use gitraptor_core::work_class::{self, WorkClass};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

use tm_common::{Env, write};

type Seen = Arc<Mutex<Vec<(ThreadId, Option<WorkClass>)>>>;

fn busy_env_with_many_files() -> Env {
    let env = Env::busy();
    for i in 0..24 {
        write(
            &env.f.repo,
            &format!("many/f{i:02}.txt"),
            format!("content {i}\n").as_bytes(),
        );
    }
    env
}

/// What every call of the capture's `abort` closure saw: (thread, class read from the OS).
fn record(seen: &Seen) -> impl Fn() -> bool + Sync + use<> {
    let seen = Arc::clone(seen);
    move || {
        seen.lock()
            .unwrap()
            .push((std::thread::current().id(), work_class::current()));
        false
    }
}

fn hook_prior(env: &Env, abort: &(dyn Fn() -> bool + Sync)) {
    let meta = HookPriorMeta {
        requester: Requester::Unattributed,
        worktree_key: "main".into(),
        requested_ms: 1,
    };
    let req = env.request(SnapshotLevel::HookPrior, None);
    env.store
        .capture_hook_prior_until(&env.oplog, &req, &meta, abort)
        .expect("the hook prior must be taken");
}

fn wait_until(what: &str, limit: Duration, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn prior_hashing_runs_in_user_initiated_and_capture_stays_in_default() {
    let env = busy_env_with_many_files();
    let seen: Seen = Arc::default();
    let abort = record(&seen);
    hook_prior(&env, &abort);

    let me = std::thread::current().id();
    let seen = seen.lock().unwrap();
    let hashing: Vec<_> = seen.iter().filter(|(t, _)| *t != me).collect();
    assert!(
        !hashing.is_empty(),
        "a prior with many files must hash on its own threads (std::thread::scope)"
    );
    for (_, class) in &hashing {
        assert_eq!(
            *class,
            Some(WorkClass::UserInitiated),
            "the hashing threads of a prior must read UserInitiated from the OS (D18, capture.rs:1168)"
        );
    }
}

#[test]
fn continuous_capture_shows_as_pool_capture_in_default_while_it_runs() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let r = start_daemon(fx, None);
    wait_until(
        "the continuous capture pool",
        Duration::from_secs(15),
        || {
            work_class::pools().contains(&PoolClass {
                pool: Pool::Capture,
                class: WorkClass::Default,
            })
        },
    );
    drop(r);
}

#[test]
fn the_writer_is_never_held_below_default() {
    // A non-prior capture hashes in line on its own thread, with the writer held; the prior's
    // hashing threads hold it too. None of them may read Utility or Background (D11, D18).
    let env = busy_env_with_many_files();
    let seen: Seen = Arc::default();
    let abort = record(&seen);

    let manual = ManualMeta {
        label: "t".into(),
        requester: Requester::Agent {
            name: "claude-code".into(),
            origin: RequesterOrigin::Detected,
            session_id: "s1".into(),
        },
        channel: Channel::Cli,
        worktree_key: "main".into(),
        requested_ms: 1,
    };
    let req = env.request(SnapshotLevel::Manual, None);
    // On its own thread in Default, as the continuous capture runs.
    let result = std::thread::scope(|s| {
        s.spawn(|| {
            work_class::set_current(WorkClass::Default);
            env.store
                .capture_manual_until(&env.oplog, &req, &manual, &abort)
        })
        .join()
        .unwrap()
    });
    result.expect("the manual capture must succeed");
    hook_prior(&env, &abort);

    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty(), "the capture must have called abort");
    for (_, class) in seen.iter() {
        assert!(
            matches!(
                class,
                Some(WorkClass::Default) | Some(WorkClass::UserInitiated)
            ),
            "a section that holds the store writer must run in Default or above, saw {class:?}"
        );
    }
}

// ----- raptor-conn and raptor-prior: a daemon with a catalog that reads its thread's class ------

/// (thread name, class from the OS) of each place the test hooks.
type Names = Arc<Mutex<Vec<(String, Option<WorkClass>)>>>;

fn here(into: &Names) {
    let name = std::thread::current().name().unwrap_or("").to_owned();
    into.lock().unwrap().push((name, work_class::current()));
}

struct AllowAll;

impl GuardrailsGate for AllowAll {
    fn evaluate(&self, _req: &GateRequest) -> GateDecision {
        GateDecision::Allow
    }
    fn record_close(&self, _req: &GateRequest, _close: PlanClose) {}
}

struct Catalog {
    conn: Names,
}

struct Step;

impl OperationCatalog for Catalog {
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
        // `operation.prepare` is served on the connection's thread.
        here(&self.conn);
        Ok(OpPlan {
            expected: json!({ "head": facts.head_commit }),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
            governed: None,
        })
    }

    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        Ok(Box::new(Step))
    }
}

impl ProtectedStep for Step {
    fn subtype(&self) -> &str {
        "noop"
    }
    fn scope(&self) -> StepScope {
        StepScope::default()
    }
    fn run(&mut self, _ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        Ok(StepOutput::default())
    }
}

/// Reads the class of the thread that asks for a prior, then delegates.
struct Spy {
    inner: Arc<dyn PriorSnapshotter>,
    prior: Names,
}

impl PriorSnapshotter for Spy {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        here(&self.prior);
        self.inner.prior(req)
    }
}

struct Running {
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    tp: TempProfile,
    _fx: Fixture,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn start_daemon(fx: Fixture, spy: Option<(Names, Names)>) -> Running {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    profile
        .add_repo(&fx.repo.join(".git").canonicalize().unwrap(), None, 1)
        .unwrap();
    drop(profile);
    let (operations, tm_prior_layer) = match spy {
        Some((conn, prior)) => (
            Some(OperationsWiring {
                catalog: Arc::new(Catalog { conn }) as Arc<dyn OperationCatalog>,
                gate: Arc::new(AllowAll),
                test_layer_override: Some(Layer::Cockpit),
                prior_deadline: Duration::from_secs(30),
                prior_layer: Some(Arc::new(move |inner: Arc<dyn PriorSnapshotter>| {
                    Arc::new(Spy {
                        inner,
                        prior: Arc::clone(&prior),
                    }) as Arc<dyn PriorSnapshotter>
                })),
            }),
            None::<TmPriorLayer>,
        ),
        None => (None, None),
    };
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations,
        tm_prior_layer,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
        tm_capture: TmCapture::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        handle,
        join: Some(join),
        tp,
        _fx: fx,
    }
}

fn connect(tp: &TempProfile) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

#[test]
fn conn_and_prior_threads_run_in_user_initiated_and_seed_in_background() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.git(&["branch", "feat-login"]);
    let wt: PathBuf = fx
        .add_worktree("feat-login", "feat-login")
        .canonicalize()
        .unwrap();
    let conn: Names = Arc::default();
    let prior: Names = Arc::default();
    let r = start_daemon(fx, Some((Arc::clone(&conn), Arc::clone(&prior))));

    let mut c = connect(&r.tp);
    let plan: PrepareResult = c
        .call(
            methods::OPERATION_PREPARE,
            json!({
                "operation": "abort-in-progress",
                "worktree": wt.to_str().unwrap(),
                "surface": "tui",
            }),
        )
        .unwrap();
    let _: Value = c
        .call(
            methods::OPERATION_RUN,
            json!({ "plan_id": plan.plan_id, "accepted_warnings": plan.warnings }),
        )
        .unwrap();

    let conn = conn.lock().unwrap();
    assert!(
        conn.iter().any(|(n, _)| n == "raptor-conn"),
        "operation.prepare must be served on raptor-conn, saw {conn:?}"
    );
    for (name, class) in conn.iter().filter(|(n, _)| n == "raptor-conn") {
        assert_eq!(
            *class,
            Some(WorkClass::UserInitiated),
            "{name} must read UserInitiated from the OS (D18)"
        );
    }
    let prior = prior.lock().unwrap();
    assert!(
        prior.iter().any(|(n, _)| n == "raptor-prior"),
        "the prior of operation.run must be taken on raptor-prior, saw {prior:?}"
    );
    for (name, class) in prior.iter().filter(|(n, _)| n == "raptor-prior") {
        assert_eq!(
            *class,
            Some(WorkClass::UserInitiated),
            "{name} must read UserInitiated from the OS (D18)"
        );
    }
}
