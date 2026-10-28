//! The operation's own part of `commit`: a governed, protected commit in the requester's
//! worktree. The plan reads the worktree with gitoxide; the step adds the named new files,
//! commits through the closed list of user operations (message by stdin, hooks run) and lets
//! the branch tip, read again under the lock, decide what happened. Git's output never does.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gitraptor_api::catalog::{MAX_REPORTED_COMMIT_PATHS, OperationArgs, OperationId, RejectReason};
use gitraptor_git::user_ops::{RepoTarget, SessionEnv, UserGitCommand, UserOp, rejecting_editor};
use gitraptor_git::{Count, ReaderOptions, RefName, RepoReader, SystemGit};
use gitraptor_policy::guard::plan::PlanShape;

use super::commit_paths::{Selection, WorktreeView, select};
use crate::executor::git::{GitRun, run_git};
use crate::executor::{Affected, CancelToken, Interrupt, OpPlan, PlanError, RepoFacts, StepPlan};
use crate::timemachine::protected::{
    NewCommit, ProtectedStep, RepoHandle, StepCtx, StepError, StepFailure, StepOutput, StepScope,
};

/// The oid of a branch that has no commit yet.
const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// How long reading the new commit's paths may take under the lock.
const READ_PATHS_DEADLINE: Duration = Duration::from_secs(10);

/// What the commit arm needs from the daemon, read once at start.
#[derive(Debug, Clone)]
pub struct CommitEnv {
    /// The system Git user operations run with.
    pub git: SystemGit,
    /// The daemon's `HOME`, for the user's Git configuration and hooks.
    pub home: Option<PathBuf>,
}

fn rejected(r: RejectReason) -> PlanError {
    PlanError::Rejected(r)
}

std::thread_local! {
    /// The path the last commit plan of this thread refused. `PlanError` is `Copy` and
    /// `ExecError::Rejected` carries the reason alone, so the path travels beside them to the
    /// channel, which plans on the connection's own thread and takes it right after the refusal.
    static REFUSED_PATH: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The path the last commit plan on this thread refused, once: repo text, untrusted.
pub(crate) fn take_refused_path() -> Option<String> {
    REFUSED_PATH.with(|p| p.borrow_mut().take())
}

/// The plan: refused before the intent without a usable Git (or on Windows until it can launch
/// user operations); otherwise the selection, read from the worktree, in the fingerprint.
pub(super) fn plan_op(
    env: Option<&CommitEnv>,
    args: &OperationArgs,
    repo: &RepoHandle,
    facts: &RepoFacts,
) -> Result<OpPlan, PlanError> {
    REFUSED_PATH.with(|p| p.borrow_mut().take());
    if cfg!(windows) || env.is_none() {
        return Err(rejected(RejectReason::GitUnavailable));
    }
    let OperationArgs::Commit(a) = args else {
        return Err(PlanError::NotImplemented);
    };
    let branch = facts
        .head_branch
        .clone()
        .ok_or(rejected(RejectReason::DetachedHead))?;
    let old = facts
        .head_commit
        .clone()
        .unwrap_or_else(|| ZERO_OID.to_owned());
    let view = read_view(facts)?;
    let selection = select(a, &view).map_err(|(r, path)| {
        REFUSED_PATH.with(|p| *p.borrow_mut() = path);
        rejected(r)
    })?;
    // Guardrails judges what will be committed: everything staged, or the named paths.
    let paths = if selection.all_staged {
        view.staged.clone()
    } else {
        selection.paths.clone()
    };
    Ok(OpPlan {
        expected: serde_json::json!({ "old": old, "branch": branch, "selection": selection }),
        warnings: Vec::new(),
        // The executor fixes the requester's own work (it knows the requester).
        affected: Affected::Nobody,
        other_session: false,
        governed: Some(PlanShape::Commit {
            branch: format!("refs/heads/{branch}"),
            old,
            paths,
            authorship: crate::guardrails::plan_gate::commit_facts(&repo.common_dir, &a.message),
        }),
    })
}

/// The worktree as the registry's gate opened it: its Git directory with the root as an explicit
/// work tree, so the worktree's `.git` is not re-resolved and swapping it after the gate cannot
/// change what the plan reads.
fn read_view(facts: &RepoFacts) -> Result<WorktreeView, PlanError> {
    let reader = RepoReader::open_worktree(&facts.git_dir, &facts.root, &ReaderOptions::default())
        .map_err(|_| rejected(RejectReason::RepoIdentityChanged))?;
    WorktreeView::read(&facts.root, reader).map_err(rejected)
}

/// The step of a checked plan. Its Git directory comes from the facts the registry's gate
/// opened, never from the worktree's `.git`.
pub(super) fn step(
    env: Option<&CommitEnv>,
    plan: &StepPlan<'_>,
) -> Result<Box<dyn ProtectedStep>, StepError> {
    let env = env.ok_or_else(|| StepError::unchanged(StepFailure::Unchanged))?;
    let OperationArgs::Commit(a) = plan.args else {
        return Err(StepError::new("arguments do not match the operation"));
    };
    let expected = &plan.op_plan.expected;
    let text = |k: &str| expected.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    let (Some(old), Some(branch)) = (text("old"), text("branch")) else {
        return Err(StepError::unchanged(StepFailure::Unchanged));
    };
    let selection: Selection = expected
        .get("selection")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .ok_or_else(|| StepError::unchanged(StepFailure::Unchanged))?;
    Ok(Box::new(CommitStep {
        env: env.clone(),
        git_dir: plan.facts.git_dir.clone(),
        common_dir: plan.repo.common_dir.clone(),
        root: plan.facts.root.clone(),
        old,
        branch,
        selection,
        message: a.message.clone(),
        session: plan.session_env.clone(),
        cancel: plan.cancel.clone(),
    }))
}

struct CommitStep {
    env: CommitEnv,
    git_dir: PathBuf,
    common_dir: PathBuf,
    root: PathBuf,
    old: String,
    branch: String,
    selection: Selection,
    message: String,
    session: SessionEnv,
    cancel: CancelToken,
}

impl CommitStep {
    fn command(&self, op: &UserOp, message: Option<&str>) -> Result<UserGitCommand, StepError> {
        let target = RepoTarget {
            git_dir: self.git_dir.clone(),
            work_tree: Some(self.root.clone()),
        };
        UserGitCommand::new(
            &self.env.git,
            &target,
            op,
            message,
            &self.session,
            &rejecting_editor(None),
            self.env.home.as_deref(),
        )
        .map_err(|_| StepError::unchanged(StepFailure::Unchanged))
    }

    fn launch(
        &self,
        ctx: &mut StepCtx<'_>,
        op: &UserOp,
        message: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<std::io::Result<GitRun>, StepError> {
        Ok(run_git(ctx, self.command(op, message)?, cancel))
    }

    /// The tip of the branch now, read from the common directory under the lock.
    fn tip(&self) -> Result<Option<String>, StepError> {
        let reader = RepoReader::open(&self.common_dir, &ReaderOptions::default())
            .map_err(|_| StepError::new("the repo cannot be read"))?;
        let name = RefName::new(&format!("refs/heads/{}", self.branch))
            .map_err(|_| StepError::new("invalid branch"))?;
        reader
            .resolve_ref(&name)
            .map_err(|_| StepError::new("the branch cannot be read"))
    }

    /// The commit at `new`, if it is the one this step made: a single new commit on `old`.
    fn landed(&self, new: &str) -> Result<NewCommit, StepError> {
        let reader = RepoReader::open(&self.common_dir, &ReaderOptions::default())
            .map_err(|_| StepError::new("the repo cannot be read"))?;
        let old = (self.old != ZERO_OID).then_some(self.old.as_str());
        if let Some(old) = old {
            match reader.ahead_behind_commits(new, old, 2) {
                Ok((Count::Exact(1), Count::Exact(0))) => {}
                _ => return Err(StepError::new("branch moved")),
            }
        }
        let changed = reader
            .changed_paths(
                old,
                new,
                MAX_REPORTED_COMMIT_PATHS,
                Instant::now() + READ_PATHS_DEADLINE,
            )
            .map_err(|_| StepError::new("the new commit cannot be read"))?;
        Ok(NewCommit {
            oid: new.to_owned(),
            paths: changed.paths,
            total: usize::try_from(changed.total).unwrap_or(usize::MAX),
        })
    }

    /// Takes the added files out of the index again, with a token of its own so the stop of
    /// the commit does not cut it. `false` when the index could not be put back.
    fn unstage(&self, ctx: &mut StepCtx<'_>) -> bool {
        if self.selection.add_first.is_empty() {
            return true;
        }
        let op = UserOp::UnstagePaths {
            paths: self.selection.add_first.clone(),
        };
        matches!(
            self.launch(ctx, &op, None, &CancelToken::default()),
            Ok(Ok(run)) if run.status.success()
        )
    }
}

impl CommitStep {
    /// The failure of a `git` that did not land: the files it added are unstaged again, and it
    /// is unchanged only if that worked and no hook descendant had to be killed (one may have
    /// died mid-write).
    fn stopped(&self, ctx: &mut StepCtx<'_>, ran: &std::io::Result<GitRun>) -> StepError {
        let stop = stop_of(ran);
        if !self.unstage(ctx) {
            return StepError::new("the index could not be put back");
        }
        if matches!(ran, Ok(r) if r.forced) {
            return StepError::new("a hook process outlived the interrupt and was killed");
        }
        StepError::unchanged(stop)
    }
}

fn stop_of(run: &std::io::Result<GitRun>) -> StepFailure {
    match run {
        Ok(r) if r.interrupted == Some(Interrupt::TimeLimit) => StepFailure::TimeLimit,
        _ => StepFailure::Unchanged,
    }
}

impl ProtectedStep for CommitStep {
    fn subtype(&self) -> &str {
        OperationId::Commit.as_str()
    }

    fn scope(&self) -> StepScope {
        StepScope {
            worktrees: Vec::new(),
            refs: vec![format!("refs/heads/{}", self.branch)],
        }
    }

    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError> {
        // Both commands are built before `add` touches the index, so a refusal to build the
        // commit can never leave paths staged.
        let op = if self.selection.all_staged {
            UserOp::CommitStaged
        } else {
            UserOp::CommitPaths {
                paths: self.selection.paths.clone(),
            }
        };
        let commit = self.command(&op, Some(&self.message))?;
        if !self.selection.add_first.is_empty() {
            let add = UserOp::AddPaths {
                paths: self.selection.add_first.clone(),
            };
            let ran = self.launch(ctx, &add, None, &self.cancel.clone())?;
            if !matches!(&ran, Ok(r) if r.status.success()) {
                // `add` writes the index whole or not at all; still, put it back.
                return Err(self.stopped(ctx, &ran));
            }
        }
        let ran = run_git(ctx, commit, &self.cancel.clone());
        // The tip decides, whatever `git` printed or how it ended (its output is the hooks'
        // text, never trusted).
        match self.tip()? {
            Some(new) if new != self.old => Ok(StepOutput {
                changed_refs: vec![format!("refs/heads/{}", self.branch)],
                new_commit: Some(self.landed(&new)?),
                ..StepOutput::default()
            }),
            None if self.old != ZERO_OID => Err(StepError::new("branch moved")),
            _ => Err(self.stopped(ctx, &ran)),
        }
    }
}

#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;
