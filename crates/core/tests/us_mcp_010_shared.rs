//! US-MCP-010 (DS-US-MCP-010 T006): with another live agent session in the worktree, "everything
//! staged" is refused (`shared-worktree-needs-paths`, whose action the MCP shows as "Indica tus
//! rutas: el worktree es compartido."); with named paths the commit goes on and commits only
//! them; the developer's session, the requester's own and a crashed agent's do not count; an
//! unreadable presence counts as shared; and a commit never carries the `active-session`
//! warning (ADR-MCP-001 § 4.3).
//!
//! The presence is injected with `DaemonBackend::with_presence` (DS-010 § Tipos): the filter
//! that drops the developer, an ended session or a dead process lives in the production
//! presence, which only a running daemon builds, so the e2e `mcp_safe_commit_paths` covers it.
//! Same rig as `us_mcp_010.rs` over temporary repos and profiles (NFR-01).
#![cfg(unix)]
#![allow(dead_code)]

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

/// Another agent's session id is in the worktree.
fn other_agent() -> Presence {
    Arc::new(|_: &Path| Some(vec!["session-2".to_owned()]))
}

fn rig_with(presence: Presence) -> Rig {
    let f = Fixture::with_commit(&git());
    f.git(&["checkout", "-q", "-b", "feat/x"]);
    let wt = f.repo.clone();
    Rig::build(f, wt, presence, None)
}

/// Stages a change of `a.txt` and leaves one of `b.txt` unstaged.
fn stage_a_leave_b(rig: &Rig) {
    rig.f.write("a.txt", "alpha changed\n");
    rig.g(&["add", "a.txt"]);
    rig.f.write("b.txt", "beta changed\n");
}

#[test]
fn all_staged_in_a_shared_worktree_is_refused_with_the_action() {
    let rig = rig_with(other_agent());
    stage_a_leave_b(&rig);
    let (commits, index) = (rig.commits(), rig.index());

    let got = rig.try_prepare(None);

    assert_eq!(
        got,
        Err(ExecError::Rejected(RejectReason::SharedWorktreeNeedsPaths)),
        "all staged with another live agent session must ask for named paths"
    );
    assert_eq!(rig.commits(), commits, "no commit");
    assert_eq!(
        rig.index(),
        index,
        "the other agent's staged work is untouched"
    );
}

#[test]
fn named_paths_in_a_shared_worktree_commit_only_them() {
    let rig = rig_with(other_agent());
    stage_a_leave_b(&rig);
    let before = rig.commits();

    rig.commit(Some(&["b.txt"]));

    assert_eq!(rig.commits(), before + 1, "one commit");
    assert_eq!(rig.changed_by_head(), "M\tb.txt", "only the named path");
    assert!(
        rig.g(&["diff", "--cached", "--name-only"])
            .contains("a.txt"),
        "what the other session had staged stays staged"
    );
}

#[test]
fn the_developers_session_does_not_make_it_shared() {
    // The production presence never lists the developer's or the TUI's session; the rule of the
    // executor is that only a session other than the requester's counts: the requester's own
    // (and an empty presence) leave "everything staged" allowed.
    for presence in [
        alone(),
        Arc::new(|_: &Path| Some(vec![SESSION.to_owned()])) as Presence,
    ] {
        let rig = rig_with(presence);
        stage_a_leave_b(&rig);
        let before = rig.commits();

        rig.commit(None);

        assert_eq!(rig.commits(), before + 1, "one commit");
        assert_eq!(rig.changed_by_head(), "M\ta.txt", "only what was staged");
    }
}

#[test]
fn a_commit_never_carries_the_active_session_warning() {
    let rig = rig_with(other_agent());
    stage_a_leave_b(&rig);

    let prepared = rig
        .try_prepared(Some(&["b.txt"]))
        .expect("named paths are accepted in a shared worktree");

    assert!(
        prepared.warnings.is_empty(),
        "a commit has no warnings, active-session included: {:?}",
        prepared.warnings
    );
}

/// MCP010-G61 (⛔6.1): a presence that cannot be read counts as shared: if it counted as
/// nobody, one agent would commit what another had staged.
#[test]
fn an_unreadable_presence_counts_as_shared() {
    let rig = rig_with(Arc::new(|_: &Path| None));
    stage_a_leave_b(&rig);
    let (commits, index) = (rig.commits(), rig.index());

    let got = rig.try_prepare(None);

    assert_eq!(
        got,
        Err(ExecError::Rejected(RejectReason::SharedWorktreeNeedsPaths)),
        "an unreadable presence must be refused like a shared worktree"
    );
    assert_eq!(rig.commits(), commits, "no commit");
    assert_eq!(rig.index(), index, "the index is as it was");
    // With named paths nothing is staged for anybody else: the commit goes on.
    rig.commit(Some(&["b.txt"]));
}

/// MCP010-G62 (⛔6.2): the open session of an agent whose process died is not in the presence,
/// so it does not block "everything staged" in that worktree for ever.
#[test]
fn a_crashed_agents_session_does_not_block_all_staged() {
    // The production presence drops a session whose `(pid, start)` no longer exists; here the
    // presence is what it returns for a worktree whose only other session crashed.
    let rig = rig_with(alone());
    stage_a_leave_b(&rig);
    let before = rig.commits();

    rig.commit(None);

    assert_eq!(rig.commits(), before + 1, "all staged was not blocked");
    assert_eq!(rig.changed_by_head(), "M\ta.txt");
}
