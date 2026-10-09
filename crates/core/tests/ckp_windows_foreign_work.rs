//! BR-CKP-AUTH-003 on Windows (H-01): the Cockpit never offers, nor accepts, the confirmation of
//! another actor's work there (TQ-14, ADR-CKP-002 § 3, ADR-TMC-005 § 3 and its note on
//! parent-process spoofing). An "unattributed" caller with layer `cockpit` that asks to merge,
//! rebase or discard an agent's work gets `confirmation-unavailable` at prepare: no challenge,
//! no plan, no oplog entry, and the step never runs. macOS and Linux keep the challenge.
//!
//! The rule is injected (`with_foreign_work_confirmable`) so both branches run on every OS; the
//! production default is checked against the platform this test runs on. The executor runs
//! against doubles of the repo layer and a real oplog in a temporary profile, never a real repo
//! or profile (NFR-01).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gitraptor_api::catalog::{
    Layer, OperationArgs, OperationId, PrepareParams, RejectReason, WarningCode,
};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::{
    Affected, Caller, ExecError, Executor, NoGuardrails, OpPlan, PlanError, PrepareInput, Prepared,
    RepoFacts, StepPlan,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{Oplog, Requester, RequesterOrigin};
use gitraptor_core::timemachine::protected::scope::McpAllowlist;
use gitraptor_core::timemachine::protected::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedBackend, ProtectedStep,
    RepoHandle, ScopeError, StepError,
};
use serde_json::{Map, json};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000cc";

/// The operations of BR-CKP-AUTH-003.
const ON_FOREIGN_WORK: [OperationId; 3] = [
    OperationId::MergeIntoBase,
    OperationId::RebaseOntoBase,
    OperationId::DiscardWorktree,
];

// ----- Doubles ---------------------------------------------------------------------------

struct NoPrior;

impl PriorSnapshotter for NoPrior {
    fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        Err(PriorError::StoreUnavailable)
    }
}

struct Nothing;

impl McpAllowlist for Nothing {
    fn allows(&self, _repo_id: &str) -> bool {
        false
    }
}

fn facts() -> RepoFacts {
    RepoFacts {
        root: "/w/shop-feat".into(),
        git_dir: "/w/shop/.git/worktrees/feat".into(),
        common_dir: "/w/shop/.git".into(),
        linked: true,
        root_id: Some((1, 2)),
        dot_git_id: Some((1, 3)),
        gitdir_linked_back: true,
        head_branch: Some("feat-wip".into()),
        head_commit: Some("1".repeat(40)),
        detached: false,
        in_progress: None,
        git_locks: Vec::new(),
        locked: false,
        grafts: false,
        branches_elsewhere: Vec::new(),
    }
}

/// Plans every operation as touching `affected`; counts the steps it is asked to build.
struct Backend {
    repo: RepoHandle,
    allow: Nothing,
    affected: Affected,
    steps: AtomicUsize,
}

impl ProtectedBackend for Backend {
    fn repo_of(&self, _folder: &Path) -> Result<RepoHandle, ScopeError> {
        Ok(self.repo.clone())
    }
    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allow
    }
    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        format!("ckp-windows-foreign-work/{}", repo.repo_id)
    }
    fn facts(&self, _repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        Ok(facts())
    }
    fn plan_op(
        &self,
        _operation: OperationId,
        _args: &OperationArgs,
        _repo: &RepoHandle,
        _facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        Ok(OpPlan {
            expected: json!({ "tip": "1".repeat(40) }),
            warnings: Vec::new(),
            affected: self.affected.clone(),
            other_session: false,
        })
    }
    fn step(&self, _plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        self.steps.fetch_add(1, Ordering::SeqCst);
        Err(StepError::new("this test never runs a step"))
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    oplog: Arc<Mutex<Oplog>>,
    backend: Backend,
    executor: Executor,
}

fn rig(affected: Affected, executor: Executor) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let oplog = Arc::new(Mutex::new(oplog));
    let repo = RepoHandle {
        repo_id: REPO.into(),
        worktree: "/w/shop-feat".into(),
        oplog: Arc::clone(&oplog),
        snapshotter: Arc::new(NoPrior),
    };
    Rig {
        _tmp: tmp,
        oplog,
        backend: Backend {
            repo,
            allow: Nothing,
            affected,
            steps: AtomicUsize::new(0),
        },
        executor,
    }
}

/// The executor with the platform rule forced: `false` is Windows, `true` macOS and Linux.
fn executor(confirmable: bool) -> Executor {
    Executor::new(Arc::new(NoGuardrails)).with_foreign_work_confirmable(confirmable)
}

fn claude2() -> Affected {
    Affected::Agent {
        session_id: "session-claude-2".into(),
    }
}

/// "Tú u otro (sin atribuir)" from the developer's terminal: layer `cockpit`.
fn unattributed() -> Resolution {
    Resolution {
        who: Who {
            actor: Actor::Unattributed,
            requester: Requester::Unattributed,
        },
        via: ResolvedVia::Ancestry,
        executor_operation: None,
        confirmable: true,
    }
}

fn agent() -> Resolution {
    Resolution {
        who: Who {
            actor: Actor::Agent {
                kind: actor::AgentKind::ClaudeCode,
                name: None,
                origin: actor::AgentOrigin::Detected,
            },
            requester: Requester::Agent {
                name: "claude-1".into(),
                origin: RequesterOrigin::Detected,
                session_id: "session-claude-1".into(),
            },
        },
        via: ResolvedVia::Ancestry,
        executor_operation: None,
        confirmable: false,
    }
}

impl Rig {
    fn prepare(
        &self,
        op: OperationId,
        resolution: &Resolution,
        layer: Layer,
    ) -> Result<Prepared, ExecError> {
        let params = PrepareParams {
            operation: op,
            worktree: None,
            args: Map::new(),
            surface: None,
            session_env: Vec::new(),
        };
        let channel = if layer == Layer::Mcp {
            RequestChannel::Mcp
        } else {
            RequestChannel::Tui
        };
        self.executor.prepare(
            &self.backend,
            PrepareInput {
                caller: Caller {
                    connection: 11,
                    pid: 4_343,
                    start_us: 1,
                    mcp: layer == Layer::Mcp,
                },
                resolution,
                layer,
                channel,
                params: &params,
                repo: self.backend.repo.clone(),
                // The caller passes the confirmation checks: only the platform rule is left.
                confirm_refusal: None,
            },
        )
    }

    /// Nothing was recorded in the repo's oplog and no step was built.
    fn untouched(&self) -> bool {
        self.oplog.lock().unwrap().last_seq().unwrap() == 0
            && self.backend.steps.load(Ordering::SeqCst) == 0
    }
}

// ----- Tests -------------------------------------------------------------------------------

/// Windows: merge, rebase and discard of claude-2's work by an unattributed caller are refused
/// with the rule's reason; no challenge, no plan, no oplog entry, no step.
#[test]
fn on_windows_foreign_work_is_refused_without_a_challenge() {
    for op in ON_FOREIGN_WORK {
        let rig = rig(claude2(), executor(false));
        let got = rig.prepare(op, &unattributed(), Layer::Cockpit);
        assert_eq!(
            got.as_ref().err(),
            Some(&ExecError::Rejected(RejectReason::ConfirmationUnavailable)),
            "{op:?}: {got:?}"
        );
        assert_eq!(rig.executor.live_plans(), 0, "{op:?}: no plan is kept");
        assert!(
            rig.untouched(),
            "{op:?}: the repo and its oplog stay as they were"
        );
    }
}

/// Windows: work whose attribution is mixed or unclear counts as another actor's.
#[test]
fn on_windows_unclear_work_is_refused_too() {
    let rig = rig(Affected::Other, executor(false));
    let got = rig.prepare(
        OperationId::DiscardWorktree,
        &unattributed(),
        Layer::Cockpit,
    );
    assert_eq!(
        got.err(),
        Some(ExecError::Rejected(RejectReason::ConfirmationUnavailable))
    );
    assert!(rig.untouched());
}

/// Windows: the unattributed caller's own work and nobody's work need no confirmation, so
/// nothing changes for them.
#[test]
fn on_windows_work_without_another_actor_still_plans() {
    for affected in [Affected::Unattributed, Affected::Nobody] {
        let rig = rig(affected.clone(), executor(false));
        let plan = rig
            .prepare(OperationId::MergeIntoBase, &unattributed(), Layer::Cockpit)
            .unwrap_or_else(|e| panic!("{affected:?}: {e:?}"));
        assert_eq!(plan.challenge, None, "{affected:?}");
        assert!(
            !plan.warnings.contains(&WarningCode::OtherActorsWork),
            "{affected:?}"
        );
    }
}

/// Windows: an agent over another agent's work keeps its own reason.
#[test]
fn on_windows_an_agent_keeps_foreign_work() {
    let rig = rig(claude2(), executor(false));
    let got = rig.prepare(OperationId::RebaseOntoBase, &agent(), Layer::Mcp);
    assert_eq!(
        got.err(),
        Some(ExecError::Rejected(RejectReason::ForeignWork))
    );
    assert!(rig.untouched());
}

/// macOS and Linux: nothing changes, the plan carries its one-use challenge.
#[test]
fn on_unix_foreign_work_still_offers_the_challenge() {
    for op in ON_FOREIGN_WORK {
        let rig = rig(claude2(), executor(true));
        let plan = rig
            .prepare(op, &unattributed(), Layer::Cockpit)
            .unwrap_or_else(|e| panic!("{op:?}: {e:?}"));
        assert!(plan.challenge.is_some(), "{op:?}: a challenge is offered");
        assert!(
            plan.warnings.contains(&WarningCode::OtherActorsWork),
            "{op:?}"
        );
        assert!(rig.untouched(), "{op:?}: prepare writes nothing");
    }
}

/// The production default follows the platform this runs on: no confirmation on Windows.
#[test]
fn the_default_rule_follows_the_platform() {
    let rig = rig(claude2(), Executor::new(Arc::new(NoGuardrails)));
    let got = rig.prepare(
        OperationId::DiscardWorktree,
        &unattributed(),
        Layer::Cockpit,
    );
    if cfg!(windows) {
        assert_eq!(
            got.err(),
            Some(ExecError::Rejected(RejectReason::ConfirmationUnavailable))
        );
    } else {
        assert!(got.expect("unix plans it").challenge.is_some());
    }
    assert!(rig.untouched());
}
