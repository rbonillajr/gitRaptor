//! Redo: take back the last undo of a scope, returning its worktree to the point taken right
//! before that undo, over the undo's own pieces (lock, prior, confirmation, overlap, applier).

// Not wired to the channel yet: `timemachine.redo` still answers `NOT_IMPLEMENTED`.
#![allow(dead_code)]

use gitraptor_api::timemachine::{PriorFailure, TmRejectReason};

use super::apply::ApplyReport;
use super::confirm::Confirmation;
use super::oplog::{Channel, OperationView, Oplog, Requester};
use super::overlap::OverlapReport;
use super::undo::{RawSide, TmRepoHandle, UndoEnv};
use crate::channel::requester::Who;

/// A redo that ran to the end.
#[derive(Debug)]
pub struct RedoDone {
    /// The redo itself, as recorded.
    pub operation_id: String,
    /// The point taken right before the redo.
    pub prior_snapshot_id: String,
    /// The undo this redo took back.
    pub redone_undo_id: String,
    /// The undo's prior: the state the worktree returned to.
    pub target_snapshot_id: String,
    pub report: ApplyReport,
    pub warnings: Vec<String>,
}

/// Why a redo did not run to the end.
#[derive(Debug)]
pub enum RedoError {
    /// Nothing changed; `operation_id` is the rejected request in the oplog.
    Rejected {
        reason: TmRejectReason,
        operation_id: Option<String>,
    },
    /// Another actor changed, after the undo, something the redo would overwrite. Nothing
    /// changed.
    Overlap {
        operation_id: Option<String>,
        // Boxed: the report would make every `Result<_, RedoError>` large.
        report: Box<OverlapReport>,
    },
    /// The redo's prior snapshot failed: it did not run.
    Prior {
        reason: PriorFailure,
        operation_id: Option<String>,
    },
    /// Stopped half-way: the redo is `interrupted`.
    Interrupted {
        operation_id: String,
        message: String,
    },
    /// The oplog or the write layer could not be used: nothing ran.
    Internal(String),
}

/// Redoes the last undo of the requested scope.
///
/// # Errors
///
/// Every [`RedoError`]; nothing changed unless it is `Interrupted`.
pub fn redo_last(
    _repo: &TmRepoHandle,
    _who: &Who,
    _channel: Channel,
    _confirm: &Confirmation<'_>,
    _env: &UndoEnv<'_>,
) -> Result<RedoDone, RedoError> {
    Err(RedoError::Internal("not implemented".to_owned()))
}

/// Whose work the redo would overwrite: the owners of what the undo took back.
pub(crate) fn redo_owners(_undo: &OperationView, _raw: &RawSide, _oplog: &Oplog) -> Vec<Requester> {
    Vec::new()
}

#[cfg(test)]
#[path = "redo_tests.rs"]
mod tests;
