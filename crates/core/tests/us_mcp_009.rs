//! The production `commit` arm (DS-US-MCP-009 T003): the real executor over a real
//! `DaemonBackend` (the production catalog with its `CommitEnv`), the real `GuardrailsEngine`, a
//! real oplog and snapshot store in a temporary profile, and a real repo in a temporary fixture
//! (NFR-01). Nothing here waits: a commit by `git` runs to its end in the calling thread.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::catalog::{
    Layer, OperationArgs, OperationId, OperationOutcome, PrepareParams, RejectReason, RunParams,
};
use gitraptor_api::timemachine::{PriorFailure, RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::{ProcError, ProcInfo, ProcSource};
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::ops::CommitEnv;
use gitraptor_core::executor::{
    Affected, Caller, ExecError, Executed, Executor, GuardrailsGate, OpPlan, PlanError,
    PrepareInput, RepoFacts, RunDone, RunEnv, RunInput, StepPlan,
};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::manual::{ManualAsk, ManualCaptured, ManualError};
use gitraptor_core::timemachine::oplog::{
    Channel, OperationState, Oplog, Requester, RequesterOrigin,
};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    DaemonBackend, McpRepos, OperationsWiring, PriorError, PriorRequest, PriorSnapshot,
    PriorSnapshotter, ProtectedBackend, ProtectedError, ProtectedStep, RepoHandle, ScopeError,
    StepError, StepFailure, TmRepos,
};
use gitraptor_git::SystemGit;
use gitraptor_git::resolve::ResolveConfig;
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Map, json};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const SESSION: &str = "session-1";
const CONNECTION: u64 = 7;

fn system_git() -> SystemGit {
    match gitraptor_git::resolve::resolve(
        &ResolveConfig::for_current_os(None),
        &gitraptor_git::Invoker::default(),
    ) {
        gitraptor_git::resolve::Resolution::Found { git, .. } => git,
        other => panic!("no Git: {other:?}"),
    }
}

/// A prior that fails like a full disk.
struct NoSpace;

impl PriorSnapshotter for NoSpace {
    fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        Err(PriorError::NoSpace)
    }
}

/// The production backend, remembering the `affected` the executor put in each plan it ran.
struct Spy {
    inner: DaemonBackend,
    affected: Mutex<Vec<Affected>>,
}

impl ProtectedBackend for Spy {
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
        self.inner.repo_of(folder)
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        self.inner.allowlist()
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        self.inner.write_lock_key(repo)
    }
    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        self.inner.facts(repo)
    }
    fn plan_op(
        &self,
        operation: OperationId,
        args: &OperationArgs,
        repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        self.inner.plan_op(operation, args, repo, facts)
    }
    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        self.affected
            .lock()
            .unwrap()
            .push(plan.op_plan.affected.clone());
        self.inner.step(plan)
    }
}

struct NoProcs;

impl ProcSource for NoProcs {
    fn read(&self, _pid: u32) -> Result<ProcInfo, ProcError> {
        Err(ProcError::Gone)
    }
    fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
        None
    }
}

struct Opts {
    /// Wire `CommitEnv` (the daemon does when it found a Git).
    git: bool,
    /// Every prior fails with a full disk.
    no_space: bool,
}

struct Rig {
    fx: Fixture,
    _profile: tempfile::TempDir,
    oplog: Arc<Mutex<Oplog>>,
    backend: Spy,
    executor: Executor,
    resolution: Resolution,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
}

impl Rig {
    fn new(fx: Fixture, opts: Opts) -> Self {
        let profile = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(profile.path().join("profile"));
        let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
        let common_dir = fx.repo.join(".git").canonicalize().unwrap();
        let repos = Arc::new(TmRepos::new(dirs.clone()));
        repos.insert(REPO, &common_dir, oplog);
        let oplog = repos.oplog(REPO).unwrap();

        let mut wiring = OperationsWiring::production_from(&|_| None);
        if opts.git {
            wiring = wiring.with_commit_env(CommitEnv {
                git: system_git(),
                home: Some(fx.home.clone()),
            });
        }
        if opts.no_space {
            wiring.prior_layer = Some(Arc::new(|_: Arc<dyn PriorSnapshotter>| {
                Arc::new(NoSpace) as Arc<dyn PriorSnapshotter>
            }));
        }
        // The real engine, attached to a registry of the test, as the daemon attaches it.
        let gate: Arc<dyn GuardrailsGate> = Arc::clone(&wiring.gate);
        gate.attach(
            &Arc::new(GuardRegistry::for_profile(dirs)),
            Arc::new(|_entry| true),
        );
        let backend = Spy {
            inner: DaemonBackend::new(repos, wiring, Arc::new(McpRepos::new([REPO.to_owned()]))),
            affected: Mutex::new(Vec::new()),
        };
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
            oplog,
            backend,
            executor: Executor::new(gate),
            resolution: Resolution {
                who,
                via: ResolvedVia::Ancestry,
                executor_operation: None,
                confirmable: false,
            },
            marks: Arc::new(ExecutorMarks::default()),
            stopping: AtomicBool::new(false),
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

    /// Prepares a `commit` over MCP; the plan id.
    fn prepare(&self, message: &str, paths: Option<&[&str]>) -> Result<String, ExecError> {
        let mut args = Map::new();
        args.insert("message".into(), json!(message));
        if let Some(paths) = paths {
            args.insert("paths".into(), json!(paths));
        }
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
                    caller: Self::caller(CONNECTION),
                    resolution: &self.resolution,
                    layer: Layer::Mcp,
                    channel: RequestChannel::Mcp,
                    params: &params,
                    repo,
                    confirm_refusal: None,
                },
            )
            .map(|p| p.plan_id)
    }

    fn run(&self, plan_id: &str) -> Result<RunDone, ExecError> {
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
        let publish = |_kind: &str, _data| {};
        let env = RunEnv {
            marks: &self.marks,
            procs: &NoProcs,
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
                caller: Self::caller(CONNECTION),
                resolution: &self.resolution,
                params: &params,
                resolve_again: &again,
                rescope: &|_repo: &RepoHandle| Ok(()),
            },
            &env,
        )
    }

    /// Prepares and runs; the executed operation.
    fn commit(&self, message: &str, paths: Option<&[&str]>) -> Executed {
        let plan = self
            .prepare(message, paths)
            .unwrap_or_else(|e| panic!("prepare of a commit plan: {e:?}"));
        match self.run(&plan) {
            Ok(RunDone::Protected(done)) => done,
            other => panic!("the commit did not run as a protected operation: {other:?}"),
        }
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

    fn status(&self) -> String {
        self.fx.git(&["status", "--porcelain"])
    }

    fn oplog_is_untouched(&self) -> bool {
        self.oplog.lock().unwrap().last_seq().unwrap() == 0
    }

    fn hook(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.fx.repo.join(".git/hooks").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn rig() -> Rig {
    Rig::new(
        Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path()),
        Opts {
            git: true,
            no_space: false,
        },
    )
}

// ----- Tests -------------------------------------------------------------------------------

/// The named path is committed with `--only` after a prior, the answer carries the commit read
/// from the branch tip, and what else was changed stays changed.
#[test]
fn a_named_path_is_committed_after_a_prior_and_reports_the_new_commit() {
    let rig = rig();
    let before = rig.head();
    rig.fx.write("a.txt", "alpha 2\n");
    rig.fx.write("b.txt", "beta 2\n");

    let done = rig.commit("fix: a", Some(&["a.txt"]));

    let new = done
        .outcome
        .output
        .new_commit
        .expect("the protected step reports the commit it made");
    assert_eq!(new.oid, rig.head(), "the oid is the tip of the branch");
    assert_ne!(new.oid, before, "a new commit was made");
    assert_eq!(new.paths, vec!["a.txt".to_owned()]);
    assert_eq!(new.total, 1);
    assert_eq!(done.result, OperationOutcome::Done);
    assert_eq!(rig.commits(), 2, "exactly one commit was added");
    assert_eq!(rig.fx.git(&["log", "-1", "--format=%s"]).trim(), "fix: a");
    assert_eq!(rig.status().trim_end(), " M b.txt", "b.txt stays unstaged");
    let op = rig
        .oplog
        .lock()
        .unwrap()
        .operation(&done.outcome.operation_id)
        .unwrap()
        .expect("the operation is in the oplog");
    assert_eq!(op.state, OperationState::Finished);
    assert!(op.prior_snapshot.is_some(), "the prior was taken first");
}

/// Without paths the commit is the index as it is.
#[test]
fn a_staged_commit_commits_only_what_is_staged() {
    let rig = rig();
    rig.fx.write("a.txt", "alpha 2\n");
    rig.fx.git(&["add", "a.txt"]);
    rig.fx.write("b.txt", "beta 2\n");

    let done = rig.commit("fix: staged", None);

    let new = done
        .outcome
        .output
        .new_commit
        .expect("the commit is reported");
    assert_eq!(new.paths, vec!["a.txt".to_owned()]);
    assert_eq!(new.total, 1);
    assert_eq!(rig.commits(), 2);
    assert_eq!(
        rig.status().trim_end(),
        " M b.txt",
        "the unstaged change stays"
    );
}

/// A named path with no change is not in the commit: the paths come from the commit.
#[test]
fn the_new_commit_paths_come_from_the_commit_not_the_request() {
    let rig = rig();
    rig.fx.write("a.txt", "alpha 2\n");

    let done = rig.commit("fix: a", Some(&["a.txt", "b.txt"]));

    let new = done
        .outcome
        .output
        .new_commit
        .expect("the commit is reported");
    assert_eq!(
        new.paths,
        vec!["a.txt".to_owned()],
        "b.txt was named but is not in the commit"
    );
    assert_eq!(new.total, 1);
}

/// The first commit of an unborn branch (no parent, no old oid) reports its paths.
#[test]
fn a_first_commit_on_an_unborn_branch_reports_its_paths() {
    let fx = Fixture::new(&gitraptor_testkit::fixture::git_from_path());
    fx.write("first.txt", "one\n");
    fx.git(&["add", "first.txt"]);
    let rig = Rig::new(
        fx,
        Opts {
            git: true,
            no_space: false,
        },
    );

    let done = rig.commit("feat: first", None);

    let new = done
        .outcome
        .output
        .new_commit
        .expect("an unborn branch reports its first commit");
    assert_eq!(new.oid, rig.head());
    assert_eq!(new.paths, vec!["first.txt".to_owned()]);
    assert_eq!(new.total, 1);
    assert_eq!(rig.commits(), 1);
    assert!(
        rig.fx
            .git(&["rev-list", "--parents", "-n1", "HEAD"])
            .trim()
            .split(' ')
            .count()
            == 1,
        "the first commit has no parent"
    );
}

#[test]
fn a_detached_head_is_refused_before_any_effect() {
    let rig = rig();
    rig.fx.git(&["checkout", "-q", "--detach"]);
    rig.fx.write("a.txt", "alpha 2\n");
    rig.fx.git(&["add", "a.txt"]);
    let (head, before) = (rig.head(), rig.fx.fingerprint());

    let err = rig.prepare("fix: a", None).unwrap_err();

    assert_eq!(
        err,
        ExecError::Rejected(RejectReason::DetachedHead),
        "{err:?}"
    );
    assert_eq!(rig.head(), head);
    assert!(rig.oplog_is_untouched(), "no intent, no prior");
    let changes = diff(&before, &rig.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

#[test]
fn a_rebase_in_progress_is_refused_before_any_effect() {
    let rig = rig();
    rig.fx.git(&["checkout", "-q", "-b", "feat"]);
    rig.fx.write("a.txt", "feat\n");
    rig.fx.git(&["commit", "-q", "-am", "feat"]);
    rig.fx.git(&["checkout", "-q", "main"]);
    rig.fx.write("a.txt", "main\n");
    rig.fx.git(&["commit", "-q", "-am", "main"]);
    rig.fx.git(&["checkout", "-q", "feat"]);
    let stopped = rig
        .fx
        .git_command(&rig.fx.repo, &["rebase", "main"])
        .output()
        .unwrap();
    assert!(
        !stopped.status.success(),
        "the fixture rebase must stop on a conflict"
    );
    let before = rig.fx.fingerprint();

    let err = rig.prepare("fix: a", None).unwrap_err();

    assert_eq!(
        err,
        ExecError::Rejected(RejectReason::OperationInProgress),
        "{err:?}"
    );
    assert!(rig.oplog_is_untouched(), "no intent, no prior");
    let changes = diff(&before, &rig.fx.fingerprint());
    assert!(changes.is_empty(), "{changes:#?}");
}

/// No system Git for user operations: refused in the plan, before the intent and the prior.
#[test]
fn without_git_commit_is_refused_before_the_intent() {
    let rig = Rig::new(
        Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path()),
        Opts {
            git: false,
            no_space: false,
        },
    );
    rig.fx.write("a.txt", "alpha 2\n");

    let err = rig.prepare("fix: a", Some(&["a.txt"])).unwrap_err();

    assert_eq!(
        err,
        ExecError::Rejected(RejectReason::GitUnavailable),
        "{err:?}"
    );
    assert!(rig.oplog_is_untouched(), "no intent, no prior");
}

/// The operation is in the timeline with the agent and the channel `mcp`.
#[test]
fn the_commit_is_recorded_with_the_agent_and_channel_mcp() {
    let rig = rig();
    rig.fx.write("a.txt", "alpha 2\n");

    let done = rig.commit("fix: a", Some(&["a.txt"]));

    let op = rig
        .oplog
        .lock()
        .unwrap()
        .operation(&done.outcome.operation_id)
        .unwrap()
        .expect("the operation is in the oplog");
    assert_eq!(op.record.channel, Channel::Mcp);
    assert_eq!(op.record.requester.session_id(), Some(SESSION));
    assert_eq!(op.state, OperationState::Finished);
}

/// ⛔3.2: the plan the step runs names the requester's own work, never "nobody".
#[test]
fn a_commit_affects_only_the_requesters_own_work() {
    let rig = rig();
    rig.fx.write("a.txt", "alpha 2\n");

    rig.commit("fix: a", Some(&["a.txt"]));

    assert_eq!(
        *rig.backend.affected.lock().unwrap(),
        vec![Affected::Agent {
            session_id: SESSION.into()
        }],
        "the executor fixes the affected work to the requester's session"
    );
}

/// ⛔3.3: a `.git` rewritten after `prepare` to point at another repo commits nothing, neither
/// in the one the gate opened nor in the one the file points to.
#[test]
fn a_dot_git_rewritten_after_prepare_commits_nothing_in_either_repo() {
    let rig = rig();
    let other = rig.fx.other_repo.clone();
    rig.fx
        .git_in(&other, &["commit", "-q", "--allow-empty", "-m", "other"]);
    std::fs::write(other.join("x.txt"), "x\n").unwrap();
    rig.fx.git_in(&other, &["add", "x.txt"]);
    rig.fx.write("a.txt", "alpha 2\n");
    rig.fx.git(&["add", "a.txt"]);
    let plan = rig
        .prepare("fix: a", None)
        .unwrap_or_else(|e| panic!("prepare of a commit plan: {e:?}"));

    // The agent swaps the worktree's `.git` for a file that points at the other repo.
    let real = rig.fx.root.join("real-git");
    std::fs::rename(rig.fx.repo.join(".git"), &real).unwrap();
    std::fs::write(
        rig.fx.repo.join(".git"),
        format!("gitdir: {}\n", other.join(".git").display()),
    )
    .unwrap();

    let res = rig.run(&plan);

    assert!(
        matches!(
            res,
            Err(ExecError::Rejected(
                RejectReason::RepoIdentityChanged | RejectReason::StateChanged
            ))
        ),
        "a rewritten .git must refuse the run: {res:?}"
    );
    let count = |git_dir: &Path| -> String {
        rig.fx
            .git_in(
                &rig.fx.root,
                &[
                    "--git-dir",
                    git_dir.to_str().unwrap(),
                    "rev-list",
                    "--count",
                    "--all",
                ],
            )
            .trim()
            .to_owned()
    };
    assert_eq!(count(&real), "1", "no commit in the repo the gate opened");
    assert_eq!(
        count(&other.join(".git")),
        "1",
        "no commit in the repo the file points to"
    );
}

/// ⛔3.4: a hook that prints what a successful commit prints, and fails, does not make a commit
/// land: the branch tip decides, not Git's output.
#[test]
fn a_hook_that_prints_a_success_line_does_not_make_a_commit_land() {
    let rig = rig();
    rig.hook(
        "pre-commit",
        "echo '[main 1234567] fix: a'\necho ' 1 file changed, 1 insertion(+)'\nexit 1",
    );
    rig.fx.write("a.txt", "alpha 2\n");
    let head = rig.head();
    let plan = rig
        .prepare("fix: a", Some(&["a.txt"]))
        .unwrap_or_else(|e| panic!("prepare of a commit plan: {e:?}"));

    let res = rig.run(&plan);

    match res {
        Err(ExecError::Protected(ProtectedError::Step { failure, .. })) => assert_eq!(
            failure,
            StepFailure::Unchanged,
            "the tip did not move: the commit did not land"
        ),
        other => panic!("a hook that rejects must fail the step, got {other:?}"),
    }
    assert_eq!(rig.head(), head, "the branch did not move");
    assert_eq!(rig.commits(), 1);
}

/// Scenario 5: a prior that cannot be taken (full disk) leaves the branch, the index and the
/// working tree as they were, and the answer names the aborted operation.
#[test]
fn a_prior_that_cannot_be_taken_leaves_the_branch_and_names_the_operation() {
    let rig = Rig::new(
        Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path()),
        Opts {
            git: true,
            no_space: true,
        },
    );
    rig.fx.write("a.txt", "alpha 2\n");
    let plan = rig
        .prepare("fix: a", Some(&["a.txt"]))
        .unwrap_or_else(|e| panic!("prepare of a commit plan: {e:?}"));
    let before = rig.fx.fingerprint();

    let res = rig.run(&plan);

    let id = match res {
        Err(ExecError::Protected(ProtectedError::Prior {
            reason: PriorFailure::NoSpace,
            operation_id: Some(id),
            ..
        })) => id,
        other => panic!("a full disk must fail the prior with its operation id, got {other:?}"),
    };
    let changes = diff(&before, &rig.fx.fingerprint());
    assert!(
        changes.is_empty(),
        "no commit, nothing changed: {changes:#?}"
    );
    let op = rig
        .oplog
        .lock()
        .unwrap()
        .operation(&id)
        .unwrap()
        .expect("the aborted operation");
    assert_eq!(op.state, OperationState::Aborted);
}
