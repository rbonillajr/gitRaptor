//! `raptor redo` (US-TMC-003): takes back the last undo of the worktree the command runs in. The
//! engine decides everything (which undo, permissions, overlap, preconditions); the CLI only names
//! the worktree and renders the answer in the user's language, over the undo's own helpers.

use std::path::Path;
use std::process::ExitCode;

use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{RedoResult, TmConfirmData, TmRejectReason};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::ClientError;
use serde_json::json;

use super::{confirm, notices, overlap, path_reason_key, worktree_root};
use crate::i18n::{Tr, t};
use crate::{engine, error_text, shown};

const CMD: &str = "raptor redo";

/// `raptor redo [--json]`: exit code 0 when the redo is done, 1 when it is refused or fails.
pub(crate) fn run(json_out: bool) -> ExitCode {
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
    notices::show_pending(&mut client, &worktree, CMD);
    let params = json!({ "worktree": worktree.to_string_lossy(), "surface": "cli" });
    let answer = match confirm::call(
        &mut client,
        methods::TM_REDO,
        params,
        !json_out,
        &|data: &TmConfirmData| eprintln!("{CMD}: {}", plan_text(data)),
    ) {
        Ok(answer) => answer,
        Err(code) => return code,
    };
    match answer {
        Ok(value) => {
            if json_out {
                println!("{value}");
                return ExitCode::SUCCESS;
            }
            match serde_json::from_value::<RedoResult>(value) {
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
            // `--json` prints an overlap's data as the daemon sent it.
            if json_out
                && let ClientError::Rpc(e) = &err
                && e.code == code::OPERATION_REJECTED
                && let Some(data) = &e.data
                && serde_json::from_value::<TmConfirmData>(data.clone()).is_ok_and(|d| {
                    matches!(
                        d.reason,
                        TmRejectReason::Overlap | TmRejectReason::OverlapUnverified
                    )
                })
            {
                println!("{data}");
                return ExitCode::FAILURE;
            }
            for line in failure_text(err, &worktree).lines() {
                eprintln!("{CMD}: {line}");
            }
            ExitCode::FAILURE
        }
    }
}

/// What the confirmation is about: the undo the redo takes back and whose work it is.
fn plan_text(data: &TmConfirmData) -> String {
    let id = data
        .undone_operation_id
        .as_deref()
        .map(sanitize)
        .unwrap_or_default();
    let owners = data
        .owners
        .iter()
        .map(crate::events::actor)
        .collect::<Vec<_>>()
        .join(", ");
    t("redo.confirm-plan", &[("id", &id), ("owners", &owners)])
}

fn print_done(result: &RedoResult, worktree: &Path) {
    println!(
        "{CMD}: {}",
        t(
            "redo.done",
            &[
                ("id", &sanitize(&result.redone_undo_id)),
                ("worktree", &shown(worktree)),
            ],
        )
    );
    println!(
        "{CMD}: {}",
        t(
            "redo.prior",
            &[("snapshot", &sanitize(&result.prior_snapshot_id))]
        )
    );
    let kept_total = usize::try_from(result.kept_total).unwrap_or(usize::MAX);
    for line in overlap::kept_lines_of(Tr::current(), &result.kept, kept_total) {
        println!("{CMD}: {line}");
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

/// The message for a failed redo: a rejection with its reason (an overlap with its detail), a
/// scope the engine does not observe, an interruption, or the common errors.
fn failure_text(err: ClientError, worktree: &Path) -> String {
    match err {
        ClientError::Rpc(err) if err.code == code::OPERATION_REJECTED => {
            let data = err
                .data
                .and_then(|d| serde_json::from_value::<TmConfirmData>(d).ok());
            match data {
                Some(d) if d.reason == TmRejectReason::ConfirmationRequired => {
                    t(&confirm::required_key("redo", Some(&d)), &[])
                }
                Some(d)
                    if matches!(
                        d.reason,
                        TmRejectReason::Overlap | TmRejectReason::OverlapUnverified
                    ) =>
                {
                    // The redo's own first line, then the detail as the undo shows it.
                    let mut lines = vec![t(&reason_key(d.reason), &[])];
                    lines.extend(
                        overlap::rejection_lines(Tr::current(), d.reason, d.overlap.as_ref())
                            .into_iter()
                            .skip(1),
                    );
                    lines.join("\n")
                }
                Some(d) => t(&reason_key(d.reason), &[]),
                None => t("redo.reason.unsupported", &[]),
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
            t("redo.interrupted", &[("id", &id)])
        }
        other => error_text(other),
    }
}

/// The message key of a rejection reason: `redo.reason.<wire code>`, one per reason.
fn reason_key(reason: TmRejectReason) -> String {
    let wire = serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unsupported".to_owned());
    format!("redo.reason.{wire}")
}

#[cfg(test)]
#[path = "redo_tests.rs"]
mod tests;
