//! Typed failures of the `commit` step and the write rules of the MCP layer (DS-US-MCP-009
//! T006): the same rig as `us_mcp_009.rs` (real executor, real `DaemonBackend` with the
//! production catalog, real `GuardrailsEngine`, real oplog and store in a temporary profile, a
//! real repo in a temporary fixture; NFR-01). A slow hook waits for a release file and writes a
//! "started" marker; the tests wait for the marker with a deadline, never a fixed sleep.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use gitraptor_api::catalog::{
    Layer, MCP_TIME_LIMIT_MS, OperationArgs, OperationEventData, OperationId, OperationOutcome,
    PrepareParams, RejectReason, RunParams,
};
use gitraptor_api::mcp_view::{MCP_TIME_LIMIT, PRIOR_DEADLINE_BOUND};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::SystemProcs;
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::ops::CommitEnv;
use gitraptor_core::executor::{
    Caller, ExecError, Executor, GuardrailsGate, OpPlan, PlanError, PrepareInput, RepoFacts,
    RunDone, RunEnv, RunInput, StepPlan,
};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::repo_lock;
use gitraptor_core::timemachine::manual::{ManualAsk, ManualCaptured, ManualError};
use gitraptor_core::timemachine::oplog::{Oplog, Requester, RequesterOrigin};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    DEFAULT_PRIOR_DEADLINE, DaemonBackend, McpRepos, OperationsWiring, ProtectedBackend,
    ProtectedError, ProtectedStep, RepoHandle, ScopeError, StepError, StepFailure, TmRepos,
};
use gitraptor_git::SystemGit;
use gitraptor_git::resolve::ResolveConfig;
use gitraptor_testkit::Fixture;
use serde_json::{Map, json};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const SESSION: &str = "session-1";
/// How long a test waits for something that must happen.
const DEADLINE: Duration = Duration::from_secs(60);

fn system_git() -> SystemGit {
    match gitraptor_git::resolve::resolve(
        &ResolveConfig::for_current_os(None),
        &gitraptor_git::Invoker::default(),
    ) {
        gitraptor_git::resolve::Resolution::Found { git, .. } => git,
        other => panic!("no Git: {other:?}"),
    }
}

/// The production backend, unchanged.
struct Backend(DaemonBackend);

impl ProtectedBackend for Backend {
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
        self.0.repo_of(folder)
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        self.0.allowlist()
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        self.0.write_lock_key(repo)
    }
    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        self.0.facts(repo)
    }
    fn plan_op(
        &self,
        operation: OperationId,
        args: &OperationArgs,
        repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        self.0.plan_op(operation, args, repo, facts)
    }
    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        self.0.step(plan)
    }
}

struct Rig {
    fx: Fixture,
    _profile: tempfile::TempDir,
    /// Markers and release files of the hooks, outside the fixture.
    scratch: tempfile::TempDir,
    backend: Backend,
    executor: Executor,
    resolution: Resolution,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
    events: Mutex<Vec<OperationEventData>>,
}

impl Rig {
    /// `mcp_limit`: the time limit of layer `mcp`, shortened for the tests that hit it.
    fn new(mcp_limit: Duration) -> Self {
        let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
        fx.git(&["checkout", "-q", "-b", "feat/x"]);
        let profile = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(profile.path().join("profile"));
        let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
        let common_dir = fx.repo.join(".git").canonicalize().unwrap();
        let repos = Arc::new(TmRepos::new(dirs.clone()));
        repos.insert(REPO, &common_dir, oplog);
        let wiring = OperationsWiring::production_from(&|_| None).with_commit_env(CommitEnv {
            git: system_git(),
            home: Some(fx.home.clone()),
        });
        let gate: Arc<dyn GuardrailsGate> = Arc::clone(&wiring.gate);
        gate.attach(
            &Arc::new(GuardRegistry::for_profile(dirs)),
            Arc::new(|_entry| true),
        );
        let backend = Backend(DaemonBackend::new(
            repos,
            wiring,
            Arc::new(McpRepos::new([REPO.to_owned()])),
        ));
        let who = Who {
            actor: Actor::Agent {
                kind: actor::AgentKind::ClaudeCode,
                name: None,
                origin: actor::AgentOrigin::Detected,
            },
            requester: Requester::Agent {
                name: "claude".into(),
                origin: RequesterOrigin::Detected,
                session_id: SESSION.into(),
            },
        };
        Self {
            fx,
            _profile: profile,
            scratch: tempfile::tempdir().unwrap(),
            backend,
            executor: Executor::new(gate).with_limits(Duration::from_secs(300), mcp_limit),
            resolution: Resolution {
                who,
                via: ResolvedVia::Ancestry,
                executor_operation: None,
                confirmable: false,
            },
            marks: Arc::new(ExecutorMarks::default()),
            stopping: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
        }
    }

    fn caller(connection: u64) -> Caller {
        Caller {
            connection,
            pid: 4_242,
            start_us: 1,
            mcp: true,
        }
    }

    /// Prepares a `commit` of `paths` on `connection`; the plan id.
    fn prepare(&self, connection: u64, paths: &[&str]) -> String {
        let mut args = Map::new();
        args.insert(
            "message".into(),
            json!("fix: a\n\nCo-Authored-By: Claude <noreply@anthropic.com>"),
        );
        args.insert("paths".into(), json!(paths));
        let params = PrepareParams {
            operation: OperationId::Commit,
            worktree: None,
            args,
            surface: None,
            session_env: Vec::new(),
        };
        let repo = self
            .backend
            .repo_of(&self.fx.repo)
            .expect("the repo is observed");
        self.executor
            .prepare(
                &self.backend,
                PrepareInput {
                    caller: Self::caller(connection),
                    resolution: &self.resolution,
                    layer: Layer::Mcp,
                    channel: RequestChannel::Mcp,
                    params: &params,
                    repo,
                    confirm_refusal: None,
                },
            )
            .unwrap_or_else(|e| panic!("prepare of a commit plan: {e:?}"))
            .plan_id
    }

    fn run(&self, connection: u64, plan_id: &str) -> Result<RunDone, ExecError> {
        let params = RunParams {
            plan_id: plan_id.to_owned(),
            accepted_warnings: Vec::new(),
            confirmation: None,
        };
        let again = || Ok((self.resolution.clone(), Layer::Mcp, None));
        let capture = |ask: &ManualAsk| {
            Ok::<_, ManualError>(ManualCaptured {
                snapshot_id: "00000000-0000-4000-8000-000000000001".into(),
                worktree: ask.worktree.clone(),
            })
        };
        let engine_mark = |_repo: &str, _worktrees: &[PathBuf]| Some(1);
        let after_step = |_repo: &str, _worktrees: &[PathBuf], _operation: &str| {};
        let publish = |_kind: &str, data: OperationEventData| {
            self.events.lock().unwrap().push(data);
        };
        let env = RunEnv {
            marks: &self.marks,
            procs: &SystemProcs,
            stopping: &self.stopping,
            prior_deadline: Duration::from_secs(10),
            engine_mark: &engine_mark,
            engine: None,
            after_step: &after_step,
            publish: &publish,
            capture: &capture,
        };
        self.executor.run_any(
            &self.backend,
            RunInput {
                caller: Self::caller(connection),
                resolution: &self.resolution,
                params: &params,
                resolve_again: &again,
                rescope: &|_repo: &RepoHandle| Ok(()),
            },
            &env,
        )
    }

    fn head(&self) -> String {
        self.fx.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn commits(&self) -> usize {
        self.fx
            .git(&["rev-list", "--count", "--all"])
            .trim()
            .parse()
            .unwrap()
    }

    fn started(&self) -> PathBuf {
        self.scratch.path().join("started")
    }

    fn release(&self) -> PathBuf {
        self.scratch.path().join("release")
    }

    /// Installs hook `name`: it writes the "started" marker and waits for the release file (at
    /// most a minute, so a failed test leaves no process behind), then exits with `code`.
    fn slow_hook(&self, name: &str, code: i32) {
        self.hook(
            name,
            &format!(
                ": > '{}'\ni=0\nwhile [ ! -f '{}' ] && [ $i -lt 1200 ]; do sleep 0.05; i=$((i+1)); done\nexit {code}",
                self.started().display(),
                self.release().display(),
            ),
        );
    }

    fn hook(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.fx.repo.join(".git/hooks").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn wait_started(&self) {
        let start = Instant::now();
        while !self.started().exists() {
            assert!(start.elapsed() < DEADLINE, "the hook never started");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn open_release(&self) {
        std::fs::write(self.release(), "").unwrap();
    }

    fn outcomes(&self) -> Vec<Option<OperationOutcome>> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.outcome)
            .collect()
    }
}

// ----- Tests -------------------------------------------------------------------------------

/// A hook that rejects: the commit did not land, the branch is where it was, and the failure is
/// `failed-unchanged`, in the error and in the finished event.
#[test]
fn a_hook_that_rejects_is_failed_unchanged() {
    let rig = Rig::new(Duration::from_secs(300));
    rig.hook("pre-commit", "echo 'no'\nexit 1");
    rig.fx.write("a.txt", "alpha 2\n");
    let head = rig.head();
    let plan = rig.prepare(7, &["a.txt"]);

    let res = rig.run(7, &plan);

    match res {
        Err(ExecError::Protected(ProtectedError::Step { failure, .. })) => {
            assert_eq!(
                failure,
                StepFailure::Unchanged,
                "a rejected commit is unchanged"
            );
        }
        other => panic!("a hook that rejects must fail the step, got {other:?}"),
    }
    assert_eq!(rig.head(), head);
    assert!(
        rig.outcomes()
            .contains(&Some(OperationOutcome::FailedUnchanged)),
        "the finished event says failed-unchanged: {:?}",
        rig.outcomes()
    );
}

/// A hook over the time limit of layer `mcp`: `time-limit`, with the operation id, the branch
/// where it was.
#[test]
fn a_hook_over_the_time_limit_leaves_the_branch_and_names_the_operation() {
    let rig = Rig::new(Duration::from_secs(5));
    rig.slow_hook("pre-commit", 0);
    rig.fx.write("a.txt", "alpha 2\n");
    let head = rig.head();
    let plan = rig.prepare(7, &["a.txt"]);

    let res = rig.run(7, &plan);

    match res {
        Err(ExecError::Protected(ProtectedError::Step {
            operation_id,
            failure,
            ..
        })) => {
            assert_eq!(
                failure,
                StepFailure::TimeLimit,
                "the limit interrupted the hook"
            );
            assert!(!operation_id.is_empty(), "the answer names the operation");
        }
        other => panic!("a hook over the limit must fail with time-limit, got {other:?}"),
    }
    assert!(
        rig.started().exists(),
        "the hook was running when the limit fired"
    );
    assert_eq!(rig.head(), head, "the branch did not move");
    assert_eq!(rig.commits(), 1);
}

/// A `post-commit` hook over the limit: the commit had already entered, so the answer is a
/// success with its oid (the tip decides, not how `git` ended).
#[test]
fn a_time_limit_after_the_commit_landed_is_a_success_with_its_oid() {
    let rig = Rig::new(Duration::from_secs(5));
    rig.slow_hook("post-commit", 0);
    rig.fx.write("a.txt", "alpha 2\n");
    let plan = rig.prepare(7, &["a.txt"]);

    let res = rig.run(7, &plan);

    let done = match res {
        Ok(RunDone::Protected(done)) => done,
        other => panic!("a commit that landed must be a success, got {other:?}"),
    };
    let new = done
        .outcome
        .output
        .new_commit
        .expect("the success carries the commit read from the tip");
    assert_eq!(new.oid, rig.head());
    assert_eq!(new.paths, vec!["a.txt".to_owned()]);
    assert_eq!(rig.commits(), 2);
}

/// A request that waits for the repo's lock longer than the bound is `queue-full`, and nothing
/// was done: no commit, no operation started.
#[test]
fn a_queue_wait_over_its_bound_is_queue_full_without_effects() {
    let rig = Rig::new(Duration::from_secs(300));
    rig.fx.write("a.txt", "alpha 2\n");
    let head = rig.head();
    let plan = rig.prepare(7, &["a.txt"]);
    // Someone else holds the repo's write lock for the whole test.
    let repo = rig.backend.repo_of(&rig.fx.repo).unwrap();
    let key = rig.backend.write_lock_key(&repo);
    let _held = repo_lock::lock_queued(&key, 8, &|| false, |_| {}).expect("the lock is free");

    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(|| {
            let res = rig.run(7, &plan);
            tx.send(res).unwrap();
        });
        let res = rx
            .recv_timeout(DEADLINE + Duration::from_secs(30))
            .expect("the run gave up waiting for the lock");
        assert_eq!(
            res.unwrap_err(),
            ExecError::Rejected(RejectReason::QueueFull),
            "the wait over its bound is queue-full"
        );
    });
    assert_eq!(rig.head(), head, "no commit");
    assert!(
        rig.events
            .lock()
            .unwrap()
            .iter()
            .all(|e| e.operation_id.is_none()),
        "no operation was started"
    );
}

#[test]
fn the_call_limit_parts_match_the_daemon_constants() {
    assert_eq!(PRIOR_DEADLINE_BOUND, DEFAULT_PRIOR_DEADLINE);
    assert_eq!(MCP_TIME_LIMIT, Duration::from_millis(MCP_TIME_LIMIT_MS));
}

/// ⛔6.1: a second write of the same agent in the same repo is refused at once with
/// `write-in-progress`; it is never queued behind the first (which would commit twice when the
/// agent retries).
#[test]
fn a_second_write_of_the_same_agent_is_refused_without_queueing() {
    let rig = Rig::new(Duration::from_secs(300));
    rig.slow_hook("pre-commit", 0);
    rig.fx.write("a.txt", "alpha 2\n");
    rig.fx.write("b.txt", "beta 2\n");
    let first = rig.prepare(7, &["a.txt"]);
    let second = rig.prepare(8, &["b.txt"]);

    std::thread::scope(|s| {
        let running = s.spawn(|| rig.run(7, &first));
        rig.wait_started();

        // If the second run queued it would never come back while the hook waits.
        let (tx, rx) = mpsc::channel();
        let (rig, second) = (&rig, &second);
        s.spawn(move || tx.send(rig.run(8, second)).unwrap());
        let refused = rx.recv_timeout(Duration::from_secs(15));
        rig.open_release();
        let refused = refused.expect("the second write was queued instead of refused");
        assert_eq!(
            refused.unwrap_err(),
            ExecError::Rejected(RejectReason::WriteInProgress)
        );

        let done = running.join().unwrap();
        assert!(matches!(done, Ok(RunDone::Protected(_))), "{done:?}");
    });
    assert_eq!(rig.commits(), 2, "only the first commit was made");
    assert!(
        rig.fx.git(&["status", "--porcelain"]).contains(" M b.txt"),
        "b.txt was not committed"
    );
}

/// The MCP layer has no cancel and a closed connection only drops plans nobody took: a commit
/// that is running finishes and lands.
#[test]
fn a_dropped_connection_does_not_stop_a_running_commit() {
    let rig = Rig::new(Duration::from_secs(300));
    rig.slow_hook("pre-commit", 0);
    rig.fx.write("a.txt", "alpha 2\n");
    let plan = rig.prepare(7, &["a.txt"]);

    let done = std::thread::scope(|s| {
        let running = s.spawn(|| rig.run(7, &plan));
        rig.wait_started();
        rig.executor.drop_connection(7);
        rig.open_release();
        running.join().unwrap()
    });

    let done = match done {
        Ok(RunDone::Protected(done)) => done,
        other => panic!("the commit must finish after the connection dropped, got {other:?}"),
    };
    let new = done
        .outcome
        .output
        .new_commit
        .expect("the commit is reported");
    assert_eq!(new.oid, rig.head());
    assert_eq!(rig.commits(), 2);
}
