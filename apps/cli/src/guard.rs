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

use gitraptor_api::AgentKind;
use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    Cause, CommitStage, Decision, Diagnostic, Effect, GuardLogEntry, GuardLogParams,
    GuardLogResult, GuardPlan, GuardRejectedData, GuardRepoParams, GuardStatus,
    GuardUninstallParams, GuardUninstallRefusedData, GuardUninstallResult, HooksPathLevel,
    HooksStatus, InstallBlocker, Level, LogDetail, LogKind, LoggedOperation, LoggedReason,
    LoggedRef, LossCause, MAX_LOG_PAGE, MinimumSetStatus, NotPreventable, Param, ParamKind,
    Permission, ProtectionState, Reason, RefBackend, RefChange, Rule, UninstallRefusal,
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
        (Rule::AuthorshipTrailerRequired, cause) => t(
            if cause == Some(Cause::MessageUnreadable) {
                "guard.reason.authorship-message-unreadable"
            } else {
                "guard.reason.authorship-trailer-required"
            },
            &[("example", &find(p, ParamKind::Example))],
        ),
        (Rule::AuthorshipHumanAuthor, _) => t(
            "guard.reason.authorship-human-author",
            &[("level", &level_text(reason.level))],
        ),
        (Rule::ProtectedBranch | Rule::ForbiddenPath, Some(Cause::Unverifiable)) => {
            t("guard.reason.policy-unverifiable", &[])
        }
        (Rule::ProtectedBranch, _) => t(
            "guard.reason.protected-branch",
            &[
                ("branch", &find(p, ParamKind::Branch)),
                ("pattern", &find(p, ParamKind::Pattern)),
                ("level", &level_text(reason.level)),
            ],
        ),
        (Rule::ForbiddenPath, _) => t(
            "guard.reason.forbidden-path",
            &[
                ("path", &find(p, ParamKind::Path)),
                ("pattern", &find(p, ParamKind::Pattern)),
                ("level", &level_text(reason.level)),
            ],
        ),
        (Rule::ConfigProtected, _) => t(
            "guard.reason.config-protected",
            &[("path", &find(p, ParamKind::Path))],
        ),
        // A relaxation notice is never a denial reason; if one is rendered as such, say only
        // that the operation could not be verified.
        (Rule::RelaxIgnored, _) => t("guard.deny.internal", &[]),
    }
}

fn level_text(level: Level) -> String {
    t(
        match level {
            Level::Floor | Level::Minimum => "guard.level.floor",
            Level::Worktree => "guard.level.worktree",
            Level::Profile => "guard.level.profile",
            Level::Local => "guard.level.local",
            Level::System => "guard.level.system",
        },
        &[],
    )
}

/// The fixed template of one warning (US-GRD-018, D3).
fn notice_text(notice: &Reason) -> Option<String> {
    (notice.rule == Rule::AuthorshipHumanAuthor)
        .then(|| t("guard.notice.authorship-human-author", &[]))
}

/// The lines a decision prints on standard error: its reasons when it does not go ahead, its
/// warnings when it does.
pub fn decision_lines(decision: &Decision) -> Vec<String> {
    if decision.applied_effect == Effect::Allow {
        return decision.notices.iter().filter_map(notice_text).collect();
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
    if outcome.authorship_unavailable {
        eprintln!("{}", t("guard.notice.authorship-unavailable", &[]));
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
            NotPreventable::PolicyActor => "guard.np.policy-actor",
            NotPreventable::PolicyReach => "guard.np.policy-reach",
            NotPreventable::PolicyFloor => "guard.np.policy-floor",
        },
        &[],
    )
}

fn blocker_text(b: InstallBlocker) -> String {
    t(
        match b {
            InstallBlocker::PriorHooks => "guard.blocker.prior-hooks",
            InstallBlocker::ChainImpossible => "guard.blocker.chain-impossible",
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
    let mut out = Vec::new();
    // A protection that stopped being active: say it is a repair and exactly what changes
    // (US-GRD-004, D9) before the usual explanation.
    if let Some(repair) = &plan.repair {
        out.push(t("guard.repair.header", &[("repo", &common)]));
        out.push(t(
            "guard.repair.cause",
            &[("cause", &cause_text(repair.cause))],
        ));
        out.push(t(
            "guard.repair.files",
            &[
                ("dir", &format!("{common}/gitraptor")),
                ("files", &names(&repair.files)),
            ],
        ));
        if let Some(value) = &repair.chains {
            out.push(t(
                "guard.repair.chains",
                &[("value", &value.sanitized()), ("dir", &hooks_dir)],
            ));
        }
        out.push(t("guard.repair.edited", &[]));
        out.push(t("guard.repair.nothing-else", &[]));
        out.push(String::new());
    }
    out.extend([
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
    ]);
    // The hooks the repo already has: kept and chained (US-GRD-002, BR-AUTH-002).
    let prior = plan.prior.as_ref().filter(|p| !p.hooks.is_empty());
    if let Some(prior) = prior {
        out.push(t(
            "guard.install.prior",
            &[
                ("hooks", &names(&prior.hooks)),
                ("dir", &prior.dir.sanitized()),
            ],
        ));
        out.push(t("guard.install.prior-kept", &[]));
    }
    out.push(t("guard.install.cannot-prevent", &[]));
    for n in &plan.not_preventable {
        out.push(format!("  - {}", not_preventable_text(*n)));
    }
    if plan.backend == RefBackend::Reftable {
        out.push(t("guard.install.reftable", &[]));
    }
    out.push(t("guard.install.cost", &[]));
    out.push(t("guard.install.unavailable", &[]));
    out.push(t("guard.install.revert", &[]));
    let config = shell_quote(&format!("{common}/config"));
    match plan
        .prior
        .as_ref()
        .filter(|p| p.level == HooksPathLevel::Local)
        .and_then(|p| p.hooks_path.as_ref())
    {
        Some(value) => out.push(t(
            "guard.install.revert-restore",
            &[
                ("config", &config),
                ("value", &shell_quote(&value.sanitized())),
            ],
        )),
        None => out.push(format!(
            "  git config --file {config} --unset core.hooksPath"
        )),
    }
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
    out.extend(protection_lines(status));
    // Granted but unprotected reads as a contradiction: the permission only
    // matters once there is a protection to apply.
    let unprotected = status.state == ProtectionState::Unprotected;
    match status.permission {
        Permission::NotAsked => out.push(t("guard.permission.not-asked", &[])),
        Permission::Granted if unprotected => {}
        Permission::Granted => out.push(t("guard.permission.granted", &[])),
        Permission::Denied => out.push(t("guard.permission.denied", &[])),
    }
    for b in &status.last_refusal {
        out.push(format!("  - {}", blocker_text(*b)));
    }
    if let Some(pending) = &status.pending {
        out.push(t(
            "guard.status.pending",
            &[("seconds", &seconds_left(pending.applies_at_ms))],
        ));
    }
    for file in &status.misnamed_settings {
        out.push(t(
            "guard.status.misnamed-settings",
            &[("file", &file.sanitized())],
        ));
    }
    out
}

/// What `guard.status` says about the hook layer, the diagnostics and the safe minimum
/// (US-GRD-004); nothing when the daemon predates `guard.protection`.
fn protection_lines(status: &GuardStatus) -> Vec<String> {
    let mut out = notice_lines(status);
    if let Some(min) = &status.minimum_set
        && min.status == MinimumSetStatus::Active
        && status.hooks.is_some()
    {
        // Nothing is denied until the hooks are installed: say what install will do.
        out.push(t(
            if status.state == ProtectionState::Unprotected {
                "guard.status.minimum-on-install"
            } else {
                "guard.status.minimum-active"
            },
            &[],
        ));
    }
    if status.hooks.is_some() {
        // The agent protection of the configuration is part of the minimum: no setting turns it off.
        out.push(t("guard.status.config-protected", &[]));
        out.push(t("guard.status.not-checked", &[]));
    }
    out
}

/// What is worth the developer's attention about the protection of a repo: it stopped being
/// active, its record was lost, its hooks are of an older template, or Git's configuration
/// could not be read (US-GRD-004). Empty when there is nothing: `raptor status` shows only this.
pub fn notice_lines(status: &GuardStatus) -> Vec<String> {
    let mut out = Vec::new();
    match &status.hooks {
        Some(hooks) if hooks.status == HooksStatus::Inactive => {
            let cause = hooks.cause.map_or_else(String::new, cause_text);
            out.push(match &hooks.worktree {
                Some(worktree) => t(
                    "guard.status.lost-worktree",
                    &[("cause", &cause), ("worktree", &param(worktree))],
                ),
                None => t("guard.status.lost", &[("cause", &cause)]),
            });
        }
        Some(hooks) if hooks.status == HooksStatus::Orphaned => {
            out.push(t("guard.status.orphaned", &[]));
        }
        _ => {}
    }
    for d in &status.diagnostics {
        match d {
            Diagnostic::TemplateOutdated => out.push(t("guard.status.template-outdated", &[])),
            Diagnostic::ConfigUnreadable => out.push(t("guard.status.config-unreadable", &[])),
            // Said by the base branch line.
            Diagnostic::BaseUnconfirmed => {}
        }
    }
    out
}

/// The text of a cause of loss.
pub fn cause_text(cause: LossCause) -> String {
    t(
        match cause {
            LossCause::HookspathChanged => "guard.cause.hookspath-changed",
            LossCause::RepoMoved => "guard.cause.repo-moved",
            LossCause::FolderMissing => "guard.cause.folder-missing",
            LossCause::DispatcherMissing => "guard.cause.dispatcher-missing",
            LossCause::DispatcherAltered => "guard.cause.dispatcher-altered",
            LossCause::DispatcherNotExecutable => "guard.cause.dispatcher-not-executable",
            LossCause::BinaryMissing => "guard.cause.binary-missing",
        },
        &[],
    )
}

fn hooks_status_text(status: HooksStatus) -> String {
    t(
        match status {
            HooksStatus::Active => "guard.hooks.active",
            HooksStatus::Inactive => "guard.hooks.inactive",
            HooksStatus::NotInstalled => "guard.hooks.not-installed",
            HooksStatus::Orphaned => "guard.hooks.orphaned",
        },
        &[],
    )
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
    let repair = plan.repair.is_some();
    let answer = if yes {
        Answer::Yes
    } else {
        ask(&t(
            if repair {
                "guard.repair.confirm"
            } else {
                "guard.install.confirm"
            },
            &[],
        ))
    };
    match answer {
        Answer::None => {
            eprintln!("{CMD}: {}", t("guard.install.no-answer", &[]));
            ExitCode::FAILURE
        }
        // Not repairing is not denying the permission: nothing is recorded.
        Answer::No if repair => {
            println!("{}", t("guard.repair.declined", &[]));
            ExitCode::SUCCESS
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
                println!(
                    "{}",
                    t(
                        if repair {
                            "guard.repair.done"
                        } else {
                            "guard.install.done"
                        },
                        &[("path", &shown(&path))]
                    )
                );
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
            // Where to see what was blocked (US-GRD-005, D1): only from an engine that has it.
            if status.state == ProtectionState::HooksOnly
                && crate::offers(&client, methods::GUARD_LOG)
                && let Ok(log) = client.call::<_, GuardLogResult>(
                    methods::GUARD_LOG,
                    &log_params(&path, LOG_DEFAULT_DAYS, Some(1)),
                )
            {
                println!(
                    "{}",
                    t(
                        "guard.status.blocked",
                        &[("count", &log.summary.blocked.to_string())]
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => guard_error(CMD, &path, err),
    }
}

/// Whole seconds until `at_ms`, rounded up.
fn seconds_left(at_ms: i64) -> String {
    let left = (at_ms - now_ms()).max(0);
    ((left + 999) / 1000).to_string()
}

fn uninstall_refusal(err: &gitraptor_api::rpc::ErrorObject) -> Option<GuardUninstallRefusedData> {
    (err.code == methods::GUARD_UNINSTALL_REFUSED.code)
        .then(|| err.data.clone())
        .flatten()
        .and_then(|d| serde_json::from_value(d).ok())
}

fn uninstall_refusal_text(reason: UninstallRefusal) -> String {
    t(
        match reason {
            UninstallRefusal::NotInstalled => "guard.uninstall.not-installed",
            UninstallRefusal::AlreadyPending => "guard.uninstall.already-pending",
            UninstallRefusal::NotTheRequester => "guard.uninstall.not-the-requester",
            UninstallRefusal::WindowOpen | UninstallRefusal::NoPending => {
                "guard.uninstall.cancelled"
            }
        },
        &[],
    )
}

/// How often the requester looks whether its announcement was cancelled.
const WINDOW_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// `raptor guard uninstall [path] [--yes]` (US-GRD-003): explains, asks, announces the removal
/// and opens the window (ADR-GRD-007 § 1, D5), then applies it when the window closes unless
/// someone cancelled it. `--yes` answers the question, never the window. Ctrl-C ends this
/// command before it applies anything: the announcement then expires on its own.
pub fn uninstall(path: Option<PathBuf>, yes: bool) -> ExitCode {
    const CMD: &str = "raptor guard uninstall";
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let repo = GuardRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    let status = match client.call::<_, GuardStatus>(methods::GUARD_STATUS, &repo) {
        Ok(status) => status,
        Err(err) => return guard_error(CMD, &path, err),
    };
    // A protection that stopped being active can be removed too (US-GRD-004).
    let inactive = status
        .hooks
        .as_ref()
        .is_some_and(|h| h.status == HooksStatus::Inactive);
    if status.state != ProtectionState::HooksOnly && !inactive {
        eprintln!("{CMD}: {}", t("guard.uninstall.not-installed", &[]));
        return ExitCode::FAILURE;
    }
    let common = crate::shown(&path);
    println!("{}", t("guard.uninstall.header", &[("repo", &common)]));
    println!();
    println!(
        "{}",
        t(
            "guard.uninstall.what",
            &[("dir", &"gitraptor/hooks".to_owned())]
        )
    );
    println!("{}", t("guard.uninstall.after", &[]));
    let answer = if yes {
        Answer::Yes
    } else {
        ask(&t("guard.uninstall.confirm", &[]))
    };
    match answer {
        Answer::None => {
            eprintln!("{CMD}: {}", t("guard.uninstall.no-answer", &[]));
            return ExitCode::FAILURE;
        }
        Answer::No => {
            println!("{}", t("guard.uninstall.kept", &[]));
            return ExitCode::SUCCESS;
        }
        Answer::Yes => {}
    }
    let refused = |err: ClientError| match &err {
        ClientError::Rpc(e) if uninstall_refusal(e).is_some() => {
            let data = uninstall_refusal(e).expect("checked");
            eprintln!("{CMD}: {}", uninstall_refusal_text(data.reason));
            ExitCode::FAILURE
        }
        _ => guard_error(CMD, &path, err),
    };
    let announce = GuardUninstallParams {
        path: repo.path.clone(),
        confirm: None,
    };
    let pending = match client.call::<_, GuardUninstallResult>(methods::GUARD_UNINSTALL, &announce)
    {
        Ok(GuardUninstallResult {
            pending: Some(pending),
            ..
        }) => pending,
        Ok(_) => {
            eprintln!("{CMD}: {}", t("guard.uninstall.failed", &[]));
            return ExitCode::FAILURE;
        }
        Err(err) => return refused(err),
    };
    println!(
        "{}",
        t(
            "guard.uninstall.window",
            &[("seconds", &pending.window_ms.div_ceil(1000).to_string())]
        )
    );
    let _ = std::io::stdout().flush();
    // Until the window closes: anyone may cancel it meanwhile.
    while now_ms() < pending.applies_at_ms {
        std::thread::sleep(
            WINDOW_POLL.min(std::time::Duration::from_millis(
                u64::try_from(pending.applies_at_ms - now_ms())
                    .unwrap_or(0)
                    .max(1),
            )),
        );
        match client.call::<_, GuardStatus>(methods::GUARD_STATUS, &repo) {
            Ok(status)
                if status
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.action_id == pending.action_id) => {}
            Ok(_) => {
                eprintln!("{CMD}: {}", t("guard.uninstall.cancelled", &[]));
                return ExitCode::FAILURE;
            }
            Err(err) => return guard_error(CMD, &path, err),
        }
    }
    let apply = GuardUninstallParams {
        path: repo.path.clone(),
        confirm: Some(pending.action_id.clone()),
    };
    loop {
        match client.call::<_, GuardUninstallResult>(methods::GUARD_UNINSTALL, &apply) {
            Ok(result) => {
                println!("{}", t("guard.uninstall.done", &[("path", &shown(&path))]));
                for line in status_lines(&result.status) {
                    println!("{line}");
                }
                return ExitCode::SUCCESS;
            }
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
            Err(err) => return refused(err),
        }
    }
}

/// `raptor guard cancel [path]`: cancels the removal waiting in a repo (not reserved: it only
/// keeps the protection).
pub fn cancel(path: Option<PathBuf>) -> ExitCode {
    const CMD: &str = "raptor guard cancel";
    let path = command_path(path);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = GuardRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    match client.call::<_, GuardStatus>(methods::GUARD_CANCEL, &params) {
        Ok(_) => {
            println!("{}", t("guard.cancel.done", &[("path", &shown(&path))]));
            ExitCode::SUCCESS
        }
        Err(ClientError::Rpc(e)) if uninstall_refusal(&e).is_some() => {
            eprintln!("{CMD}: {}", t("guard.cancel.none", &[]));
            ExitCode::FAILURE
        }
        Err(err) => guard_error(CMD, &path, err),
    }
}

/// `--days` of `raptor guard log`: the default window and the retention (BR-TIME-002).
pub const LOG_DEFAULT_DAYS: u32 = 7;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The fixed name of a logged rule.
fn log_rule_text(reason: &LoggedReason) -> String {
    let rule = match reason.rule {
        Rule::MinimumForcePush => "guard.log.rule.force-push",
        Rule::MinimumBaseBranchDelete => "guard.log.rule.base-branch-delete",
        Rule::AuthorshipTrailerRequired => "guard.log.rule.trailer-required",
        Rule::AuthorshipHumanAuthor => "guard.log.rule.human-author",
        Rule::ProtectedBranch => "guard.log.rule.protected-branch",
        Rule::ForbiddenPath => "guard.log.rule.forbidden-path",
        Rule::ConfigProtected => "guard.log.rule.config-protected",
        Rule::RelaxIgnored => "guard.log.rule.relax-ignored",
        Rule::Degraded
        | Rule::ChannelNotAuthentic
        | Rule::RepoMismatch
        | Rule::InputRejected
        | Rule::InternalError => "guard.log.rule.system",
    };
    let level = match reason.level {
        Level::Minimum => t("guard.log.level-minimum", &[]),
        other => level_text(other),
    };
    t(
        "guard.log.rule",
        &[("rule", &t(rule, &[])), ("level", &level)],
    )
}

fn change_text(r: &LoggedRef) -> String {
    let key = match r.change {
        RefChange::Create => "guard.log.change.create",
        RefChange::Update => "guard.log.change.update",
        RefChange::Force => "guard.log.change.force",
        RefChange::Delete => "guard.log.change.delete",
    };
    t(key, &[("name", &param(&r.name))])
}

fn refs_text(refs: &[LoggedRef]) -> String {
    refs.iter().map(change_text).collect::<Vec<_>>().join(", ")
}

fn operation_text(op: &LoggedOperation) -> String {
    match op {
        LoggedOperation::Push { remote, refs } => t(
            "guard.log.op.push",
            &[
                (
                    "remote",
                    &remote
                        .as_ref()
                        .map_or_else(|| t("guard.log.unknown", &[]), param),
                ),
                ("refs", &refs_text(refs)),
            ],
        ),
        LoggedOperation::RefTransaction { refs } => {
            t("guard.log.op.refs", &[("refs", &refs_text(refs))])
        }
        LoggedOperation::Rebase { .. } => t("guard.log.op.rebase", &[]),
        LoggedOperation::Commit {
            stage: CommitStage::SecondLine,
        } => t("guard.log.op.commit-second-line", &[]),
        LoggedOperation::Commit { .. } => t("guard.log.op.commit", &[]),
        LoggedOperation::ProtectionState {
            from,
            to,
            cause,
            expected,
        } => {
            let (from, to) = (hooks_status_text(*from), hooks_status_text(*to));
            let args: [(&str, &dyn std::fmt::Display); 2] = [("from", &from), ("to", &to)];
            match (expected, cause) {
                (true, _) => t("guard.log.op.protection-own", &args),
                (false, Some(c)) => t(
                    "guard.log.op.protection-cause",
                    &[("from", &from), ("to", &to), ("cause", &cause_text(*c))],
                ),
                (false, None) => t("guard.log.op.protection", &args),
            }
        }
    }
}

fn agent_text(agent: Option<AgentKind>) -> String {
    match agent {
        Some(AgentKind::ClaudeCode) => "claude-code".into(),
        Some(AgentKind::Other) => "other".into(),
        None => t("guard.log.unattributed", &[]),
    }
}

/// The lines of one log entry: when, what was decided on which operation, why, who and
/// where; for a commit, what is known of its authorship (DS-US-GRD-018 D12).
pub fn log_entry_lines(e: &GuardLogEntry) -> Vec<String> {
    let decided = match e.kind {
        LogKind::Denial => t("guard.log.denied", &[]),
        LogKind::Notice => t("guard.log.notice", &[]),
        LogKind::ProtectionState => t("guard.log.protection", &[]),
    };
    let mut first = format!(
        "{} · {decided} · {}",
        crate::events::local_time(e.at_ms, e.utc_offset_s),
        operation_text(&e.operation)
    );
    if e.count > 1 {
        first.push_str(&format!(
            " ({})",
            t(
                "guard.log.times",
                &[
                    ("count", &e.count.to_string()),
                    (
                        "last",
                        &crate::events::local_time(e.last_ms, e.utc_offset_s)
                    ),
                ],
            )
        ));
    }
    let mut out = vec![first];
    if e.kind == LogKind::ProtectionState {
        return out;
    }
    for reason in &e.reasons {
        out.push(format!("  {}", log_rule_text(reason)));
    }
    let policy = e.authorship.as_ref().and_then(|a| a.policy.as_deref());
    if e.reasons.is_empty() && policy == Some("flexible") {
        out.push(format!("  {}", t("guard.log.rule.flexible", &[])));
    }
    if e.detail == LogDetail::RateLimited {
        out.push(format!("  {}", t("guard.log.over-cap", &[])));
        return out;
    }
    out.push(format!(
        "  {}",
        t("guard.log.actor", &[("actor", &agent_text(e.actor))])
    ));
    if e.worktree.is_some() || e.branch.is_some() {
        let unknown = || t("guard.log.unknown", &[]);
        out.push(format!(
            "  {}",
            t(
                "guard.log.where",
                &[
                    ("worktree", &e.worktree.as_ref().map_or_else(unknown, param)),
                    ("branch", &e.branch.as_ref().map_or_else(unknown, param)),
                ],
            )
        ));
    }
    if matches!(e.operation, LoggedOperation::Commit { .. }) {
        out.push(format!(
            "  {}",
            match e.kind {
                LogKind::Denial => t("guard.log.author-not-created", &[]),
                LogKind::Notice | LogKind::ProtectionState => {
                    t("guard.log.author-see-events", &[])
                }
            }
        ));
        if let Some(a) = &e.authorship {
            let agents: Vec<String> = a
                .coauthors
                .iter()
                .flatten()
                .map(|k| agent_text(Some(*k)))
                .collect();
            out.push(format!(
                "  {}",
                if agents.is_empty() {
                    t("guard.log.no-agent-trailer", &[])
                } else {
                    t("guard.log.coauthors", &[("agents", &agents.join(", "))])
                }
            ));
        }
    }
    out
}

/// The whole text of `raptor guard log`: the count, what was not logged, then the entries.
pub fn log_lines(log: &GuardLogResult, days: u32) -> Vec<String> {
    let mut out = vec![t(
        "guard.log.summary",
        &[
            ("count", &log.summary.blocked.to_string()),
            ("days", &days.to_string()),
        ],
    )];
    if log.summary.notices > 0 {
        out.push(t(
            "guard.log.notices",
            &[("count", &log.summary.notices.to_string())],
        ));
    }
    if log.summary.rate_limited > 0 {
        out.push(t(
            "guard.log.rate-limited",
            &[("count", &log.summary.rate_limited.to_string())],
        ));
    }
    let offset = log.entries.first().map_or(0, |e| e.utc_offset_s);
    for gap in &log.unlogged_periods {
        let from = crate::events::local_time(gap.from_ms, offset);
        out.push(match gap.to_ms {
            Some(to) => t(
                "guard.log.unlogged",
                &[
                    ("from", &from),
                    ("to", &crate::events::local_time(to, offset)),
                ],
            ),
            None => t("guard.log.unlogged-open", &[("from", &from)]),
        });
    }
    if log.entries.is_empty() {
        out.push(t("guard.log.empty", &[]));
    }
    for entry in &log.entries {
        out.push(String::new());
        out.extend(log_entry_lines(entry));
    }
    out
}

fn log_params(path: &std::path::Path, days: u32, limit: Option<u32>) -> GuardLogParams {
    GuardLogParams {
        path: path.to_string_lossy().into_owned(),
        since_ms: Some(now_ms() - i64::from(days) * DAY_MS),
        limit,
    }
}

/// `raptor guard log [path] [--days N] [--limit N] [--json]` (US-GRD-005).
pub fn log(path: Option<PathBuf>, days: u32, limit: u32, json: bool) -> ExitCode {
    const CMD: &str = "raptor guard log";
    let path = command_path(path);
    let days = days.clamp(1, gitraptor_api::guard::LOG_RETENTION_DAYS as u32);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    if !crate::offers(&client, methods::GUARD_LOG) {
        eprintln!("{CMD}: {}", t("guard.restart-engine", &[]));
        return ExitCode::FAILURE;
    }
    let params = log_params(&path, days, Some(limit.clamp(1, MAX_LOG_PAGE)));
    match client.call::<_, GuardLogResult>(methods::GUARD_LOG, &params) {
        Ok(log) if json => {
            println!("{}", serde_json::to_string(&log).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(log) => {
            for line in log_lines(&log, days) {
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

    fn log_entry(kind: LogKind, operation: LoggedOperation) -> GuardLogEntry {
        GuardLogEntry {
            at_ms: 1_800_000_000_000,
            utc_offset_s: 0,
            last_ms: 1_800_000_060_000,
            count: 3,
            worktree: Some(Untrusted::new("/w/\u{1b}[31mdemo")),
            branch: Some(Untrusted::new("feat-\u{7}x")),
            actor: None,
            operation,
            kind,
            detail: LogDetail::Full,
            effect: Effect::Deny,
            applied_effect: Effect::Deny,
            reasons: vec![LoggedReason {
                rule: Rule::MinimumForcePush,
                level: Level::Minimum,
                cause: None,
            }],
            layer: gitraptor_api::guard::LogLayer::Hooks,
            origin: gitraptor_api::guard::LogOrigin::Daemon,
            decision_id: "d".into(),
            authorship: None,
        }
    }

    /// Every message the log can print exists in both languages (US-GRD-005).
    #[test]
    fn entry_lines_in_both_languages() {
        let source = include_str!("guard.rs");
        let keys: Vec<&str> = source
            .split('"')
            .filter(|s| {
                (s.starts_with("guard.log.") && !s.ends_with('.')) || *s == "guard.status.blocked"
            })
            .collect();
        assert!(keys.len() > 20, "{keys:?}");
        for key in keys {
            assert!(has_key(key), "{key}");
        }
        assert!(has_key("guard.restart-engine"));
    }

    /// Paths, branches, refs and remotes come from the repo: shown sanitized, inside «» (M-05).
    #[test]
    fn untrusted_fields_are_sanitized() {
        let push = LoggedOperation::Push {
            remote: Some(Untrusted::new("orig\u{1b}]0;x\u{7}in")),
            refs: vec![LoggedRef {
                name: Untrusted::new("ma\u{1b}[2Jin"),
                change: RefChange::Force,
            }],
        };
        let lines = log_entry_lines(&log_entry(LogKind::Denial, push)).join("\n");
        assert!(
            !lines.contains('\u{1b}') && !lines.contains('\u{7}'),
            "{lines:?}"
        );
        assert!(lines.contains('«'), "{lines}");
        // A commit denial says the author is not available; a notice points to the events.
        let commit = LoggedOperation::Commit {
            stage: CommitStage::CommitMsg,
        };
        let denied = log_entry_lines(&log_entry(LogKind::Denial, commit.clone())).join("\n");
        assert!(
            denied.contains(&t("guard.log.author-not-created", &[])),
            "{denied}"
        );
        let noticed = log_entry_lines(&log_entry(LogKind::Notice, commit)).join("\n");
        assert!(
            noticed.contains(&t("guard.log.author-see-events", &[])),
            "{noticed}"
        );
    }

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
            "guard.status.misnamed-settings",
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
        for lang in crate::i18n::feature_files("guard") {
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

    #[test]
    fn status_names_a_misnamed_settings_file_without_its_control_characters() {
        let status = GuardStatus {
            repo_id: "r".into(),
            state: ProtectionState::Unprotected,
            permission: Permission::NotAsked,
            offer: true,
            protected_bases: Vec::new(),
            base_confirmed: false,
            not_preventable: Vec::new(),
            last_refusal: Vec::new(),
            misnamed_settings: vec![Untrusted::new(".gitraptor/config\u{1b}[31m.json")],
            pending: None,
            hooks: None,
            diagnostics: Vec::new(),
            minimum_set: None,
        };
        let lines = status_lines(&status);
        let last = lines.last().unwrap();
        assert!(last.contains(".gitraptor/config"), "{last}");
        assert!(last.contains("settings.json"), "{last}");
        assert!(!last.contains('\u{1b}'), "{last}");

        let clean = GuardStatus {
            misnamed_settings: Vec::new(),
            ..status
        };
        assert!(
            !status_lines(&clean)
                .iter()
                .any(|l| l.contains("settings.json"))
        );
    }

    fn bare_status(state: ProtectionState, permission: Permission) -> GuardStatus {
        GuardStatus {
            repo_id: "r".into(),
            state,
            permission,
            offer: true,
            protected_bases: Vec::new(),
            base_confirmed: false,
            not_preventable: Vec::new(),
            last_refusal: Vec::new(),
            misnamed_settings: Vec::new(),
            pending: None,
            hooks: Some(gitraptor_api::guard::HooksLayer::of(
                HooksStatus::NotInstalled,
            )),
            diagnostics: Vec::new(),
            minimum_set: Some(gitraptor_api::guard::MinimumSet {
                status: MinimumSetStatus::Active,
            }),
        }
    }

    #[test]
    fn status_of_an_unprotected_repo_does_not_claim_anything_is_denied() {
        let lines = status_lines(&bare_status(
            ProtectionState::Unprotected,
            Permission::Granted,
        ))
        .join("\n");
        assert!(
            lines.contains(&t("guard.status.minimum-on-install", &[])),
            "{lines}"
        );
        assert!(
            !lines.contains(&t("guard.status.minimum-active", &[])),
            "{lines}"
        );
        assert!(
            !lines.contains(&t("guard.permission.granted", &[])),
            "{lines}"
        );
        for spanish in [false, true] {
            let text = crate::i18n::text_in(spanish, "guard.status.minimum-on-install").unwrap();
            assert!(text.contains("raptor guard install"), "{text}");
            assert!(
                !text.contains("are denied") && !text.contains("se deniegan"),
                "{text}"
            );
        }
        // Not asked / denied still say so.
        let asked = status_lines(&bare_status(
            ProtectionState::Unprotected,
            Permission::NotAsked,
        ));
        assert!(asked.contains(&t("guard.permission.not-asked", &[])));
    }

    #[test]
    fn config_protection_and_relax_ignored_have_their_own_texts_in_both_languages() {
        for spanish in [false, true] {
            for key in [
                "guard.reason.config-protected",
                "guard.log.rule.config-protected",
                "guard.log.rule.relax-ignored",
                "guard.status.config-protected",
            ] {
                let text = crate::i18n::text_in(spanish, key).unwrap();
                assert!(!text.is_empty(), "{key}");
            }
        }
        let denial = crate::i18n::text_in(false, "guard.reason.config-protected").unwrap();
        assert!(denial.contains("Guardrails configuration"), "{denial}");
        let relax = crate::i18n::text_in(false, "guard.log.rule.relax-ignored").unwrap();
        assert!(
            relax.contains("relaxation was ignored while the agent acted"),
            "{relax}"
        );
    }

    #[test]
    fn status_of_a_protected_repo_keeps_the_minimum_and_the_permission() {
        let lines = status_lines(&bare_status(
            ProtectionState::HooksOnly,
            Permission::Granted,
        ))
        .join("\n");
        assert!(
            lines.contains(&t("guard.status.minimum-active", &[])),
            "{lines}"
        );
        assert!(
            lines.contains(&t("guard.permission.granted", &[])),
            "{lines}"
        );
        assert!(
            !lines.contains(&t("guard.status.minimum-on-install", &[])),
            "{lines}"
        );
    }
}
