//! The texts of `system.snapshot-failed` (US-GRD-017): one fixed template that names the denied
//! operation, and a hint per cause that asks to repeat that same operation, in the `snapshotreq.`
//! group. None of them mentions the exception or how to lift the protection (ADR-GRD-003 § 3,
//! M-05).

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{Cause, Hook, Reason};

use crate::i18n::Tr;

/// Longest branch name shown in a denial, in characters (M-05).
const MAX_BRANCH_CHARS: usize = 120;

/// The operation a missing recovery point denied. Only chooses the words of the message: the
/// decision is never taken from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeniedOp {
    BranchDelete { branch: Option<String> },
    Rebase { branch: Option<String> },
    Push,
    Other,
}

impl DeniedOp {
    /// The suffix of the per-operation keys (`snapshotreq.hint.start.<op>`, `guardstate.retry.<op>`).
    pub(super) fn key_suffix(&self) -> &'static str {
        match self {
            Self::BranchDelete { .. } => "branch-delete",
            Self::Rebase { .. } => "rebase",
            Self::Push => "push",
            Self::Other => "other",
        }
    }
}

/// The operation a hook was asked about, from its kind, its Git arguments and its standard
/// input (`<old> <new> <ref>` lines for `reference-transaction`). Whatever cannot be told is
/// [`DeniedOp::Other`].
pub(super) fn denied_op(hook: Hook, git_args: &[std::ffi::OsString], input: &[u8]) -> DeniedOp {
    match hook {
        Hook::PrePush => DeniedOp::Push,
        // `pre-rebase <upstream> [<branch>]`: without a branch it rebases the current one.
        Hook::PreRebase => DeniedOp::Rebase {
            branch: git_args.get(1).and_then(|a| a.to_str()).map(shown_branch),
        },
        Hook::ReferenceTransaction => deleted_branch(input)
            .map(|branch| DeniedOp::BranchDelete {
                branch: Some(branch),
            })
            .unwrap_or(DeniedOp::Other),
        Hook::PreCommit | Hook::CommitMsg => DeniedOp::Other,
    }
}

/// The first local branch the transaction deletes (its new value is the zero oid).
fn deleted_branch(input: &[u8]) -> Option<String> {
    let input = std::str::from_utf8(input).ok()?;
    input.lines().find_map(|line| {
        let mut fields = line.trim_end_matches('\r').split(' ');
        let (Some(_old), Some(new), Some(name), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return None;
        };
        let zero = !new.is_empty() && new.bytes().all(|b| b == b'0');
        let branch = name.strip_prefix("refs/heads/")?;
        (zero && !branch.is_empty()).then(|| shown_branch(branch))
    })
}

/// A branch name as the agent reads it: sanitized and bounded (M-05).
fn shown_branch(raw: &str) -> String {
    let clean = Untrusted::new(raw).capped(4 * MAX_BRANCH_CHARS).sanitized();
    let mut out: String = clean.chars().take(MAX_BRANCH_CHARS).collect();
    if clean.chars().count() > MAX_BRANCH_CHARS {
        out.push('…');
    }
    out
}

/// The two lines of a `system.snapshot-failed` reason: what did not run and why, then how to go
/// on, naming that same operation.
///
/// A reason without a snapshot cause (never sent by the daemon) reads as an internal failure: the
/// operation still did not run, and the text says so.
pub(super) fn reason_text(reason: &Reason, denied: DeniedOp, tr: Tr) -> String {
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
    let op = denied.key_suffix();
    let hint = tr.t(
        hint,
        &[
            ("start", &tr.t(&format!("snapshotreq.hint.start.{op}"), &[])),
            ("unobserved", &tr.t("snapshotreq.hint.unobserved", &[])),
            ("again", &tr.t(&format!("snapshotreq.again.{op}"), &[])),
        ],
    );
    let what = match &denied {
        DeniedOp::BranchDelete {
            branch: Some(branch),
        } => tr.t("snapshotreq.deny.branch-delete", &[("branch", branch)]),
        DeniedOp::Rebase {
            branch: Some(branch),
        } => tr.t("snapshotreq.deny.rebase", &[("branch", branch)]),
        DeniedOp::Rebase { branch: None } => tr.t("snapshotreq.deny.rebase-unnamed", &[]),
        DeniedOp::BranchDelete { branch: None } | DeniedOp::Push | DeniedOp::Other => {
            tr.t("snapshotreq.deny.other", &[])
        }
    };
    [
        tr.t(
            "snapshotreq.deny",
            &[("what", &what), ("cause", &tr.t(cause, &[]))],
        ),
        tr.t("snapshotreq.next", &[("hint", &hint)]),
    ]
    .join("\n")
}

/// The key of what a cause of `system.snapshot-failed` means, in business words.
pub(super) fn cause_key(cause: Option<Cause>) -> &'static str {
    match cause {
        Some(Cause::SnapshotTimeLimit) => "snapshotreq.cause.time-limit",
        Some(Cause::SnapshotNoSpace) => "snapshotreq.cause.no-space",
        Some(Cause::SnapshotQuotaExceeded) => "snapshotreq.cause.quota-exceeded",
        Some(Cause::SnapshotDiscarded) => "snapshotreq.cause.discarded",
        Some(Cause::SnapshotNoWorktree) => "snapshotreq.cause.no-worktree",
        Some(Cause::SnapshotUnavailable) => "snapshotreq.cause.unavailable",
        _ => "snapshotreq.cause.internal",
    }
}

/// The key of the rule's name in `raptor guard log`.
pub(super) fn log_rule_key() -> &'static str {
    "snapshotreq.log.rule"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transaction_that_deletes_a_branch_names_it() {
        let oid = "a".repeat(40);
        let zero = "0".repeat(40);
        let input = format!("{oid} {zero} refs/heads/feat-x\n");
        assert_eq!(
            denied_op(Hook::ReferenceTransaction, &[], input.as_bytes()),
            DeniedOp::BranchDelete {
                branch: Some("feat-x".into())
            }
        );
        // An update or a tag deletion is not a branch deletion.
        let update = format!("{zero} {oid} refs/heads/feat-x\n{oid} {zero} refs/tags/v1\n");
        assert_eq!(
            denied_op(Hook::ReferenceTransaction, &[], update.as_bytes()),
            DeniedOp::Other
        );
    }

    #[test]
    fn a_rebase_names_its_branch_only_when_git_passes_it() {
        let args = |a: &[&str]| a.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        assert_eq!(
            denied_op(Hook::PreRebase, &args(&["main", "feat-y"]), b""),
            DeniedOp::Rebase {
                branch: Some("feat-y".into())
            }
        );
        assert_eq!(
            denied_op(Hook::PreRebase, &args(&["main"]), b""),
            DeniedOp::Rebase { branch: None }
        );
        assert_eq!(denied_op(Hook::PrePush, &[], b""), DeniedOp::Push);
    }
}
