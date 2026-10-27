//! Debug-only arms of the governed operations, so the process tests can drive the real engine,
//! the real `raptor-hook` and the binding of the executor's `git` (D14). Compiled only with
//! `debug_assertions` and armed only with `GITRAPTOR_TEST_EXECUTOR_ARMS=1` plus
//! `GITRAPTOR_PROFILE_DIR`.
//!
//! They are not the product's logic of these operations (each one is its story's): no
//! preconditions beyond what the shape needs, and the test `discard-worktree` has no
//! confirmation naming what cannot be recovered (R-CKP-6). They run inside the protected
//! operation (prior snapshot first) and launch `git` only through [`run_git`].

use std::path::{Path, PathBuf};

use gitraptor_api::catalog::{OperationArgs, OperationId, OperationOutcome, RejectReason};
use gitraptor_git::resolve::{GitVersion, SystemGit};
use gitraptor_git::user_ops::{RepoTarget, UserGitCommand, UserOp, rejecting_editor};
use gitraptor_git::{Oid, ReaderOptions, RefName, RepoReader};
use gitraptor_policy::guard::plan::{Expected, PlanShape};

use super::super::git::{run_git, run_git_with};
use super::super::{Affected, CancelToken, OpPlan, PlanError, RepoFacts, StepPlan};
use crate::timemachine::protected::{
    ProtectedStep, RepoHandle, StepCtx, StepError, StepOutput, StepScope,
};

/// The operations the test arms serve.
pub(super) const TEST_ARMS: &[OperationId] = &[
    OperationId::MergeIntoBase,
    OperationId::CreateWorktree,
    OperationId::DiscardWorktree,
    OperationId::RebaseOntoBase,
];

/// What the test arms need from the daemon's environment, read once.
#[derive(Debug, Clone)]
pub(super) struct TestArms {
    git: PathBuf,
    home: Option<PathBuf>,
}

impl TestArms {
    pub(super) fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            git: env(crate::daemon::TEST_GIT_ENV)
                .filter(|g| !g.is_empty())
                .map_or_else(|| PathBuf::from("/usr/bin/git"), PathBuf::from),
            // The test fixture's home next to its profile; never the user's own (NFR-01).
            home: env(crate::profile::PROFILE_DIR_ENV)
                .filter(|p| !p.is_empty())
                .and_then(|p| Path::new(&p).parent().map(|r| r.join("home")))
                .filter(|h| h.is_dir()),
        }
    }
}

fn rejected() -> PlanError {
    PlanError::Rejected(RejectReason::StateChanged)
}

fn full(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

fn reader(path: &Path) -> Result<RepoReader, PlanError> {
    RepoReader::open(path, &ReaderOptions::default()).map_err(|_| rejected())
}

fn resolve(reader: &RepoReader, refname: &str) -> Result<String, PlanError> {
    let name = RefName::new(refname).map_err(|_| rejected())?;
    reader
        .resolve_ref(&name)
        .ok()
        .flatten()
        .ok_or_else(rejected)
}

fn plan(shape: PlanShape) -> OpPlan {
    OpPlan {
        expected: serde_json::Value::Null,
        warnings: Vec::new(),
        affected: Affected::Nobody,
        other_session: false,
        governed: Some(shape),
    }
}

/// The plan of one test arm: its shape, read from the repo.
pub(super) fn plan_op(
    operation: OperationId,
    args: &OperationArgs,
    repo: &RepoHandle,
    facts: &RepoFacts,
) -> Result<OpPlan, PlanError> {
    let common = reader(&repo.common_dir)?;
    let base = gitraptor_policy::team::resolve_main_branch(&common)
        .map(|m| m.name.as_str().to_owned())
        .unwrap_or_else(|_| "main".to_owned());
    let head = || facts.head_commit.clone().ok_or_else(rejected);
    let branch = || facts.head_branch.clone().ok_or_else(rejected);
    let shape = match (operation, args) {
        (OperationId::MergeIntoBase, _) => PlanShape::Merge {
            base: full(&base),
            old: resolve(&common, &full(&base))?,
            integrated: head()?,
        },
        (OperationId::RebaseOntoBase, _) => PlanShape::Rebase {
            branch: full(&branch()?),
            upstream: full(&base),
            old_tip: head()?,
            onto: resolve(&common, &full(&base))?,
        },
        (OperationId::CreateWorktree, OperationArgs::CreateWorktree(a)) => {
            if a.path.is_none() {
                return Err(rejected());
            }
            PlanShape::CreateWorktree {
                branch: full(&a.branch),
                at: head()?,
            }
        }
        (OperationId::DiscardWorktree, _) => PlanShape::DeleteWorktree {
            branch: full(&branch()?),
            tip: head()?,
        },
        _ => return Err(PlanError::NotImplemented),
    };
    Ok(plan(shape))
}

/// One `git` of a test arm, run as a marked child of the step.
struct Launch<'a> {
    arms: &'a TestArms,
    git_dir: PathBuf,
    session: gitraptor_git::user_ops::SessionEnv,
    cancel: CancelToken,
}

impl Launch<'_> {
    fn command(
        &self,
        op: &UserOp,
        work_tree: Option<&Path>,
        message: Option<&str>,
    ) -> Result<UserGitCommand, StepError> {
        let git = SystemGit {
            path: self.arms.git.clone(),
            version: GitVersion {
                major: 2,
                minor: 0,
                patch: 0,
            },
        };
        let target = RepoTarget {
            git_dir: self.git_dir.clone(),
            work_tree: work_tree.map(Path::to_path_buf),
        };
        UserGitCommand::new(
            &git,
            &target,
            op,
            message,
            &self.session,
            &rejecting_editor(None),
            self.arms.home.as_deref(),
        )
        .map_err(StepError::new)
    }

    /// Runs `op`; `Ok(false)` when `git` failed.
    fn run(
        &self,
        ctx: &mut StepCtx<'_>,
        op: &UserOp,
        work_tree: Option<&Path>,
        message: Option<&str>,
        transitions: &[Expected],
    ) -> Result<bool, StepError> {
        let cmd = self.command(op, work_tree, message)?;
        let ran = if transitions.is_empty() {
            run_git(ctx, cmd, &self.cancel)
        } else {
            run_git_with(ctx, cmd, &self.cancel, transitions)
        };
        Ok(ran
            .map_err(|e| StepError::new(e.to_string()))?
            .status
            .success())
    }
}

fn oid(hex: &str) -> Result<Oid, StepError> {
    Oid::from_hex(hex).ok_or_else(|| StepError::new("invalid oid"))
}

fn refname(name: &str) -> Result<RefName, StepError> {
    RefName::new(name).map_err(|_| StepError::new("invalid ref"))
}

/// The step of one test arm.
struct TestStep {
    arms: TestArms,
    operation: OperationId,
    shape: PlanShape,
    args: OperationArgs,
    worktree: PathBuf,
    common_dir: PathBuf,
    session: gitraptor_git::user_ops::SessionEnv,
    cancel: CancelToken,
}

pub(super) fn step(
    arms: &TestArms,
    plan: &StepPlan<'_>,
) -> Result<Box<dyn ProtectedStep>, StepError> {
    let shape = plan
        .op_plan
        .governed
        .clone()
        .ok_or_else(|| StepError::new("a test arm needs its shape"))?;
    Ok(Box::new(TestStep {
        arms: arms.clone(),
        operation: plan.operation,
        shape,
        args: plan.args.clone(),
        worktree: plan.facts.root.clone(),
        common_dir: plan.repo.common_dir.clone(),
        session: plan.session_env.clone(),
        cancel: plan.cancel.clone(),
    }))
}

impl ProtectedStep for TestStep {
    fn subtype(&self) -> &str {
        self.operation.as_str()
    }

    fn scope(&self) -> StepScope {
        StepScope::default()
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        let launch = Launch {
            arms: &self.arms,
            git_dir: self.common_dir.clone(),
            session: self.session.clone(),
            cancel: self.cancel.clone(),
        };
        let failed = || Err(StepError::new("git failed"));
        let wt = self.worktree.clone();
        let mut outcome = None;
        match (&self.shape, &self.args) {
            (
                PlanShape::Merge {
                    base,
                    old,
                    integrated,
                },
                _,
            ) => {
                let op = UserOp::UpdateRef {
                    name: refname(base)?,
                    new: oid(integrated)?,
                    old: oid(old)?,
                };
                if !launch.run(ctx, &op, None, None, &[])? {
                    return failed();
                }
            }
            (
                PlanShape::Rebase {
                    branch,
                    old_tip,
                    onto,
                    ..
                },
                _,
            ) => {
                // The worktree's own `.git` file names its Git directory.
                let rebase = UserOp::RebaseOnto { onto: oid(onto)? };
                let in_worktree = Launch {
                    git_dir: worktree_git_dir(&wt),
                    ..launch
                };
                if !in_worktree.run(ctx, &rebase, Some(&wt), None, &[])? {
                    // Atomic: back to the previous tip, with its own transition.
                    let back = [Expected::RestoreTo {
                        refname: branch.clone(),
                        value: old_tip.clone(),
                    }];
                    if !in_worktree.run(ctx, &UserOp::RebaseAbort, Some(&wt), None, &back)? {
                        return failed();
                    }
                    outcome = Some(OperationOutcome::ConflictReverted);
                }
            }
            (PlanShape::CreateWorktree { branch, at }, OperationArgs::CreateWorktree(a)) => {
                let path = a.path.as_deref().map(PathBuf::from).ok_or_else(|| {
                    StepError::new("create-worktree needs a path in the test arm")
                })?;
                let short = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                let op = UserOp::WorktreeAdd {
                    branch: refname(short)?,
                    path,
                    start: oid(at)?,
                };
                if !launch.run(ctx, &op, None, None, &[])? {
                    return failed();
                }
            }
            (PlanShape::DeleteWorktree { branch, tip }, _) => {
                let remove = UserOp::WorktreeRemove { path: wt.clone() };
                if !launch.run(ctx, &remove, None, None, &[])? {
                    return failed();
                }
                let delete = UserOp::DeleteRef {
                    name: refname(branch)?,
                    old: oid(tip)?,
                };
                if !launch.run(ctx, &delete, None, None, &[])? {
                    return failed();
                }
            }
            _ => return Err(StepError::new("arguments do not match the shape")),
        }
        Ok(StepOutput {
            outcome,
            ..StepOutput::default()
        })
    }
}

/// The Git directory of the worktree rooted at `root`: `.git` itself, or the directory its
/// `.git` file points to (a linked worktree).
fn worktree_git_dir(root: &Path) -> PathBuf {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return dot_git;
    }
    std::fs::read_to_string(&dot_git)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|l| l.strip_prefix("gitdir: ").map(str::trim).map(PathBuf::from))
        })
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .unwrap_or(dot_git)
}
