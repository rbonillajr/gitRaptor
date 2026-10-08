//! `raptor undo` (US-TMC-002): undoes the last operation of the worktree the
//! command runs in. The engine decides everything (target, permissions,
//! preconditions); the CLI only names the worktree and renders the answer
//! in the user's language.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{NotRestoredReason, TmRejectReason, TmRejectedData, UndoResult};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::ClientError;
use gitraptor_core::timemachine::restore::KEPT_REF_IN_RECREATED_WORKTREE;
use serde_json::json;

use crate::i18n::t;
use crate::{engine, error_text, shown};

const CMD: &str = "raptor undo";

/// The root of the worktree that contains `dir`: the nearest folder with a
/// `.git` entry. The engine checks that it is the root of a worktree of an
/// observed repo.
pub fn worktree_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok())
        .map(Path::to_path_buf)
}

pub fn run(json_out: bool) -> ExitCode {
    let cwd = std::env::current_dir()
        .and_then(gitraptor_cli::paths::canonicalize)
        .ok();
    let Some(worktree) = cwd.as_deref().and_then(worktree_root) else {
        eprintln!("{CMD}: {}", t("undo.not-in-worktree", &[]));
        return ExitCode::FAILURE;
    };
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let answer: Result<serde_json::Value, ClientError> = client.call(
        methods::TM_UNDO,
        json!({ "worktree": worktree.to_string_lossy(), "surface": "cli" }),
    );
    match answer {
        Ok(value) => {
            if json_out {
                println!("{value}");
                return ExitCode::SUCCESS;
            }
            match serde_json::from_value::<UndoResult>(value) {
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
            eprintln!("{CMD}: {}", failure_text(err, &worktree));
            ExitCode::FAILURE
        }
    }
}

fn print_done(result: &UndoResult, worktree: &Path) {
    let operation = result
        .undone_subtype
        .as_ref()
        .map_or_else(|| t("undo.operation-unnamed", &[]), |s| s.sanitized());
    println!(
        "{CMD}: {}",
        t(
            "undo.done",
            &[
                ("operation", &operation),
                ("id", &result.undone_operation_id),
                ("worktree", &shown(worktree)),
            ],
        )
    );
    println!(
        "{CMD}: {}",
        t("undo.saved", &[("snapshot", &result.prior_snapshot_id)])
    );
    // A branch the undo of a restore left as it is: only a worktree that
    // restore recreated has it out. The ref name is untrusted repo text.
    for kept in result.warnings.iter().filter_map(|w| {
        w.strip_prefix(KEPT_REF_IN_RECREATED_WORKTREE)?
            .strip_prefix(':')
    }) {
        println!("{CMD}: {}", t("undo.kept-ref", &[("ref", &sanitize(kept))]));
    }
    for path in &result.not_restored {
        let reason = t(path_reason_key(path.reason), &[]);
        println!(
            "{CMD}: {}",
            t(
                "undo.not-restored",
                &[("reason", &reason), ("path", &path.path.sanitized())],
            )
        );
    }
}

/// The message for a failed undo: a rejection with its reason, a scope the
/// engine does not observe, an interruption, or the common errors.
fn failure_text(err: ClientError, worktree: &Path) -> String {
    match err {
        ClientError::Rpc(err) if err.code == code::OPERATION_REJECTED => {
            let reason = err
                .data
                .and_then(|d| serde_json::from_value::<TmRejectedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(reason) => t(reason_key(reason), &[]),
                None => t("undo.reason.unsupported", &[]),
            }
        }
        ClientError::Rpc(err) if err.code == code::SCOPE_REFUSED => {
            t("undo.not-observed", &[("worktree", &shown(worktree))])
        }
        ClientError::Rpc(err) if err.code == code::OPERATION_FAILED => {
            let id = err
                .data
                .as_ref()
                .and_then(|d| d.get("operation_id"))
                .and_then(|v| v.as_str())
                .map(sanitize)
                .unwrap_or_default();
            t("undo.interrupted", &[("id", &id)])
        }
        other => error_text(other),
    }
}

fn reason_key(reason: TmRejectReason) -> &'static str {
    match reason {
        TmRejectReason::NothingToUndo => "undo.reason.nothing-to-undo",
        TmRejectReason::RawGitNotCovered => "undo.reason.raw-git-not-covered",
        TmRejectReason::TargetUnavailable => "undo.reason.target-unavailable",
        TmRejectReason::OtherActor => "undo.reason.other-actor",
        TmRejectReason::ConfirmationRequired => "undo.reason.confirmation-required",
        TmRejectReason::GitOperationInProgress => "undo.reason.git-operation-in-progress",
        TmRejectReason::GitBusy => "undo.reason.git-busy",
        TmRejectReason::RepoBusy => "undo.reason.repo-busy",
        TmRejectReason::RepoUntrusted => "undo.reason.repo-untrusted",
        TmRejectReason::WorktreeUnavailable => "undo.reason.worktree-unavailable",
        TmRejectReason::InvalidSnapshot => "undo.reason.invalid-snapshot",
        TmRejectReason::HostileTree => "undo.reason.hostile-tree",
        TmRejectReason::RefMoved => "undo.reason.ref-moved",
        TmRejectReason::RefInUse => "undo.reason.ref-in-use",
        TmRejectReason::Unsupported => "undo.reason.unsupported",
        TmRejectReason::GitUnavailable => "undo.reason.git-unavailable",
    }
}

fn path_reason_key(reason: NotRestoredReason) -> &'static str {
    match reason {
        NotRestoredReason::Overlap => "undo.path.overlap",
        NotRestoredReason::NotGuaranteed => "undo.path.not-guaranteed",
        NotRestoredReason::Blocked => "undo.path.blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::has_key;

    /// Every rejection reason has its message in English and Spanish.
    #[test]
    fn every_rejection_has_a_message() {
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
            // The key is the wire code: one place to keep in step.
            let wire = serde_json::to_value(reason).unwrap();
            assert_eq!(key, format!("undo.reason.{}", wire.as_str().unwrap()));
        }
        for reason in [
            NotRestoredReason::Overlap,
            NotRestoredReason::NotGuaranteed,
            NotRestoredReason::Blocked,
        ] {
            assert!(has_key(path_reason_key(reason)));
        }
        for key in [
            "undo.done",
            "undo.saved",
            "undo.not-restored",
            "undo.kept-ref",
            "undo.operation-unnamed",
            "undo.not-in-worktree",
            "undo.not-observed",
            "undo.interrupted",
        ] {
            assert!(has_key(key), "{key}");
        }
    }

    #[test]
    fn the_worktree_is_found_from_a_subfolder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert_eq!(worktree_root(&root.join("src/deep")), Some(root.clone()));
        assert_eq!(worktree_root(&root), Some(root));
        assert_eq!(worktree_root(tmp.path()), None);
    }
}
