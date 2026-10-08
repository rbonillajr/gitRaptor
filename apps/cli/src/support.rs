//! What the subcommands share: the profile, the engine client and the
//! presentation of errors and refusals (ADR-GRP-016 § 5).

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gitraptor_api::messages::{
    ClientKind, RefusalReason, RefusedData, RepoRejectedData, RepoRejection, Snapshot,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::{ErrorObject, code};
use gitraptor_api::timemachine::{PriorFailedData, PriorFailure};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::channel::authz::{ConsoleIssue, own_console_issue};
use gitraptor_core::client::{Client, ClientError, ClientOptions, ensure_daemon};
use gitraptor_core::profile::ProfileDirs;

use crate::codes;
use crate::i18n::t;

pub(crate) fn profile_dirs() -> Result<ProfileDirs, ExitCode> {
    ProfileDirs::resolve().map_err(|err| {
        eprintln!("raptor: {err}");
        ExitCode::FAILURE
    })
}

/// A client of the running engine, starting it on demand.
pub(crate) fn engine(command: &str) -> Result<Client, ExitCode> {
    let dirs = profile_dirs()?;
    ensure_daemon(&ClientOptions::new(dirs, ClientKind::Cli)).map_err(|err| {
        eprintln!("{command}: {}", sanitize(&err.to_string()));
        ExitCode::FAILURE
    })
}

pub(crate) fn snapshot(client: &mut Client, command: &str) -> Result<Snapshot, ExitCode> {
    client
        .call(methods::ENGINE_SNAPSHOT, serde_json::json!({}))
        .map_err(|err| {
            eprintln!("{command}: {}", sanitize(&err.to_string()));
            ExitCode::FAILURE
        })
}

/// The message for any other failed call. A failed prior snapshot says, in
/// the user's language, that the operation did not run and why (US-TMC-001).
pub(crate) fn error_text(err: ClientError) -> String {
    match err {
        ClientError::Rpc(err) if err.code == code::PRIOR_SNAPSHOT_FAILED => t(
            prior_failure_key(
                err.data
                    .and_then(|d| serde_json::from_value::<PriorFailedData>(d).ok())
                    .map(|d| d.reason),
            ),
            &[],
        ),
        ClientError::ChannelRejected => t("channel.rejected", &[]),
        // N7: presented from the code, never from the daemon's message.
        // A module's own code (ADR-GRP-016 § 3) is presented the same way.
        ClientError::Rpc(err) => match gitraptor_api::rpc::error_name(err.code) {
            Some(name) => t(&codes::error_name_key(name), &[]),
            None => sanitize(&err.message),
        },
        other => sanitize(&other.to_string()),
    }
}

pub(crate) fn prior_failure_key(reason: Option<PriorFailure>) -> &'static str {
    match reason {
        Some(PriorFailure::NoSpace) => "prior.no-space",
        Some(PriorFailure::StoreUnavailable) => "prior.store-unavailable",
        Some(PriorFailure::Timeout) => "prior.timeout",
        Some(PriorFailure::DaemonStopping) => "prior.daemon-stopping",
        Some(PriorFailure::CaptureFailed) | None => "prior.capture-failed",
    }
}

/// The message for a refused reserved command; `agent_key` names what an
/// agent may not do.
pub(crate) fn refusal_text(err: &ErrorObject, agent_key: &str) -> String {
    let reason = err
        .data
        .clone()
        .and_then(|d| serde_json::from_value::<RefusedData>(d).ok())
        .map(|d| d.reason);
    match reason {
        Some(RefusalReason::AgentAncestry | RefusalReason::SessionLeaderAgent) => t(agent_key, &[]),
        Some(RefusalReason::NoControllingTerminal) => t(terminal_key(own_console_issue()), &[]),
        // The terminal cannot prove who is asking (mintty, a pipe, a relay):
        // say what to do instead of only that the engine could not verify.
        Some(RefusalReason::IdentityUnverified | RefusalReason::Unsupported) => t(
            if cfg!(windows) {
                "common.refused-unverified-console"
            } else {
                "common.refused-unverified-terminal"
            },
            &[],
        ),
        _ => t("common.refused-unverified", &[]),
    }
}

/// The message for `no-controlling-terminal`: on Windows this process
/// diagnoses its own console to say why and what to do (DS-TS-GRP-004 § 9,
/// C10). The daemon decided; this only explains.
fn terminal_key(issue: Option<ConsoleIssue>) -> &'static str {
    match issue {
        None => "common.refused-terminal",
        Some(ConsoleIssue::NoConsole) => "common.refused-terminal-no-console",
        Some(ConsoleIssue::NotInteractive) => "common.refused-terminal-session",
        Some(ConsoleIssue::NotYours) => "common.refused-terminal-foreign-console",
    }
}

/// Asks on the terminal. Without a terminal there is nobody to ask: no.
pub(crate) fn confirm(question: &str) -> bool {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        eprintln!("raptor: {}", t("common.confirm-needs-terminal", &[]));
        return false;
    }
    eprint!("{question} ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if stdin.lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "yes" | "s" | "si" | "sí"
    )
}

/// The folder a repo command names: the given one or the current one, made
/// absolute. Nothing is searched upwards: a folder inside a repo is not
/// that repo (US-GRP-001).
pub(crate) fn command_path(path: Option<PathBuf>) -> PathBuf {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    gitraptor_cli::paths::canonicalize(&path)
        .or_else(|_| std::path::absolute(&path))
        .unwrap_or(path)
}

pub(crate) fn shown(path: &Path) -> String {
    sanitize(&path.display().to_string())
}

pub(crate) fn repo_error(command: &str, path: &Path, err: ClientError) -> ExitCode {
    let message = match err {
        ClientError::Rpc(err) if err.code == code::RESERVED_REFUSED => {
            refusal_text(&err, "repo.refused-agent")
        }
        ClientError::Rpc(err) if err.code == code::REPO_REJECTED => {
            let reason = err
                .data
                .and_then(|d| serde_json::from_value::<RepoRejectedData>(d).ok())
                .map(|d| d.reason);
            let key = match reason {
                Some(RepoRejection::NotARepo) => "repo.not-a-repo",
                Some(RepoRejection::Untrusted) => "repo.untrusted",
                Some(RepoRejection::UnknownRepo | RepoRejection::NotObserved) => "repo.unknown",
                Some(RepoRejection::Unreadable) | None => "repo.unreadable",
            };
            t(key, &[("path", &shown(path))])
        }
        other => error_text(other),
    };
    eprintln!("{command}: {message}");
    ExitCode::FAILURE
}

/// Whether the running engine offers `method`: a daemon of the same
/// protocol but older than this binary keeps running after an upgrade.
pub(crate) fn offers(client: &Client, method: &str) -> bool {
    client.hello().methods.iter().any(|m| m == method)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n;

    /// Every reason of a failed prior snapshot has its message (en/es).
    #[test]
    fn every_prior_failure_has_a_message() {
        let reasons = [
            PriorFailure::NoSpace,
            PriorFailure::StoreUnavailable,
            PriorFailure::Timeout,
            PriorFailure::DaemonStopping,
            PriorFailure::CaptureFailed,
        ];
        for reason in reasons.into_iter().map(Some).chain([None]) {
            let key = prior_failure_key(reason);
            assert!(i18n::has_key(key), "{key}");
        }
        let err = ClientError::Rpc(
            ErrorObject::new(code::PRIOR_SNAPSHOT_FAILED, "no space").with_data(PriorFailedData {
                reason: PriorFailure::NoSpace,
                operation_id: None,
            }),
        );
        assert_ne!(error_text(err), "prior.no-space");
    }

    /// C10: every console issue has its own message (en/es), and the generic
    /// one stays for Unix.
    #[test]
    fn a_terminal_refusal_explains_the_console_issue() {
        let issues = [
            ConsoleIssue::NoConsole,
            ConsoleIssue::NotInteractive,
            ConsoleIssue::NotYours,
        ];
        let keys: Vec<_> = issues
            .into_iter()
            .map(Some)
            .chain([None])
            .map(terminal_key)
            .collect();
        for key in &keys {
            assert!(i18n::has_key(key), "{key}");
        }
        assert_eq!(keys[3], "common.refused-terminal");
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }

    fn refused(reason: RefusalReason) -> ErrorObject {
        ErrorObject::new(code::RESERVED_REFUSED, "refused").with_data(RefusedData { reason })
    }

    /// A terminal that cannot prove who asks (mintty) is told what to do, in
    /// en/es; the other reasons keep the generic text.
    #[test]
    fn an_unverifiable_terminal_is_told_what_to_do() {
        let key = if cfg!(windows) {
            "common.refused-unverified-console"
        } else {
            "common.refused-unverified-terminal"
        };
        for reason in [
            RefusalReason::IdentityUnverified,
            RefusalReason::Unsupported,
        ] {
            assert_eq!(refusal_text(&refused(reason), "x"), t(key, &[]));
        }
        for spanish in [false, true] {
            let windows = i18n::text_in(spanish, "common.refused-unverified-console").unwrap();
            let unix = i18n::text_in(spanish, "common.refused-unverified-terminal").unwrap();
            assert!(windows.contains("PowerShell") && windows.contains("Windows Terminal"));
            assert!(!unix.contains("PowerShell"));
            for text in [windows, unix] {
                assert!(text.contains("interactiv"), "{text}");
            }
        }
        assert_eq!(
            refusal_text(&refused(RefusalReason::DaemonDescendant), "x"),
            t("common.refused-unverified", &[])
        );
    }
}
