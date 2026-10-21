//! `raptor guard confirm [path]` (US-GRD-014): shows the plan of confirming the team baseline of
//! the main branch, asks (default "no"), and, when it relaxes, waits for the window in which
//! anyone can cancel it (ADR-GRD-007 § 1). There is no option to skip the plan.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use gitraptor_api::guard::{
    ConfirmPlanView, GuardConfirmParams, GuardConfirmResult, GuardRepoParams, GuardStatus,
    PendingAction, UninstallRefusal,
};
use gitraptor_api::{Untrusted, UntrustedName, methods};
use gitraptor_core::client::{Client, ClientError};

use super::{Answer, WINDOW_POLL, ask, guard_error, now_ms, uninstall_refusal};
use crate::i18n::t;
use crate::{command_path, engine, shown};

const CMD: &str = "raptor guard confirm";

fn name(value: &UntrustedName) -> String {
    value.sanitized()
}

fn list(values: &[Untrusted]) -> String {
    if values.is_empty() {
        return "-".to_owned();
    }
    values
        .iter()
        .map(Untrusted::sanitized)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The plan as lines: the source, the base branch apart from the relaxations, each relaxation.
pub(super) fn plan_lines(repo: &str, plan: &ConfirmPlanView) -> Vec<String> {
    let mut out = vec![t("guard.confirm.header", &[("repo", &repo.to_owned())])];
    out.push(match &plan.source {
        Some(source) => t(
            "guard.confirm.source",
            &[
                ("reference", &name(&source.reference)),
                (
                    "commit",
                    &source.commit.chars().take(12).collect::<String>(),
                ),
            ],
        ),
        None => t("guard.confirm.source-none", &[]),
    });
    let to = name(&plan.base.to);
    out.push(match plan.base.from.as_ref().map(name) {
        Some(from) if from != to => t("guard.confirm.base", &[("from", &from), ("to", &to)]),
        _ => t("guard.confirm.base-same", &[("to", &to)]),
    });
    out.push(t(
        "guard.confirm.relaxations",
        &[("count", &plan.relaxations.len().to_string())],
    ));
    for r in &plan.relaxations {
        out.push(format!(
            "  {}",
            t(
                "guard.confirm.relaxation",
                &[
                    ("key", &r.key.as_str().to_owned()),
                    ("before", &list(&r.before)),
                    ("after", &list(&r.after)),
                ],
            )
        ));
    }
    out
}

/// What an answer without effect says, and its exit code. Once the developer said yes,
/// "nothing to confirm" means this command confirmed nothing (another client did): a failure.
fn outcome(result: &GuardConfirmResult, accepted: bool) -> ExitCode {
    let key = match result {
        GuardConfirmResult::NothingToConfirm if !accepted => {
            println!("{}", t("guard.confirm.nothing", &[]));
            return ExitCode::SUCCESS;
        }
        GuardConfirmResult::NothingToConfirm => "guard.confirm.changed",
        GuardConfirmResult::BaseInvalid => "guard.confirm.base-invalid",
        GuardConfirmResult::FloorUnreadable => "guard.confirm.floor-unreadable",
        GuardConfirmResult::UnshowableRelaxation => "guard.confirm.unshowable",
        GuardConfirmResult::ChangedDuringWindow => "guard.confirm.changed",
        GuardConfirmResult::NotObserved => "guard.confirm.not-observed",
        GuardConfirmResult::Plan { .. }
        | GuardConfirmResult::Applied { .. }
        | GuardConfirmResult::Pending { .. } => "guard.confirm.failed",
    };
    eprintln!("{CMD}: {}", t(key, &[]));
    ExitCode::FAILURE
}

fn refused_text(reason: UninstallRefusal) -> &'static str {
    match reason {
        UninstallRefusal::AlreadyPending => "guard.confirm.already-pending",
        UninstallRefusal::NotTheRequester => "guard.confirm.not-the-requester",
        UninstallRefusal::NotInstalled => "guard.confirm.failed",
        UninstallRefusal::WindowOpen | UninstallRefusal::NoPending => "guard.confirm.cancelled",
    }
}

fn error(path: &std::path::Path, err: ClientError) -> ExitCode {
    match &err {
        ClientError::Rpc(e) if uninstall_refusal(e).is_some() => {
            let reason = uninstall_refusal(e).map_or(UninstallRefusal::NoPending, |d| d.reason);
            eprintln!("{CMD}: {}", t(refused_text(reason), &[]));
            ExitCode::FAILURE
        }
        _ => guard_error(CMD, path, err),
    }
}

fn done(path: &std::path::Path, plan: &ConfirmPlanView) -> ExitCode {
    println!(
        "{}",
        t(
            "guard.confirm.done",
            &[("path", &shown(path)), ("base", &name(&plan.base.to))]
        )
    );
    ExitCode::SUCCESS
}

/// Until the window closes; `false` when someone cancelled it meanwhile.
fn wait_window(
    client: &mut Client,
    repo: &GuardRepoParams,
    pending: &PendingAction,
) -> Result<bool, ClientError> {
    while now_ms() < pending.applies_at_ms {
        std::thread::sleep(
            WINDOW_POLL.min(std::time::Duration::from_millis(
                u64::try_from(pending.applies_at_ms - now_ms())
                    .unwrap_or(0)
                    .max(1),
            )),
        );
        let status = client.call::<_, GuardStatus>(methods::GUARD_STATUS, repo)?;
        if !status
            .pending
            .as_ref()
            .is_some_and(|p| p.action_id == pending.action_id)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `raptor guard confirm [path]`.
pub fn confirm(path: Option<PathBuf>) -> ExitCode {
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let repo = GuardRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    let ask_plan = GuardConfirmParams {
        path: repo.path.clone(),
        accept: None,
        confirm: None,
    };
    let (plan, fingerprint) =
        match client.call::<_, GuardConfirmResult>(methods::GUARD_CONFIRM, &ask_plan) {
            Ok(GuardConfirmResult::Plan { plan, fingerprint }) => (plan, fingerprint),
            Ok(other) => return outcome(&other, false),
            Err(err) => return error(&path, err),
        };
    for line in plan_lines(&shown(&path), &plan) {
        println!("{line}");
    }
    let to = name(&plan.base.to);
    let question = if plan.relaxations.is_empty() {
        t("guard.confirm.question", &[("to", &to)])
    } else {
        t(
            "guard.confirm.question-relaxations",
            &[("to", &to), ("count", &plan.relaxations.len().to_string())],
        )
    };
    let _ = std::io::stdout().flush();
    match ask(&question) {
        Answer::None => {
            eprintln!("{CMD}: {}", t("guard.confirm.no-answer", &[]));
            return ExitCode::FAILURE;
        }
        Answer::No => {
            println!("{}", t("guard.confirm.declined", &[]));
            return ExitCode::SUCCESS;
        }
        Answer::Yes => {}
    }
    let accept = GuardConfirmParams {
        path: repo.path.clone(),
        accept: Some(fingerprint),
        confirm: None,
    };
    let pending = match client.call::<_, GuardConfirmResult>(methods::GUARD_CONFIRM, &accept) {
        Ok(GuardConfirmResult::Applied { plan }) => return done(&path, &plan),
        Ok(GuardConfirmResult::Pending { action, .. }) => action,
        Ok(other) => return outcome(&other, true),
        Err(err) => return error(&path, err),
    };
    println!(
        "{}",
        t(
            "guard.confirm.window",
            &[("seconds", &pending.window_ms.div_ceil(1000).to_string())]
        )
    );
    let _ = std::io::stdout().flush();
    match wait_window(&mut client, &repo, &pending) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("{CMD}: {}", t("guard.confirm.cancelled", &[]));
            return ExitCode::FAILURE;
        }
        Err(err) => return error(&path, err),
    }
    let apply = GuardConfirmParams {
        path: repo.path.clone(),
        accept: None,
        confirm: Some(pending.action_id.clone()),
    };
    loop {
        match client.call::<_, GuardConfirmResult>(methods::GUARD_CONFIRM, &apply) {
            Ok(GuardConfirmResult::Applied { plan }) => return done(&path, &plan),
            Ok(other) => return outcome(&other, true),
            // The daemon's clock decides: wait what it says is left.
            Err(ClientError::Rpc(e))
                if uninstall_refusal(&e)
                    .is_some_and(|d| d.reason == UninstallRefusal::WindowOpen) =>
            {
                let left = uninstall_refusal(&e)
                    .and_then(|d| d.remaining_ms)
                    .unwrap_or(100)
                    .max(1);
                std::thread::sleep(std::time::Duration::from_millis(left));
            }
            Err(err) => return error(&path, err),
        }
    }
}
