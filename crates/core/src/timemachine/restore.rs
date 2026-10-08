//! `restore` of a worktree to a point of the timeline (US-TMC-009,
//! BR-TMC-WF-003, ADR-TMC-004 § 1, ADR-TMC-005).
//!
//! A protected operation of kind `restore` on the path of `undo_last`: the
//! repo lock, the engine settled, the plan, the base permission rule over
//! every actor whose work the restore takes back, the Git preconditions, the
//! intent, its own prior snapshot and the applier with a bounded ref scope.
//! It stays in the worktree's stack, so `undo` takes it back.
//!
//! Worktrees the point had and that are gone are recreated without
//! checkout. Each one is recorded in the restore's `warnings` as
//! `recreated-worktree:<key>` ([`recreated_worktree_warning`]), with the
//! key the point's meta gives it, so an undo of the restore never refuses
//! a branch only that worktree has out: it keeps it and says so with
//! [`KEPT_REF_IN_RECREATED_WORKTREE`].

use std::path::PathBuf;

use gitraptor_api::timemachine::{PriorFailure, TmRejectReason};

use super::apply::ApplyReport;
use super::oplog::{Channel, Requester};
use super::undo::{TmRepoHandle, UndoEnv};
use crate::channel::requester::Who;

/// Warning prefix a restore records for each worktree it recreated,
/// followed by `:` and the worktree's key in the point's meta.
pub const RECREATED_WORKTREE_WARNING: &str = "recreated-worktree";

/// Warning code an undo of a restore records when it leaves a branch as it
/// is because only a worktree that restore recreated has it checked out.
pub const KEPT_REF_IN_RECREATED_WORKTREE: &str = "kept-ref-in-recreated-worktree";

/// The warning a restore records for the recreated worktree `key`.
pub fn recreated_worktree_warning(key: &str) -> String {
    format!("{RECREATED_WORKTREE_WARNING}:{key}")
}

/// A finished restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreDone {
    pub operation_id: String,
    pub prior_snapshot_id: String,
    pub target_snapshot_id: String,
    /// Existing worktrees brought to the point, the requested one first.
    pub worktrees: Vec<PathBuf>,
    /// Worktrees of the point that were gone and were recreated.
    pub recreated: Vec<PathBuf>,
    /// Full ref names moved to the point's values.
    pub refs: Vec<String>,
    /// Local branches that exist now and not at the point: left as they are.
    pub kept_branches: Vec<String>,
    /// Branches of the point that are gone and were not brought back,
    /// since nothing done from the plan's worktrees touched them.
    pub not_returned_branches: Vec<String>,
    pub report: ApplyReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// No snapshot with that id in this repo's oplog. Not recorded.
    NotFound,
    /// Nothing changed; `operation_id` is the rejected request in the oplog.
    Rejected {
        reason: TmRejectReason,
        operation_id: Option<String>,
    },
    /// The restore's prior snapshot failed: it did not run.
    Prior {
        reason: PriorFailure,
        operation_id: Option<String>,
    },
    /// Stopped half-way: the restore is `interrupted` and `undo` returns to
    /// its prior snapshot.
    Interrupted {
        operation_id: String,
        message: String,
    },
    /// The oplog or the write layer could not be used: nothing ran.
    Internal(String),
}

/// The base permission rule (ADR-TMC-005 § 2) over every actor whose work
/// the restore takes back. `Ok` with no owners. Any owner that yields
/// `OtherActor` makes the result `OtherActor`; otherwise any
/// `ConfirmationRequired` makes it `ConfirmationRequired`. Each owner is
/// checked with [`super::undo::permission`].
pub fn restore_permission(
    _who: &Requester,
    _channel: Channel,
    _owners: &[Requester],
) -> Result<(), TmRejectReason> {
    Err(TmRejectReason::Unsupported)
}

/// Restores `repo.repo.worktree` to snapshot `snapshot_id` for `who`
/// through `channel`.
///
/// # Errors
///
/// [`RestoreError::NotFound`] for an id this repo's oplog does not have;
/// otherwise as [`RestoreError`] describes.
pub fn restore_to(
    _repo: &TmRepoHandle,
    _snapshot_id: &str,
    _who: &Who,
    _channel: Channel,
    _env: &UndoEnv<'_>,
) -> Result<RestoreDone, RestoreError> {
    Err(RestoreError::Internal("not implemented".into()))
}
