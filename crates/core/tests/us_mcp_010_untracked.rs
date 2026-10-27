//! US-MCP-010 (DS-US-MCP-010 T005): a new file enters a `safe_commit` only when it is named
//! (also in a new folder); "everything staged" never adds a file. A commit that does not land
//! (a hook rejects it, or the time limit interrupts it) leaves the index as it was: only what the
//! commit added is unstaged, never what was staged before; and when Git leaves an orphan
//! `index.lock` the unstage fails and the result is `failed-changed`, with the lock kept.
//!
//! Same rig as `us_mcp_010.rs`: the production `DaemonBackend` and commit arm over temporary
//! repos and profiles (NFR-01). A hook that waits is released by never creating its file: the
//! time limit (2 s here) ends it, not a sleep of the test.
#![cfg(unix)]
#![allow(dead_code)]

use gitraptor_core::timemachine::protected::{ProtectedError, StepFailure};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use gitraptor_api::catalog::{Layer, OperationId, PrepareParams, RejectReason, RunParams};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::{ProcError, ProcInfo, ProcSource};
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::ops::CommitEnv;
use gitraptor_core::executor::{
    Caller, ExecError, Executed, Executor, GuardrailsGate, PrepareInput, Prepared, RunDone, RunEnv,
    RunInput,
};
use gitraptor_core::guardrails::plan_gate::GuardrailsEngine;
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::observe::locate;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::manual::{ManualAsk, ManualCaptured, ManualError};
use gitraptor_core::timemachine::oplog::{Oplog, Requester, RequesterOrigin};
use gitraptor_core::timemachine::protected::backend::Presence;
use gitraptor_core::timemachine::protected::{
    DaemonBackend, McpRepos, OperationsWiring, ProtectedBackend, TmRepos,
};
use gitraptor_git::SystemGit;
use gitraptor_testkit::Fixture;
use serde_json::{Map, Value, json};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const SESSION: &str = "session-1";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn system_git() -> SystemGit {
    match gitraptor_git::resolve::resolve(
        &gitraptor_git::resolve::ResolveConfig::for_current_os(None),
        &gitraptor_git::Invoker::default(),
    ) {
        gitraptor_git::resolve::Resolution::Found { git, .. } => git,
        other => panic!("no Git: {other:?}"),
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

/// An executor over the production `DaemonBackend` with the production commit arm, the real
/// Guardrails engine and a real oplog, in a temporary profile (NFR-01).
struct Rig {
    f: Fixture,
    /// The worktree the commit runs in.
    wt: PathBuf,
    backend: DaemonBackend,
    executor: Executor,
    resolution: Resolution,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
}

/// Nobody else is in the worktree.
fn alone() -> Presence {
    Arc::new(|_: &Path| Some(Vec::new()))
}

impl Rig {
    /// A fixture with a commit, on a feature branch.
    fn new() -> Self {
        let f = Fixture::with_commit(&git());
        f.git(&["checkout", "-q", "-b", "feat/x"]);
        let wt = f.repo.clone();
        Self::build(f, wt, alone(), None)
    }

    fn build(f: Fixture, wt: PathBuf, presence: Presence, limit: Option<Duration>) -> Self {
        let dirs = ProfileDirs::under_root(&f.profile);
        let common = locate(&f.repo).unwrap();
        let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
        let tm = Arc::new(TmRepos::new(dirs.clone()));
        tm.insert(REPO, &common, oplog);
        let engine = Arc::new(GuardrailsEngine::default());
        let registry = Arc::new(GuardRegistry::for_profile(dirs));
        engine.attach(&registry, Arc::new(|_entry| true));
        let mut wiring = OperationsWiring::production_from(&|_| None).with_commit_env(CommitEnv {
            git: system_git(),
            home: Some(f.home.clone()),
        });
        wiring.gate = engine as Arc<dyn GuardrailsGate>;
        let mut executor = Executor::new(Arc::clone(&wiring.gate));
        if let Some(limit) = limit {
            executor = executor.with_limits(Duration::from_secs(600), limit);
        }
        let backend = DaemonBackend::new(tm, wiring, Arc::new(McpRepos::new([REPO.to_owned()])))
            .with_presence(presence);
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
            f,
            wt,
            backend,
            executor,
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

    fn caller() -> Caller {
        Caller {
            connection: 7,
            pid: 4_242,
            start_us: 1,
            mcp: true,
        }
    }

    /// `safe_commit` as the MCP asks it: `paths` `None` is "everything staged".
    fn try_prepare(&self, paths: Option<&[&str]>) -> Result<String, ExecError> {
        self.try_prepared(paths).map(|p| p.plan_id)
    }

    fn try_prepared(&self, paths: Option<&[&str]>) -> Result<Prepared, ExecError> {
        let mut args = Map::new();
        args.insert("message".into(), json!("feat: a change"));
        if let Some(paths) = paths {
            args.insert("paths".into(), Value::from(paths.to_vec()));
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
            .repo_of(&self.wt)
            .expect("the worktree is observed");
        self.executor.prepare(
            &self.backend,
            PrepareInput {
                caller: Self::caller(),
                resolution: &self.resolution,
                layer: Layer::Mcp,
                channel: RequestChannel::Mcp,
                params: &params,
                repo,
                confirm_refusal: None,
            },
        )
    }

    fn prepare(&self, paths: Option<&[&str]>) -> String {
        self.try_prepare(paths)
            .unwrap_or_else(|e| panic!("prepare of a commit plan over {paths:?}: {e:?}"))
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
            prior_deadline: Duration::from_secs(30),
            engine_mark: &engine_mark,
            engine: None,
            after_step: &after_step,
            publish: &publish,
            capture: &capture,
        };
        self.executor.run_any(
            &self.backend,
            RunInput {
                caller: Self::caller(),
                resolution: &self.resolution,
                params: &params,
                resolve_again: &again,
                rescope: &|_repo: &gitraptor_core::timemachine::protected::RepoHandle| Ok(()),
            },
            &env,
        )
    }

    /// Prepare and run; the commit it made.
    fn commit(&self, paths: Option<&[&str]>) -> Executed {
        let plan = self.prepare(paths);
        match self.run(&plan) {
            Ok(RunDone::Protected(done)) => done,
            other => panic!("commit over {paths:?} did not land: {other:?}"),
        }
    }

    fn g(&self, args: &[&str]) -> String {
        self.f.git_in(&self.wt, args)
    }

    fn commits(&self) -> usize {
        self.g(&["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }

    /// The index as Git has it.
    fn index(&self) -> String {
        self.g(&["ls-files", "-s"])
    }

    /// Prepare is refused with `reason` and nothing moved: no commit, same index.
    fn assert_refused(&self, paths: &[&str], reason: RejectReason) {
        let (commits, index) = (self.commits(), self.index());
        let got = self.try_prepare(Some(paths));
        assert_eq!(
            got,
            Err(ExecError::Rejected(reason)),
            "paths {paths:?} must be refused with {reason:?}"
        );
        assert_eq!(self.commits(), commits, "a refused plan made no commit");
        assert_eq!(
            self.index(),
            index,
            "a refused plan left the index as it was"
        );
    }

    /// The paths a commit touched, as `name-status` lines.
    fn changed_by_head(&self) -> String {
        self.g(&["show", "--name-status", "--format=", "HEAD"])
            .trim()
            .to_owned()
    }
}

/// A `pre-commit` hook in the repo's own hooks folder.
fn hook(f: &Fixture, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = f.repo.join(".git/hooks/pre-commit");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// How the commit failed: the failure kind of its protected step.
fn step_failure(got: Result<RunDone, ExecError>) -> StepFailure {
    match got {
        Err(ExecError::Protected(ProtectedError::Step { failure, .. })) => failure,
        other => panic!("expected a failed protected step, got {other:?}"),
    }
}

/// The hook blocks until a file that is never created; only the time limit ends it.
const WAIT_FOR_RELEASE: &str = "while [ ! -e \"$0.release\" ]; do sleep 0.1; done";

/// Gives `.git` back its write permission, whatever the test did.
struct Restore(PathBuf);

impl Drop for Restore {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn a_new_file_enters_only_when_named() {
    let rig = Rig::new();
    rig.f.write("new.txt", "new\n");
    rig.f.write("other.txt", "other\n");

    let done = rig.commit(Some(&["new.txt"]));

    assert_eq!(
        rig.changed_by_head(),
        "A\tnew.txt",
        "only the named file entered"
    );
    let new = done.outcome.output.new_commit.expect("the commit it made");
    assert_eq!(new.paths, vec!["new.txt".to_owned()]);
    assert!(
        rig.g(&["status", "--porcelain"]).contains("?? other.txt"),
        "the file that was not named stays untracked"
    );

    // "Everything staged" never adds a file that is not in the index.
    rig.f.write("a.txt", "alpha changed\n");
    rig.g(&["add", "a.txt"]);
    rig.f.write("late.txt", "late\n");
    rig.commit(None);
    assert_eq!(
        rig.changed_by_head(),
        "M\ta.txt",
        "all staged commits only what is staged"
    );
    assert!(
        rig.g(&["status", "--porcelain"]).contains("?? late.txt"),
        "all staged did not add the untracked file"
    );
}

#[test]
fn a_new_file_in_a_new_folder_is_committed_when_named() {
    let rig = Rig::new();
    rig.f.write("fresh/deep/n.txt", "new\n");
    rig.f.write("fresh/deep/m.txt", "not named\n");

    rig.commit(Some(&["fresh/deep/n.txt"]));

    assert_eq!(rig.changed_by_head(), "A\tfresh/deep/n.txt");
    assert!(
        rig.g(&["status", "--porcelain"])
            .contains("?? fresh/deep/m.txt"),
        "the sibling file that was not named stays untracked, listed one by one"
    );
}

#[test]
fn a_rejected_commit_leaves_the_index_as_it_was() {
    let rig = Rig::new();
    hook(&rig.f, "echo 'no' >&2\nexit 1");
    rig.f.write("new.txt", "new\n");
    let (commits, index) = (rig.commits(), rig.index());
    let plan = rig.prepare(Some(&["new.txt"]));

    let got = rig.run(&plan);

    assert_eq!(
        step_failure(got),
        StepFailure::Unchanged,
        "a hook rejection is failed-unchanged"
    );
    assert_eq!(rig.commits(), commits, "no commit");
    assert_eq!(
        rig.index(),
        index,
        "the file the commit added was unstaged: the index is as it was"
    );
    assert!(rig.wt.join("new.txt").exists(), "the file itself stays");
}

/// MCP010-G52 (⛔5.2): `rm --cached` is only for what the commit added; what was staged before
/// (the agent's or the developer's work) stays staged.
#[test]
fn a_rejected_commit_keeps_what_was_staged_before() {
    let rig = Rig::new();
    rig.f.write("staged.txt", "staged before\n");
    rig.g(&["add", "staged.txt"]);
    rig.f.write("a.txt", "alpha changed\n");
    rig.g(&["add", "a.txt"]);
    hook(&rig.f, "exit 1");
    rig.f.write("new.txt", "new\n");
    let index = rig.index();
    let plan = rig.prepare(Some(&["staged.txt", "a.txt", "new.txt"]));

    let got = rig.run(&plan);

    assert_eq!(step_failure(got), StepFailure::Unchanged);
    assert_eq!(
        rig.index(),
        index,
        "staged.txt and a.txt stay staged; only new.txt, added by the commit, was unstaged"
    );
    assert!(
        rig.g(&["diff", "--cached", "--name-only"])
            .contains("staged.txt"),
        "what was staged before is still staged"
    );
}

#[test]
fn a_timed_out_or_cancelled_commit_unstages_what_it_added() {
    // Layer `mcp` has no `operation.cancel`: its interruption is the time limit (DS-009 T006).
    let f = Fixture::with_commit(&git());
    f.git(&["checkout", "-q", "-b", "feat/x"]);
    hook(&f, WAIT_FOR_RELEASE);
    let wt = f.repo.clone();
    let rig = Rig::build(f, wt, alone(), Some(Duration::from_secs(2)));
    rig.f.write("new.txt", "new\n");
    let (commits, index) = (rig.commits(), rig.index());
    let plan = rig.prepare(Some(&["new.txt"]));

    let got = rig.run(&plan);

    assert_eq!(
        step_failure(got),
        StepFailure::TimeLimit,
        "an interrupted commit that did not land is time-limit"
    );
    assert_eq!(rig.commits(), commits, "no commit");
    assert_eq!(
        rig.index(),
        index,
        "the added file was unstaged: index as it was"
    );
    assert!(
        !rig.wt.join(".git/index.lock").exists(),
        "Git cleaned its own lock on the interrupt"
    );
}

/// MCP010-G53 (⛔5.3): the interrupted commit leaves an orphan `index.lock` (the hook takes the
/// write permission of `.git` away, so Git cannot remove its own lock when the interrupt kills
/// it): `rm --cached` fails and the result is `failed-changed`, never `failed-unchanged`; the
/// lock is not deleted.
#[test]
fn an_orphan_index_lock_after_a_time_limit_is_failed_changed() {
    let f = Fixture::with_commit(&git());
    f.git(&["checkout", "-q", "-b", "feat/x"]);
    let git_dir = f.repo.join(".git");
    let _restore = Restore(git_dir.clone());
    hook(
        &f,
        &format!("chmod 555 '{}'\n{WAIT_FOR_RELEASE}", git_dir.display()),
    );
    let wt = f.repo.clone();
    let rig = Rig::build(f, wt, alone(), Some(Duration::from_secs(2)));
    rig.f.write("new.txt", "new\n");
    let plan = rig.prepare(Some(&["new.txt"]));

    let got = rig.run(&plan);

    assert_eq!(
        step_failure(got),
        StepFailure::Changed,
        "an orphan index.lock makes the unstage fail: failed-changed"
    );
    assert!(
        git_dir.join("index.lock").exists(),
        "a Git lock is never deleted by the executor"
    );
}
