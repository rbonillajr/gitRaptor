//! `raptor restore`: return the worktree to a point of the timeline. The
//! engine decides everything (target, scope, permissions, preconditions);
//! this file names the worktree and renders the answer in the user's
//! language.

use std::path::Path;
use std::process::ExitCode;

use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{NotRestoredReason, RestoreResult, TmRejectReason, TmRejectedData};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::ClientError;
use serde_json::json;

use super::Global;
use crate::i18n::t;
use crate::{engine, error_text, shown, undo};

const CMD: &str = "raptor restore";

/// Return the worktree you are in to a point of the timeline; the state right before is saved.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    /// The id of the point, as `raptor timeline` shows it.
    snapshot_id: String,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        let cwd = std::env::current_dir()
            .and_then(gitraptor_cli::paths::canonicalize)
            .ok();
        let Some(worktree) = cwd.as_deref().and_then(undo::worktree_root) else {
            eprintln!("{CMD}: {}", t("restore.not-in-worktree", &[]));
            return ExitCode::FAILURE;
        };
        let mut client = match engine(CMD) {
            Ok(client) => client,
            Err(code) => return code,
        };
        let answer: Result<serde_json::Value, ClientError> = client.call(
            methods::TM_RESTORE,
            json!({
                "worktree": worktree.to_string_lossy(),
                "snapshot_id": self.snapshot_id,
                "surface": "cli",
            }),
        );
        match answer {
            Ok(value) => {
                if self.json {
                    println!("{value}");
                    return ExitCode::SUCCESS;
                }
                match serde_json::from_value::<RestoreResult>(value) {
                    Ok(result) => {
                        print_done(&result, &worktree);
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("{CMD}: {}", sanitize(&err.to_string()));
                        ExitCode::FAILURE
                    }
                }
            }
            Err(err) => {
                eprintln!("{CMD}: {}", failure_text(err, &worktree, &self.snapshot_id));
                ExitCode::FAILURE
            }
        }
    }
}

fn print_done(result: &RestoreResult, worktree: &Path) {
    println!(
        "{CMD}: {}",
        t(
            "restore.done",
            &[
                ("worktree", &shown(worktree)),
                ("id", &sanitize(&result.target_snapshot_id)),
            ],
        )
    );
    println!(
        "{CMD}: {}",
        t(
            "restore.saved",
            &[("snapshot", &sanitize(&result.prior_snapshot_id))]
        )
    );
    for root in &result.recreated {
        println!(
            "{CMD}: {}",
            t("restore.recreated", &[("worktree", &root.sanitized())])
        );
    }
    for branch in &result.kept_branches {
        println!(
            "{CMD}: {}",
            t("restore.kept-branch", &[("branch", &branch.sanitized())])
        );
    }
    for branch in &result.not_returned_branches {
        println!(
            "{CMD}: {}",
            t(
                "restore.branch-not-returned",
                &[("branch", &branch.sanitized())]
            )
        );
    }
    for path in &result.not_restored {
        let reason = t(path_reason_key(path.reason), &[]);
        println!(
            "{CMD}: {}",
            t(
                "restore.not-restored",
                &[("reason", &reason), ("path", &path.path.sanitized())],
            )
        );
    }
}

/// The message for a failed restore: a rejection with its reason, an unknown
/// or malformed point, a scope the engine does not observe, an interruption,
/// or the common errors.
fn failure_text(err: ClientError, worktree: &Path, id: &str) -> String {
    match err {
        ClientError::Rpc(err) if err.code == code::OPERATION_REJECTED => {
            let reason = err
                .data
                .and_then(|d| serde_json::from_value::<TmRejectedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(reason) => t(reason_key(reason), &[]),
                None => t("restore.reason.unsupported", &[]),
            }
        }
        ClientError::Rpc(err) if err.code == code::NOT_FOUND => {
            t("restore.not-found", &[("id", &sanitize(id))])
        }
        ClientError::Rpc(err) if err.code == code::INVALID_PARAMS => {
            t("restore.invalid-id", &[])
        }
        ClientError::Rpc(err) if err.code == code::SCOPE_REFUSED => {
            t("restore.not-observed", &[("worktree", &shown(worktree))])
        }
        ClientError::Rpc(err) if err.code == code::OPERATION_FAILED => {
            let id = err
                .data
                .as_ref()
                .and_then(|d| d.get("operation_id"))
                .and_then(|v| v.as_str())
                .map(sanitize)
                .unwrap_or_default();
            t("restore.interrupted", &[("id", &id)])
        }
        ClientError::Rpc(err) if err.code == code::NOT_IMPLEMENTED => {
            t("restore.restart-engine", &[])
        }
        other => error_text(other),
    }
}

/// The per-path reasons read the same as in `raptor undo`: same applier, same causes.
fn path_reason_key(reason: NotRestoredReason) -> &'static str {
    match reason {
        NotRestoredReason::Overlap => "undo.path.overlap",
        NotRestoredReason::NotGuaranteed => "undo.path.not-guaranteed",
        NotRestoredReason::Blocked => "undo.path.blocked",
    }
}

/// Both keys of the confirmation exist on every platform; the platform picks
/// which one is told.
fn reason_key(reason: TmRejectReason) -> &'static str {
    match reason {
        TmRejectReason::NothingToUndo => "restore.reason.nothing-to-undo",
        TmRejectReason::RawGitNotCovered => "restore.reason.raw-git-not-covered",
        TmRejectReason::TargetUnavailable => "restore.reason.target-unavailable",
        TmRejectReason::OtherActor => "restore.reason.other-actor",
        TmRejectReason::ConfirmationRequired => {
            if cfg!(windows) {
                "restore.reason.confirmation-unavailable-windows"
            } else {
                "restore.reason.confirmation-required"
            }
        }
        TmRejectReason::ConfirmationUnavailable => "restore.reason.confirmation-unavailable-windows",
        TmRejectReason::ChallengeInvalid => "restore.reason.challenge-invalid",
        TmRejectReason::GitOperationInProgress => "restore.reason.git-operation-in-progress",
        TmRejectReason::GitBusy => "restore.reason.git-busy",
        TmRejectReason::RepoBusy => "restore.reason.repo-busy",
        TmRejectReason::RepoUntrusted => "restore.reason.repo-untrusted",
        TmRejectReason::WorktreeUnavailable => "restore.reason.worktree-unavailable",
        TmRejectReason::InvalidSnapshot => "restore.reason.invalid-snapshot",
        TmRejectReason::HostileTree => "restore.reason.hostile-tree",
        TmRejectReason::RefMoved => "restore.reason.ref-moved",
        TmRejectReason::RefInUse => "restore.reason.ref-in-use",
        TmRejectReason::Unsupported => "restore.reason.unsupported",
        TmRejectReason::GitUnavailable => "restore.reason.git-unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::has_key;

    #[test]
    fn every_rejection_and_message_exists_in_both_languages() {
        for reason in [
            TmRejectReason::NothingToUndo,
            TmRejectReason::RawGitNotCovered,
            TmRejectReason::TargetUnavailable,
            TmRejectReason::OtherActor,
            TmRejectReason::ConfirmationRequired,
            TmRejectReason::GitOperationInProgress,
            TmRejectReason::GitBusy,
            TmRejectReason::RepoBusy,
            TmRejectReason::RepoUntrusted,
            TmRejectReason::WorktreeUnavailable,
            TmRejectReason::InvalidSnapshot,
            TmRejectReason::HostileTree,
            TmRejectReason::RefMoved,
            TmRejectReason::RefInUse,
            TmRejectReason::Unsupported,
            TmRejectReason::GitUnavailable,
        ] {
            let key = reason_key(reason);
            assert!(has_key(key), "{key}");
        }
        for reason in [
            NotRestoredReason::Overlap,
            NotRestoredReason::NotGuaranteed,
            NotRestoredReason::Blocked,
        ] {
            assert!(has_key(path_reason_key(reason)));
        }
        for key in [
            "restore.reason.confirmation-required",
            "restore.reason.confirmation-unavailable-windows",
            "restore.done",
            "restore.saved",
            "restore.recreated",
            "restore.not-restored",
            "restore.kept-branch",
            "restore.branch-not-returned",
            "restore.not-in-worktree",
            "restore.not-observed",
            "restore.not-found",
            "restore.invalid-id",
            "restore.interrupted",
            "restore.restart-engine",
        ] {
            assert!(has_key(key), "{key}");
        }
    }
}
