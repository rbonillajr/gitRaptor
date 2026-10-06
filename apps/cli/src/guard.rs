//! `raptor guard` and `raptor hook` (US-GRD-001).
//!
//! - `raptor guard install`: explains what protecting a repo means and asks; the install is a
//!   reserved command decided by the daemon (ADR-GRD-007 § 1, BR-AUTH-002).
//! - `raptor guard status`: the protection status of a repo.
//! - `raptor hook`: what a Guardrails dispatcher starts. Prints one fixed template per reason
//!   code with labelled, sanitized parameters (M-05) and exits 0 (allow) or 1 (deny).

use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    Cause, Decision, Effect, GuardPlan, GuardRejectedData, GuardRepoParams, GuardStatus,
    InstallBlocker, NotPreventable, Param, ParamKind, Permission, ProtectionState, Reason,
    RefBackend, Rule,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_core::client::ClientError;
use gitraptor_core::guardrails::hook::{self, Degraded, HookArgs, HookEnv};

use crate::i18n::t;
use crate::{command_path, engine, error_text, refusal_text, repo_error, shown};

/// Longest parameter shown to the agent, in characters (M-05).
const MAX_PARAM_CHARS: usize = 120;

/// Hook input bound: the whole standard input is read, each line is bounded by the parser.
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;

fn param(value: &Untrusted) -> String {
    let clean = value.capped(4 * MAX_PARAM_CHARS).sanitized();
    let mut out: String = clean.chars().take(MAX_PARAM_CHARS).collect();
    if clean.chars().count() > MAX_PARAM_CHARS {
        out.push('…');
    }
    format!("«{out}»")
}

fn find(params: &[Param], kind: ParamKind) -> String {
    params
        .iter()
        .find(|p| p.kind == kind)
        .map(|p| param(&p.value))
        .unwrap_or_else(|| "«»".into())
}

/// The fixed template of one reason.
fn reason_text(reason: &Reason) -> String {
    let p = &reason.params;
    match (reason.rule, reason.cause) {
        (Rule::MinimumForcePush, Some(Cause::ShallowHistory)) => t(
            "guard.deny.shallow",
            &[("branch", &find(p, ParamKind::Branch))],
        ),
        (Rule::MinimumForcePush, _) => t(
            "guard.deny.force-push",
            &[
                ("branch", &find(p, ParamKind::Branch)),
                ("remote", &find(p, ParamKind::Remote)),
            ],
        ),
        (Rule::MinimumBaseBranchDelete, Some(Cause::Alias)) => t(
            "guard.deny.alias",
            &[
                ("ref", &find(p, ParamKind::Ref)),
                ("base", &find(p, ParamKind::Base)),
            ],
        ),
        (Rule::MinimumBaseBranchDelete, Some(Cause::RenameOntoBase)) => t(
            "guard.deny.rename-onto-base",
            &[
                ("base", &find(p, ParamKind::Base)),
                ("branch", &find(p, ParamKind::Branch)),
                ("oid", &find(p, ParamKind::Oid)),
            ],
        ),
        (Rule::MinimumBaseBranchDelete, _) => t(
            "guard.deny.base-delete",
            &[("branch", &find(p, ParamKind::Branch))],
        ),
        (Rule::Degraded, cause) => t(
            "guard.degraded",
            &[(
                "reason",
                &t(
                    if cause == Some(Cause::InstanceMismatch) {
                        "guard.degraded.instance"
                    } else {
                        "guard.degraded.unreachable"
                    },
                    &[],
                ),
            )],
        ),
        (Rule::ChannelNotAuthentic, _) => t("guard.deny.channel", &[]),
        (Rule::RepoMismatch, _) => t("guard.deny.repo-mismatch", &[]),
        (Rule::InputRejected, _) => t("guard.deny.input", &[]),
        (Rule::InternalError, _) => t("guard.deny.internal", &[]),
    }
}

/// The lines a decision prints on standard error.
pub fn decision_lines(decision: &Decision) -> Vec<String> {
    if decision.applied_effect == Effect::Allow {
        return Vec::new();
    }
    decision.reasons.iter().map(reason_text).collect()
}

/// `raptor hook <constants> -- <git args>`: never prompts, never starts the daemon.
pub fn hook(args: &[OsString]) -> ExitCode {
    let Some(args) = HookArgs::parse(args) else {
        eprintln!("{}", t("guard.deny.input", &[]));
        return ExitCode::FAILURE;
    };
    let mut input = Vec::new();
    // One byte past the bound means the input was cut: never decide on a prefix.
    let read = std::io::stdin()
        .lock()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut input);
    if read.is_err() || input.len() as u64 > MAX_INPUT_BYTES {
        eprintln!("{}", t("guard.deny.input", &[]));
        return ExitCode::FAILURE;
    }
    let env = HookEnv {
        git_dir: std::env::var_os("GIT_DIR").map(PathBuf::from),
        cwd: std::env::current_dir().unwrap_or_default(),
    };
    let outcome = hook::run(&args, &env, &input);
    if let Some(cause) = outcome.degraded
        && outcome.allowed()
    {
        let reason = if cause == Degraded::InstanceMismatch {
            "guard.degraded.instance"
        } else {
            "guard.degraded.unreachable"
        };
        eprintln!("{}", t("guard.degraded", &[("reason", &t(reason, &[]))]));
    }
    if let Some(decision) = &outcome.decision {
        for line in decision_lines(decision) {
            eprintln!("{line}");
        }
    }
    if outcome.allowed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn not_preventable_text(n: NotPreventable) -> String {
    t(
        match n {
            NotPreventable::ResetHard => "guard.np.reset-hard",
            NotPreventable::Merge => "guard.np.merge",
            NotPreventable::RemoveWorktree => "guard.np.remove-worktree",
            NotPreventable::CreateWorktreeUnrecognized => "guard.np.create-worktree",
            NotPreventable::RenameBaseReftable => "guard.np.rename-base-reftable",
            NotPreventable::VoluntarySkips => "guard.np.voluntary-skips",
        },
        &[],
    )
}

fn blocker_text(b: InstallBlocker) -> String {
    t(
        match b {
            InstallBlocker::PriorHooks => "guard.blocker.prior-hooks",
            InstallBlocker::WorktreeConfig => "guard.blocker.worktree-config",
            InstallBlocker::IncludeDefinesHooksPath => "guard.blocker.include",
            InstallBlocker::IncludeIfOnbranch => "guard.blocker.onbranch",
            InstallBlocker::NotRepresentable => "guard.blocker.not-representable",
            InstallBlocker::OrphanFolder => "guard.blocker.orphan-folder",
            InstallBlocker::AlreadyInstalled => "guard.blocker.already-installed",
            InstallBlocker::DispatcherMissing => "guard.blocker.dispatcher-missing",
            InstallBlocker::Bare => "guard.blocker.bare",
            InstallBlocker::PlatformUnsupported => "guard.unsupported-platform",
        },
        &[],
    )
}

fn names(list: &[Untrusted]) -> String {
    list.iter()
        .map(|b| b.sanitized())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The explanation shown before asking (BR-AUTH-002, Q-GRD-28, Q-GRD-30).
fn explain(plan: &GuardPlan) -> String {
    let common = plan.common_dir.sanitized();
    let hooks_dir = plan.hooks_dir.sanitized();
    let mut out = vec![
        t("guard.install.header", &[("repo", &common)]),
        String::new(),
        t(
            "guard.install.what",
            &[
                (
                    "hooks",
                    &plan
                        .hooks
                        .iter()
                        .map(|h| h.git_name())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                ("dir", &hooks_dir),
            ],
        ),
        t("guard.install.where", &[("repo", &common)]),
        t("guard.install.scope", &[("count", &plan.worktrees.len())]),
        t("guard.install.why", &[]),
        match &plan.confirms_base {
            Some(base) => t("guard.install.base", &[("base", &base.sanitized())]),
            None => t(
                "guard.install.base-unconfirmed",
                &[("bases", &names(&plan.protected_bases))],
            ),
        },
        t("guard.install.cannot-prevent", &[]),
    ];
    for n in &plan.not_preventable {
        out.push(format!("  - {}", not_preventable_text(*n)));
    }
    if plan.backend == RefBackend::Reftable {
        out.push(t("guard.install.reftable", &[]));
    }
    out.push(t("guard.install.cost", &[]));
    out.push(t("guard.install.unavailable", &[]));
    out.push(t("guard.install.revert", &[]));
    out.push(format!(
        "  git config --file {} --unset core.hooksPath",
        shell_quote(&format!("{common}/config"))
    ));
    out.push(t(
        "guard.install.revert-folder",
        &[("dir", &format!("{common}/gitraptor"))],
    ));
    out.push(t("guard.install.if-not", &[]));
    out.join("\n")
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The developer's answer.
enum Answer {
    Yes,
    No,
    /// No terminal, end of input or an empty line: nothing is recorded.
    None,
}

fn ask(question: &str) -> Answer {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        eprintln!("raptor: {}", t("common.confirm-needs-terminal", &[]));
        return Answer::None;
    }
    eprint!("{question} ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    match stdin.lock().read_line(&mut answer) {
        Ok(0) | Err(_) => Answer::None,
        Ok(_) => match answer.trim().to_lowercase().as_str() {
            "y" | "yes" | "s" | "si" | "sí" => Answer::Yes,
            "n" | "no" => Answer::No,
            _ => Answer::None,
        },
    }
}

fn status_lines(status: &GuardStatus) -> Vec<String> {
    let mut out = vec![match status.state {
        ProtectionState::Unprotected => t("guard.state.unprotected", &[]),
        ProtectionState::HooksOnly => t("guard.state.hooks-only", &[]),
    }];
    if status.state == ProtectionState::HooksOnly {
        out.push(if status.base_confirmed {
            t(
                "guard.status.base-confirmed",
                &[("bases", &names(&status.protected_bases))],
            )
        } else {
            t(
                "guard.status.base-unconfirmed",
                &[("bases", &names(&status.protected_bases))],
            )
        });
    }
    out.push(match status.permission {
        Permission::NotAsked => t("guard.permission.not-asked", &[]),
        Permission::Granted => t("guard.permission.granted", &[]),
        Permission::Denied => t("guard.permission.denied", &[]),
    });
    for b in &status.last_refusal {
        out.push(format!("  - {}", blocker_text(*b)));
    }
    out
}

fn rejected_text(err: &gitraptor_api::rpc::ErrorObject) -> Vec<String> {
    err.data
        .clone()
        .and_then(|d| serde_json::from_value::<GuardRejectedData>(d).ok())
        .map(|d| d.blockers.into_iter().map(blocker_text).collect())
        .unwrap_or_default()
}

fn guard_error(command: &str, path: &std::path::Path, err: ClientError) -> ExitCode {
    match err {
        ClientError::Rpc(err) if err.code == code::RESERVED_REFUSED => {
            eprintln!("{command}: {}", refusal_text(&err, "guard.refused-agent"));
            ExitCode::FAILURE
        }
        ClientError::Rpc(err) if err.code == code::GUARD_REJECTED => {
            eprintln!("{command}: {}", t("guard.install.refused", &[]));
            for line in rejected_text(&err) {
                eprintln!("  - {line}");
            }
            ExitCode::FAILURE
        }
        ClientError::Rpc(ref e) if e.code == code::REPO_REJECTED => repo_error(command, path, err),
        other => {
            eprintln!("{command}: {}", error_text(other));
            ExitCode::FAILURE
        }
    }
}

/// `raptor guard install [path] [--yes]` (US-GRD-001 E1 and E5).
pub fn install(path: Option<PathBuf>, yes: bool) -> ExitCode {
    const CMD: &str = "raptor guard install";
    if cfg!(windows) {
        eprintln!("{CMD}: {}", t("guard.unsupported-platform", &[]));
        return ExitCode::FAILURE;
    }
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = GuardRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    let plan = match client.call::<_, GuardPlan>(methods::GUARD_PLAN, &params) {
        Ok(plan) => plan,
        Err(err) => return guard_error(CMD, &path, err),
    };
    if plan.blockers.contains(&InstallBlocker::AlreadyInstalled) {
        println!("{}", t("guard.install.already", &[("path", &shown(&path))]));
        return ExitCode::SUCCESS;
    }
    println!("{}", explain(&plan));
    if !plan.blockers.is_empty() {
        // Nothing to ask: the daemon checks again, records the refused attempt with its
        // causes (ADR-GRD-001 § 4 paso 1) and installs nothing.
        return match client.call::<_, GuardStatus>(methods::GUARD_INSTALL, &params) {
            Err(err) => guard_error(CMD, &path, err),
            Ok(_) => {
                println!("{}", t("guard.install.done", &[("path", &shown(&path))]));
                ExitCode::SUCCESS
            }
        };
    }
    let answer = if yes {
        Answer::Yes
    } else {
        ask(&t("guard.install.confirm", &[]))
    };
    match answer {
        Answer::None => {
            eprintln!("{CMD}: {}", t("guard.install.no-answer", &[]));
            ExitCode::FAILURE
        }
        Answer::No => match client.call::<_, GuardStatus>(methods::GUARD_DECLINE, &params) {
            Ok(_) => {
                println!("{}", t("guard.install.declined", &[]));
                ExitCode::SUCCESS
            }
            Err(err) => guard_error(CMD, &path, err),
        },
        Answer::Yes => match client.call::<_, GuardStatus>(methods::GUARD_INSTALL, &params) {
            Ok(status) => {
                println!("{}", t("guard.install.done", &[("path", &shown(&path))]));
                for line in status_lines(&status) {
                    println!("{line}");
                }
                ExitCode::SUCCESS
            }
            Err(err) => guard_error(CMD, &path, err),
        },
    }
}

/// `raptor guard status [path] [--json]`.
pub fn status(path: Option<PathBuf>, json: bool) -> ExitCode {
    const CMD: &str = "raptor guard status";
    if cfg!(windows) {
        eprintln!("{CMD}: {}", t("guard.unsupported-platform", &[]));
        return ExitCode::FAILURE;
    }
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = GuardRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    match client.call::<_, GuardStatus>(methods::GUARD_STATUS, &params) {
        Ok(status) if json => {
            println!("{}", serde_json::to_string(&status).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(status) => {
            for line in status_lines(&status) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => guard_error(CMD, &path, err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::has_key;

    #[test]
    fn every_code_has_a_message_in_both_languages() {
        for key in [
            "guard.deny.force-push",
            "guard.deny.base-delete",
            "guard.deny.alias",
            "guard.deny.shallow",
            "guard.deny.rename-onto-base",
            "guard.deny.channel",
            "guard.deny.repo-mismatch",
            "guard.deny.input",
            "guard.deny.internal",
            "guard.degraded",
            "guard.degraded.instance",
            "guard.degraded.unreachable",
            "guard.refused-agent",
            "guard.unsupported-platform",
        ] {
            assert!(has_key(key), "{key}");
        }
    }

    #[test]
    fn parameters_are_labelled_sanitized_and_bounded() {
        // M-05, ADR-GRD-003 Validación 10.
        let hostile = Untrusted::new(format!(
            "feat\u{1b}]52;c;ZXZpbA==\u{7}\u{202e}x\u{2028}y{}",
            "z".repeat(500)
        ));
        let shown = param(&hostile);
        assert!(shown.starts_with('«') && shown.ends_with('»'));
        for bad in ['\u{1b}', '\u{7}', '\u{202e}', '\u{2028}'] {
            assert!(!shown.contains(bad), "{bad:?} survived");
        }
        assert!(shown.chars().count() <= MAX_PARAM_CHARS + 3);
    }

    #[test]
    fn no_message_mentions_an_exception_or_how_to_disable() {
        for lang in [
            include_str!("../i18n/en.txt"),
            include_str!("../i18n/es.txt"),
        ] {
            for line in lang
                .lines()
                .filter(|l| l.starts_with("guard.deny") || l.starts_with("guard.degraded"))
            {
                let lower = line.to_lowercase();
                for word in [
                    "guard exec",
                    "--no-verify",
                    "hookspath",
                    "uninstall",
                    "desinstal",
                    "excep",
                ] {
                    assert!(!lower.contains(word), "{line}");
                }
            }
        }
    }
}
