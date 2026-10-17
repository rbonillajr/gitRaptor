//! The texts of `system.snapshot-failed` (US-GRD-017): one fixed template and a hint per cause,
//! in the `snapshotreq.` group. None of them mentions the exception or how to lift the
//! protection (ADR-GRD-003 § 3, M-05).

use gitraptor_api::guard::{Cause, Reason};

use crate::i18n::t;

/// The fixed template of a `system.snapshot-failed` reason, with its cause and hint.
///
/// A reason without a snapshot cause (never sent by the daemon) reads as an internal failure: the
/// operation still did not run, and the text says so.
pub(super) fn reason_text(reason: &Reason) -> String {
    let (cause, hint) = match reason.cause {
        Some(Cause::SnapshotTimeLimit) => (
            "snapshotreq.cause.time-limit",
            "snapshotreq.hint.large-file",
        ),
        Some(Cause::SnapshotNoSpace) => ("snapshotreq.cause.no-space", "snapshotreq.hint.no-space"),
        Some(Cause::SnapshotQuotaExceeded) => {
            ("snapshotreq.cause.quota-exceeded", "snapshotreq.hint.retry")
        }
        Some(Cause::SnapshotDiscarded) => ("snapshotreq.cause.discarded", "snapshotreq.hint.retry"),
        Some(Cause::SnapshotNoWorktree) => (
            "snapshotreq.cause.no-worktree",
            "snapshotreq.hint.no-worktree",
        ),
        Some(Cause::SnapshotUnavailable) => (
            "snapshotreq.cause.unavailable",
            "snapshotreq.hint.unavailable",
        ),
        _ => ("snapshotreq.cause.internal", "snapshotreq.hint.retry"),
    };
    let hint = t(
        hint,
        &[
            ("start", &t("snapshotreq.hint.start", &[])),
            ("unobserved", &t("snapshotreq.hint.unobserved", &[])),
        ],
    );
    t(
        "snapshotreq.deny",
        &[("cause", &t(cause, &[])), ("hint", &hint)],
    )
}

/// The key of the rule's name in `raptor guard log`.
pub(super) fn log_rule_key() -> &'static str {
    "snapshotreq.log.rule"
}
