//! The guaranteed prior confirms the intent's mark (fix/tm-restore-calm-mark, D12): when the
//! engine's mark moves between the intent and the prior, the prior is taken again; after three
//! tries the operation is refused with `git-busy` and its step never runs.
//!
//! A testkit fixture (temporary repo, home and profile; never this repo or the real profile,
//! NFR-01), the real store behind the prior, and a step double that only records that it ran. The
//! engine moves its mark right after every answer, so every confirmation sees another mark.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use gitraptor_api::catalog::{
    Layer, OperationArgs, OperationId, PrepareParams, RejectReason, RunParams,
};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_testkit::Fixture;
use serde_json::json;

use super::{
    Affected, Caller, ExecError, Executor, NoGuardrails, OpPlan, PlanError, PrepareInput,
    RepoFacts, RunEnv, RunInput, StepPlan,
};
use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::{ProcError, ProcInfo, ProcSource};
use crate::channel::requester::{Resolution, Who};
use crate::profile::ProfileDirs;
use crate::timemachine::engine::{EngineLink, RawGitEvent};
use crate::timemachine::manual::{ManualAsk, ManualCaptured, ManualError};
use crate::timemachine::oplog::Oplog;
use crate::timemachine::protected::scope::McpAllowlist;
use crate::timemachine::protected::{
    ProtectedBackend, ProtectedStep, RepoHandle, ScopeError, StepCtx, StepError, StepOutput,
    StepScope, StoreSnapshotter, TmRepos,
};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000bb";

/// An engine always calm whose mark moves right after every answer: an event persisted between
/// the intent's mark and the prior's validity point, every time.
struct Moving(AtomicI64);

impl EngineLink for Moving {
    fn mark(&self, _repo_id: &str) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
        Some(self.0.fetch_add(1, Ordering::SeqCst))
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(Vec::new())
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, _repo_id: &str) -> u64 {
        0
    }
}

struct Allow;

impl McpAllowlist for Allow {
    fn allows(&self, _repo_id: &str) -> bool {
        true
    }
}

/// A step that only records that it ran.
struct Recorded(Arc<AtomicBool>);

impl ProtectedStep for Recorded {
    fn subtype(&self) -> &str {
        "abort-in-progress"
    }
    fn scope(&self) -> StepScope {
        StepScope {
            worktrees: Vec::new(),
            refs: Vec::new(),
        }
    }
    fn run(&mut self, _ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        self.0.store(true, Ordering::SeqCst);
        Ok(StepOutput {
            changed_refs: Vec::new(),
            outcome: None,
            git_output: None,
            new_commit: None,
        })
    }
}

struct Backend {
    repo: RepoHandle,
    facts: RepoFacts,
    lock_key: String,
    ran: Arc<AtomicBool>,
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, _folder: &Path) -> Result<RepoHandle, ScopeError> {
        Ok(self.repo.clone())
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &Allow
    }
    fn write_lock_key(&self, _repo: &RepoHandle) -> String {
        self.lock_key.clone()
    }
    fn facts(&self, _repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        Ok(self.facts.clone())
    }
    fn plan_op(
        &self,
        _operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        _facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        Ok(OpPlan {
            expected: json!({}),
            warnings: Vec::new(),
            affected: Affected::Nobody,
            other_session: false,
            governed: None,
        })
    }
    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        Ok(Box::new(Recorded(Arc::clone(&self.ran))))
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

fn caller() -> Caller {
    Caller {
        connection: 7,
        pid: 4_242,
        start_us: 1,
        mcp: false,
    }
}

/// C14: the engine's mark moves between the intent and the prior, every time: the operation is
/// refused with `git-busy`, its step never runs and the repo is intact.
#[test]
fn a_guaranteed_prior_whose_mark_moved_refuses_with_git_busy() {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let head = f.git(&["rev-parse", "HEAD"]);
    let status = f.git(&["status", "--porcelain"]);
    let dirs = ProfileDirs::under_root(&f.profile);
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    // The repo's oplog and store as the daemon opens them.
    let repos = TmRepos::new(dirs.clone());
    repos.insert(REPO, &common, oplog);
    let (oplog, store) = repos.repo(REPO).unwrap();
    let repo = RepoHandle {
        repo_id: REPO.into(),
        worktree: f.repo.clone(),
        common_dir: common.clone(),
        oplog: Arc::clone(&oplog),
        snapshotter: Arc::new(StoreSnapshotter {
            store: store.unwrap(),
            oplog: Arc::clone(&oplog),
            profile: dirs.clone(),
            common_dir: common.clone(),
        }),
    };
    let ran = Arc::new(AtomicBool::new(false));
    let backend = Backend {
        repo,
        facts: RepoFacts {
            root: f.repo.clone(),
            git_dir: common.clone(),
            common_dir: common.clone(),
            linked: false,
            root_id: Some((1, 2)),
            dot_git_id: Some((1, 3)),
            gitdir_linked_back: true,
            head_branch: Some("main".into()),
            head_commit: Some(head.trim().to_owned()),
            detached: false,
            in_progress: None,
            git_locks: Vec::new(),
            locked: false,
            grafts: false,
            branches_elsewhere: Vec::new(),
        },
        lock_key: format!("prior-mark-tests/{}", f.root.display()),
        ran: Arc::clone(&ran),
    };
    let resolution = Resolution {
        who: Who::unattributed(),
        via: ResolvedVia::None,
        executor_operation: None,
        confirmable: true,
    };
    let executor = Executor::new(Arc::new(NoGuardrails));
    let args = serde_json::Map::new();
    let prepared = executor
        .prepare(
            &backend,
            PrepareInput {
                caller: caller(),
                resolution: &resolution,
                layer: Layer::Cockpit,
                channel: RequestChannel::Tui,
                params: &PrepareParams {
                    operation: OperationId::AbortInProgress,
                    worktree: None,
                    args,
                    surface: None,
                    session_env: Vec::new(),
                },
                repo: backend.repo.clone(),
                confirm_refusal: None,
            },
        )
        .unwrap_or_else(|e| panic!("prepare: {e:?}"));

    let engine: Arc<dyn EngineLink> = Arc::new(Moving(AtomicI64::new(1)));
    let marks = Arc::new(ExecutorMarks::default());
    let stopping = AtomicBool::new(false);
    let mark_engine = Arc::clone(&engine);
    let engine_mark = move |repo: &str, worktrees: &[PathBuf]| {
        mark_engine.settle(repo, worktrees, Duration::from_secs(2))
    };
    let after_step = |_repo: &str, _worktrees: &[PathBuf], _operation: &str| {};
    let publish = |_kind: &str, _data| {};
    let capture = |_ask: &ManualAsk| Err::<ManualCaptured, _>(ManualError::Unavailable);
    let env = RunEnv {
        marks: &marks,
        procs: &NoProcs,
        stopping: &stopping,
        prior_deadline: Duration::from_secs(10),
        engine_mark: &engine_mark,
        engine: Some(engine),
        after_step: &after_step,
        publish: &publish,
        capture: &capture,
    };
    let again = || Ok((resolution.clone(), Layer::Cockpit, None));
    let out = executor.run(
        &backend,
        RunInput {
            caller: caller(),
            resolution: &resolution,
            params: &RunParams {
                plan_id: prepared.plan_id,
                accepted_warnings: Vec::new(),
                confirmation: None,
            },
            resolve_again: &again,
            rescope: &|_repo: &RepoHandle| Ok(()),
        },
        &env,
    );
    assert!(
        matches!(out, Err(ExecError::Rejected(RejectReason::GitBusy))),
        "a prior whose mark keeps moving is refused as git-busy: {out:?}"
    );
    assert!(!ran.load(Ordering::SeqCst), "the step never ran");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head, "the repo is intact");
    assert_eq!(f.git(&["status", "--porcelain"]), status);
}
