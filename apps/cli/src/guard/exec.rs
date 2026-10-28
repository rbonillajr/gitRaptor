//! `raptor guard exec [--yes] -- git <args…>`: the conscious exception (US-GRD-006). The daemon
//! normalizes the command, announces it to every client and opens a cancellable window; only
//! once it closed does it issue a one-use token, which goes into the environment of the one git
//! this process starts. Nothing here decides what is skipped: the daemon does, and the hooks of
//! that git present the token.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use gitraptor_api::guard::{
    Diagnostic, ExceptionPlanView, ExceptionRefusal, GuardExceptionChildParams,
    GuardExceptionEndParams, GuardExceptionEndResult, GuardExceptionParams,
    GuardExceptionRefusedData, GuardExceptionResult, GuardRepoParams, GuardStatus, IssuedException,
    PendingAction, ProtectionState, Rule,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::{ErrorObject, code};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::{Client, ClientError};

use super::{Answer, WINDOW_POLL, ask, guard_error, now_ms, operation_text, param};
use crate::i18n::t;
use crate::{refusal_text, shown};

const CMD: &str = "raptor guard exec";

/// The exit code of a command interrupted by Ctrl-C (128 + SIGINT).
const INTERRUPTED: u8 = 130;

/// The variable the hooks of the exception's git read the token from.
const TOKEN_ENV: &str = gitraptor_core::guardrails::hook::EXCEPTION_TOKEN_ENV;

/// The `GIT_*` variables the exception's git keeps (D10): how it authenticates and who signs.
/// Anything else of Git's (`GIT_DIR`, `GIT_EDITOR`, `GIT_PAGER`, `GIT_EXTERNAL_DIFF`,
/// `GIT_CONFIG_*`…) could change the repo or beat the `-c` the daemon fixed.
const KEPT_GIT: [&str; 4] = [
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "GIT_TERMINAL_PROMPT",
];
const KEPT_GIT_PREFIXES: [&str; 2] = ["GIT_AUTHOR_", "GIT_COMMITTER_"];

/// Programs outside Git's own variables that git would start for the user.
const DROPPED: [&str; 3] = ["EDITOR", "VISUAL", "PAGER"];

/// `raptor guard exec [--yes] -- git <args…>`. The exit code is git's; a refusal, a
/// cancellation or a missing engine never starts git.
pub fn exec(yes: bool, argv: Vec<OsString>) -> ExitCode {
    let Some(argv) = utf8(argv) else {
        eprintln!("{CMD}: {}", t("guardexec.non-utf8", &[]));
        return ExitCode::FAILURE;
    };
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("{CMD}: {}", t("guardexec.no-folder", &[]));
        return ExitCode::FAILURE;
    };
    let path = acting_dir(&cwd, &argv);
    // No engine, no exception: this command never starts one (D19).
    let mut client = match crate::profile_dirs()
        .ok()
        .and_then(|dirs| Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).ok())
    {
        Some(client) => client,
        None => {
            eprintln!("{CMD}: {}", t("guardexec.no-engine", &[]));
            return ExitCode::FAILURE;
        }
    };
    let served = client
        .hello()
        .capabilities
        .as_ref()
        .is_some_and(|caps| caps.iter().any(|c| c == methods::CAP_GUARD_EXCEPTION.name));
    if !served {
        eprintln!("{CMD}: {}", t("guardexec.unavailable", &[]));
        return ExitCode::FAILURE;
    }
    let repo = GuardRepoParams {
        path: path.to_string_lossy().into_owned(),
    };
    // Only to explain sooner: the daemon decides both again after authorizing (D12).
    // A protection that is there but not fully active is left to the daemon, which tells an
    // old template apart.
    match client.call::<_, GuardStatus>(methods::GUARD_STATUS, &repo) {
        Ok(status) if status.diagnostics.contains(&Diagnostic::TemplateOutdated) => {
            return refused(ExceptionRefusal::OutdatedInstall, &argv);
        }
        // An altered dispatch file may read as unprotected: the daemon tells an old template
        // apart from a repo never protected, so only a repo without any protection stops here.
        Ok(status)
            if status.state == ProtectionState::Unprotected
                && status.hooks.is_none()
                && !status.diagnostics.contains(&Diagnostic::LeftoverFolder) =>
        {
            return refused(ExceptionRefusal::NotInstalled, &argv);
        }
        Ok(_) => {}
        Err(err) => return guard_error(CMD, &path, err),
    }
    // From here on Ctrl-C cancels the window instead of killing this process (D8).
    let interrupted = gitraptor_core::interrupt::interrupt_flag().ok();
    let announce = GuardExceptionParams {
        path: repo.path.clone(),
        argv: argv.clone(),
        confirm: None,
    };
    let (pending, plan) =
        match client.call::<_, GuardExceptionResult>(methods::GUARD_EXCEPTION, &announce) {
            Ok(GuardExceptionResult {
                pending: Some(pending),
                plan,
                ..
            }) => (pending, plan),
            Ok(_) => {
                eprintln!("{CMD}: {}", t("error.guard-exception-refused", &[]));
                return ExitCode::FAILURE;
            }
            Err(err) => return call_error(err, &path, &argv),
        };
    show_plan(&path, &plan, &pending);
    let answer = (!yes).then(spawn_question);
    match window(&mut client, &repo, &pending, answer, interrupted.as_deref()) {
        Window::Closed => {}
        Window::Cancelled => {
            eprintln!("{CMD}: {}", t("guardexec.cancelled", &[]));
            return ExitCode::FAILURE;
        }
        Window::Declined(key) => {
            cancel(&mut client, &repo);
            eprintln!("{CMD}: {}", t(key, &[]));
            return ExitCode::FAILURE;
        }
        Window::Interrupted => {
            cancel(&mut client, &repo);
            eprintln!("{CMD}: {}", t("guardexec.cancelled", &[]));
            return ExitCode::from(INTERRUPTED);
        }
        Window::Failed(err) => return guard_error(CMD, &path, err),
    }
    let issued = match confirm(&mut client, &repo, &argv, &pending) {
        Ok(issued) => issued,
        Err(code) => return code,
    };
    run_git(&mut client, &repo, &pending.action_id, &argv, issued)
}

/// Every argument as UTF-8, or `None` (D1: a non-UTF-8 argument is refused here).
fn utf8(argv: Vec<OsString>) -> Option<Vec<String>> {
    argv.into_iter().map(|a| a.into_string().ok()).collect()
}

/// Where the git of `argv` acts: `cwd` with each leading `-C <dir>` applied, as git does. The
/// daemon checks it exists and is a worktree of the repo; any other global option is refused
/// there.
fn acting_dir(cwd: &Path, argv: &[String]) -> PathBuf {
    let mut dir = cwd.to_path_buf();
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg != "-C" {
            break;
        }
        match rest.next() {
            Some(d) if !d.is_empty() => dir = dir.join(d),
            Some(_) => {}
            None => break,
        }
    }
    dir
}

/// The git subcommand of `argv`, after its `-C` options: `pull` gets its own way out.
fn subcommand(argv: &[String]) -> Option<&str> {
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == "-C" || arg == "-c" {
            rest.next();
            continue;
        }
        return Some(arg.as_str());
    }
    None
}

fn refusal_key(reason: ExceptionRefusal, argv: &[String]) -> &'static str {
    let pull = subcommand(argv) == Some("pull");
    match reason {
        ExceptionRefusal::UnsupportedForm | ExceptionRefusal::UnsupportedOperation if pull => {
            "guardexec.refused.pull"
        }
        ExceptionRefusal::NotInstalled => "guardexec.refused.not-installed",
        ExceptionRefusal::OutdatedInstall => "guardexec.refused.outdated-install",
        ExceptionRefusal::AlreadyPending => "guardexec.refused.already-pending",
        ExceptionRefusal::WindowOpen => "guardexec.refused.window-open",
        ExceptionRefusal::NoPending => "guardexec.refused.no-pending",
        ExceptionRefusal::NotTheRequester => "guardexec.refused.not-the-requester",
        ExceptionRefusal::UnsupportedForm => "guardexec.refused.unsupported-form",
        ExceptionRefusal::UnsupportedOperation => "guardexec.refused.unsupported-operation",
        ExceptionRefusal::NotDenied => "guardexec.refused.not-denied",
        ExceptionRefusal::NotExceptable => "guardexec.refused.not-exceptable",
        ExceptionRefusal::ChildNotOurs => "guardexec.refused.child-not-ours",
    }
}

fn refused(reason: ExceptionRefusal, argv: &[String]) -> ExitCode {
    eprintln!("{CMD}: {}", t(refusal_key(reason, argv), &[]));
    ExitCode::FAILURE
}

fn refusal_data(err: &ErrorObject) -> Option<GuardExceptionRefusedData> {
    (err.code == methods::GUARD_EXCEPTION_REFUSED.code)
        .then(|| err.data.clone())
        .flatten()
        .and_then(|d| serde_json::from_value(d).ok())
}

fn call_error(err: ClientError, path: &Path, argv: &[String]) -> ExitCode {
    match err {
        ClientError::Rpc(e) if refusal_data(&e).is_some() => {
            let data = refusal_data(&e).map(|d| d.reason);
            refused(data.unwrap_or(ExceptionRefusal::NoPending), argv)
        }
        ClientError::Rpc(e) if e.code == code::RESERVED_REFUSED => {
            eprintln!("{CMD}: {}", refusal_text(&e, "guardexec.refused-agent"));
            ExitCode::FAILURE
        }
        other => guard_error(CMD, path, other),
    }
}

/// The rules as their stable names, labelled.
fn rules_text(rules: &[Rule]) -> String {
    rules
        .iter()
        .map(|r| {
            let name = serde_json::to_value(r)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            param(&gitraptor_api::Untrusted::new(name))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn show_plan(path: &Path, plan: &ExceptionPlanView, pending: &PendingAction) {
    println!(
        "{}",
        t(
            "guardexec.header",
            &[
                ("repo", &shown(path)),
                ("operation", &operation_text(&plan.operation)),
            ],
        )
    );
    println!(
        "{}",
        t("guardexec.skips", &[("rules", &rules_text(&plan.rules))])
    );
    if plan.rules.contains(&Rule::SnapshotFailed) {
        println!("{}", t("guardexec.no-prior", &[]));
    }
    println!(
        "{}",
        t(
            "guardexec.window",
            &[("seconds", &pending.window_ms.div_ceil(1000).to_string())]
        )
    );
    let _ = std::io::stdout().flush();
}

/// The question, asked on its own thread so the window keeps being watched meanwhile.
fn spawn_question() -> mpsc::Receiver<Answer> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ask(&t("guardexec.confirm", &[])));
    });
    rx
}

enum Window {
    /// Closed, and answered yes (or `--yes`).
    Closed,
    /// Someone cancelled it (`raptor guard cancel`) or it is gone.
    Cancelled,
    /// The developer did not say yes: the key of what to say.
    Declined(&'static str),
    /// Ctrl-C.
    Interrupted,
    Failed(ClientError),
}

/// Until the window closes and the question is answered: anyone may cancel meanwhile, and
/// Ctrl-C cancels it. `--yes` answers the question, never the window.
fn window(
    client: &mut Client,
    repo: &GuardRepoParams,
    pending: &PendingAction,
    mut answer: Option<mpsc::Receiver<Answer>>,
    interrupted: Option<&AtomicBool>,
) -> Window {
    let stop = || interrupted.is_some_and(|f| f.load(Ordering::SeqCst));
    loop {
        if stop() {
            return Window::Interrupted;
        }
        if let Some(rx) = &answer {
            match rx.try_recv() {
                Ok(Answer::Yes) => answer = None,
                Ok(Answer::No) => return Window::Declined("guardexec.kept"),
                Ok(Answer::None) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Window::Declined("guardexec.no-answer");
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let left = pending.applies_at_ms - now_ms();
        if left <= 0 && answer.is_none() {
            return Window::Closed;
        }
        let nap = u64::try_from(left).unwrap_or(0).max(1);
        std::thread::sleep(WINDOW_POLL.min(Duration::from_millis(nap)));
        if stop() {
            return Window::Interrupted;
        }
        match client.call::<_, GuardStatus>(methods::GUARD_STATUS, repo) {
            Ok(status)
                if status
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.action_id == pending.action_id) => {}
            Ok(_) => return Window::Cancelled,
            Err(err) => return Window::Failed(err),
        }
    }
}

/// `guard.cancel` for the announcement this process made; a failure leaves it to expire.
fn cancel(client: &mut Client, repo: &GuardRepoParams) {
    let _ = client.call::<_, GuardStatus>(methods::GUARD_CANCEL, repo);
}

/// Call 2: the token, once the daemon's clock says the window closed.
fn confirm(
    client: &mut Client,
    repo: &GuardRepoParams,
    argv: &[String],
    pending: &PendingAction,
) -> Result<IssuedException, ExitCode> {
    let params = GuardExceptionParams {
        path: repo.path.clone(),
        argv: argv.to_vec(),
        confirm: Some(pending.action_id.clone()),
    };
    loop {
        match client.call::<_, GuardExceptionResult>(methods::GUARD_EXCEPTION, &params) {
            Ok(GuardExceptionResult {
                issued: Some(issued),
                ..
            }) => return Ok(issued),
            Ok(_) => {
                eprintln!("{CMD}: {}", t("error.guard-exception-refused", &[]));
                return Err(ExitCode::FAILURE);
            }
            Err(ClientError::Rpc(e))
                if refusal_data(&e).is_some_and(|d| d.reason == ExceptionRefusal::WindowOpen) =>
            {
                let left = refusal_data(&e)
                    .and_then(|d| d.remaining_ms)
                    .unwrap_or(100)
                    .max(1);
                std::thread::sleep(Duration::from_millis(left));
            }
            Err(ClientError::Rpc(e))
                if refusal_data(&e).is_some_and(|d| d.reason == ExceptionRefusal::NoPending) =>
            {
                eprintln!("{CMD}: {}", t("guardexec.cancelled", &[]));
                return Err(ExitCode::FAILURE);
            }
            Err(err) => return Err(call_error(err, Path::new(&repo.path), argv)),
        }
    }
}

/// Whether git keeps `key` in its environment (D10).
fn kept(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        // A name that is not UTF-8 is not one of Git's nor one of the dropped ones.
        return true;
    };
    let upper = if cfg!(windows) {
        key.to_ascii_uppercase()
    } else {
        key.to_owned()
    };
    if upper == TOKEN_ENV || DROPPED.contains(&upper.as_str()) {
        return false;
    }
    if !upper.starts_with("GIT_") {
        return true;
    }
    KEPT_GIT.contains(&upper.as_str()) || KEPT_GIT_PREFIXES.iter().any(|p| upper.starts_with(p))
}

/// The environment of the exception's git: this one without what D10 drops, plus the token.
fn child_env(token: &str) -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(k, _)| kept(k))
        .chain(std::iter::once((TOKEN_ENV.into(), token.into())))
        .collect()
}

/// The daemon's git, only as it sent it: absolute and intact (never searched in `PATH`).
fn git_program(issued: &IssuedException) -> Option<PathBuf> {
    let git = &issued.git;
    let path = PathBuf::from(git.raw());
    (!git.is_lossy() && !git.is_truncated() && path.is_absolute()).then_some(path)
}

/// Starts `<git> <-c …> <args…>` without a shell, with inherited stdio and this folder,
/// registers it, waits for it and ends the exception. The exit code is git's.
fn run_git(
    client: &mut Client,
    repo: &GuardRepoParams,
    action_id: &str,
    argv: &[String],
    issued: IssuedException,
) -> ExitCode {
    let end = |client: &mut Client| {
        client.call::<_, GuardExceptionEndResult>(
            methods::GUARD_EXCEPTION_END,
            &GuardExceptionEndParams {
                path: repo.path.clone(),
                action_id: action_id.to_owned(),
            },
        )
    };
    let Some(program) = git_program(&issued) else {
        let _ = end(client);
        eprintln!("{CMD}: {}", t("guardexec.git-failed", &[]));
        return ExitCode::FAILURE;
    };
    let mut command = Command::new(program);
    for pair in &issued.config {
        command.arg("-c").arg(pair);
    }
    command
        .args(argv.iter().skip(1))
        .env_clear()
        .envs(child_env(issued.token.expose()));
    drop(issued);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = end(client);
            eprintln!("{CMD}: {}", t("guardexec.git-failed", &[]));
            return ExitCode::FAILURE;
        }
    };
    drop(command);
    // Bound before git reaches its first hook; if this fails, git still runs and its hooks
    // simply deny as always (never kill it halfway).
    let _ = client.call::<_, serde_json::Value>(
        methods::GUARD_EXCEPTION_CHILD,
        &GuardExceptionChildParams {
            path: repo.path.clone(),
            action_id: action_id.to_owned(),
            pid: child.id(),
        },
    );
    let status = child.wait();
    if let Ok(GuardExceptionEndResult { consumed: false }) = end(client) {
        eprintln!("{CMD}: {}", t("guardexec.not-used", &[]));
    }
    match status.ok().and_then(|s| s.code()) {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_acting_dir_applies_each_leading_dash_c() {
        let cwd = Path::new("/w");
        assert_eq!(
            acting_dir(cwd, &args(&["git", "-C", "a", "-C", "b", "push"])),
            PathBuf::from("/w/a/b")
        );
        assert_eq!(
            acting_dir(cwd, &args(&["git", "-C", "/r", "push", "-C", "x"])),
            PathBuf::from("/r")
        );
        assert_eq!(
            acting_dir(cwd, &args(&["git", "push"])),
            PathBuf::from("/w")
        );
    }

    #[test]
    fn pull_gets_its_own_way_out() {
        let pull = args(&["git", "-C", "/r", "pull"]);
        assert_eq!(subcommand(&pull), Some("pull"));
        assert_eq!(
            refusal_key(ExceptionRefusal::UnsupportedForm, &pull),
            "guardexec.refused.pull"
        );
        assert_eq!(
            refusal_key(
                ExceptionRefusal::UnsupportedForm,
                &args(&["git", "push", "--all"])
            ),
            "guardexec.refused.unsupported-form"
        );
    }

    #[test]
    fn the_child_environment_drops_what_could_redirect_git() {
        for key in [
            "GIT_DIR",
            "GIT_EDITOR",
            "GIT_PAGER",
            "GIT_EXTERNAL_DIFF",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
            "EDITOR",
            "VISUAL",
            "PAGER",
            TOKEN_ENV,
        ] {
            assert!(!kept(OsStr::new(key)), "{key} kept");
        }
        for key in [
            "GIT_SSH",
            "GIT_SSH_COMMAND",
            "GIT_ASKPASS",
            "GIT_TERMINAL_PROMPT",
            "GIT_AUTHOR_NAME",
            "GIT_COMMITTER_EMAIL",
            "HOME",
            "PATH",
            "LANG",
        ] {
            assert!(kept(OsStr::new(key)), "{key} dropped");
        }
    }

    #[test]
    fn every_refusal_has_its_text_in_both_languages() {
        for reason in [
            ExceptionRefusal::NotInstalled,
            ExceptionRefusal::OutdatedInstall,
            ExceptionRefusal::AlreadyPending,
            ExceptionRefusal::WindowOpen,
            ExceptionRefusal::NoPending,
            ExceptionRefusal::NotTheRequester,
            ExceptionRefusal::UnsupportedForm,
            ExceptionRefusal::UnsupportedOperation,
            ExceptionRefusal::NotDenied,
            ExceptionRefusal::NotExceptable,
            ExceptionRefusal::ChildNotOurs,
        ] {
            let key = refusal_key(reason, &args(&["git", "push"]));
            for lang in crate::i18n::feature_files("guardexec") {
                assert!(
                    lang.lines().any(|l| l.starts_with(&format!("{key} = "))),
                    "{key} missing"
                );
            }
        }
    }
}
