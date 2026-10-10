//! Delivery of the interruption notices the recovery at start records (ADR-TMC-003 § 6.5): the
//! next client of a worktree receives each one once.
//!
//! Only interruptions are delivered here. A purge notice starts its grace period when it is
//! first shown (ADR-TMC-007 § 4.2), so it is never handed out by this path.

use std::path::Path;

use gitraptor_api::timemachine::{TimelineOperationKind, TmNotice, TmNoticeKind};

use crate::daemon::{Field, Logger};
use crate::profile::ProfileError;
use crate::timemachine::oplog::{Channel, NoticeKind, OperationKind, Oplog};

/// Takes the interruption notices of `worktree` (and those of the whole repo) that no client
/// received, marks each one delivered on `channel` and returns them, oldest first. The caller
/// holds the oplog's lock for the whole call, so two clients never both receive one.
///
/// Delivery is at least once: a notice whose mark cannot be written, or whose operation cannot be
/// read, is still returned, and `logger` records only its id and the kind of error, never a path.
///
/// # Errors
///
/// The pending notices cannot be read.
pub fn take_interruptions(
    oplog: &mut Oplog,
    worktree: &Path,
    channel: Channel,
    now_ms: i64,
    logger: &Logger,
) -> Result<Vec<TmNotice>, ProfileError> {
    // The same key the operation's scope was written with: the resolved root, as text.
    let key = worktree.to_string_lossy();
    let pending: Vec<_> = oplog
        .pending_notices(Some(&key))?
        .into_iter()
        .filter(|n| n.kind == NoticeKind::Interruption)
        .collect();
    let mut taken = Vec::with_capacity(pending.len());
    for notice in pending {
        // An operation that cannot be read must not lose this notice nor those already marked:
        // it goes out without its kind, and the CLI shows the generic message.
        let operation = match notice.operation_id.as_deref() {
            Some(id) => oplog.operation(id).unwrap_or_else(|e| {
                logger.warn(
                    "tm_notice_operation_unreadable",
                    &[
                        ("notice", Field::id(&notice.notice_id)),
                        ("error", Field::Text(error_kind(&e))),
                    ],
                );
                None
            }),
            None => None,
        };
        if let Err(e) = oplog.mark_notice_delivered(&notice.notice_id, channel, now_ms) {
            logger.warn(
                "tm_notice_unmarked",
                &[
                    ("notice", Field::id(&notice.notice_id)),
                    ("error", Field::Text(error_kind(&e))),
                ],
            );
        }
        taken.push(TmNotice {
            notice_id: notice.notice_id,
            kind: TmNoticeKind::Interruption,
            operation_id: notice.operation_id,
            operation_kind: operation.as_ref().map(|op| wire_kind(op.record.kind)),
            prior_snapshot_id: operation.and_then(|op| op.prior_snapshot),
            recorded_utc_ms: notice.recorded_ms,
        });
    }
    Ok(taken)
}

fn wire_kind(kind: OperationKind) -> TimelineOperationKind {
    match kind {
        OperationKind::Protected => TimelineOperationKind::Protected,
        OperationKind::Undo => TimelineOperationKind::Undo,
        OperationKind::Redo => TimelineOperationKind::Redo,
        OperationKind::Restore => TimelineOperationKind::Restore,
    }
}

/// A fixed word for the log: the error's text may hold a path.
fn error_kind(e: &ProfileError) -> &'static str {
    match e {
        ProfileError::Io(_) => "io",
        ProfileError::Sqlite(_) => "sqlite",
        ProfileError::InvalidWrite(_) => "invalid-write",
        _ => "other",
    }
}

#[cfg(test)]
#[path = "notices_tests.rs"]
mod tests;
