//! `raptor agent register` and `raptor agent withdraw` (US-GRP-009): the
//! explicit registration of an agent in a worktree, by the developer or by
//! the agent itself, and its withdrawal (reserved to the developer).
//!
//! The engine decides who asks: the developer names the worktree; an agent
//! registers in the worktree it works in, as the agent it is
//! (ADR-GRP-005 § 6.6). Every text from the engine is untrusted (SEC-12).

use std::path::PathBuf;
use std::process::ExitCode;

use gitraptor_api::messages::{
    AgentSupport, DeclaredAgent, RegistrationOutcome, RegistrationRegisterParams,
    RegistrationRegisterResult, RegistrationRejectedData, RegistrationRejection,
    RegistrationWithdrawParams, RegistrationWithdrawResult,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::{InvalidData, code};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::ClientError;
use serde_json::json;

use crate::i18n::t;
use crate::status::wire;
use crate::{command_path, engine, error_text, refusal_text};

/// Claude Code by any of its usual names; anything else is the declared
/// name of an "other agent" (BR-VAL-001).
pub fn declared(agent: &str) -> DeclaredAgent {
    let folded = agent
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    match folded.as_str() {
        "claude code" | "claude-code" | "claude" => DeclaredAgent::ClaudeCode,
        _ => DeclaredAgent::Other {
            name: agent.trim().to_owned(),
        },
    }
}

/// The agent as the messages name it.
fn shown(agent: &DeclaredAgent) -> String {
    match agent {
        DeclaredAgent::ClaudeCode => t("actor.claude-code", &[]),
        DeclaredAgent::Other { name } => t("actor.other-agent", &[("name", &sanitize(name))]),
    }
}

/// `raptor agent register <agent> [--worktree <path>] [--json]`.
pub fn register(agent: &str, worktree: Option<PathBuf>, json: bool) -> ExitCode {
    const CMD: &str = "raptor agent register";
    let agent = declared(agent);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = RegistrationRegisterParams {
        agent: agent.clone(),
        // Only a named folder: without it, the engine takes the caller's.
        worktree: worktree.map(|w| command_path(Some(w)).to_string_lossy().into_owned()),
    };
    match client.call::<_, RegistrationRegisterResult>(methods::REGISTRATION_REGISTER, &params) {
        Ok(result) if json => {
            let (agent, agent_name, origin) = match &result.actor {
                gitraptor_api::Actor::Agent { kind, name, origin } => (
                    wire(kind),
                    name.as_ref().map(|n| n.raw().to_owned()),
                    wire(origin),
                ),
                gitraptor_api::Actor::Unattributed => (String::new(), None, String::new()),
            };
            println!(
                "{}",
                json!({
                    "repo_id": result.repo_id,
                    "session_id": result.session_id,
                    "outcome": wire(&result.outcome),
                    "agent": agent,
                    "agent_name": agent_name,
                    "origin": origin,
                    "support": wire(&result.support),
                })
            );
            ExitCode::SUCCESS
        }
        Ok(result) => {
            println!("{}", register_text(&agent, &result));
            ExitCode::SUCCESS
        }
        Err(err) => fail(CMD, &agent, err),
    }
}

fn register_text(agent: &DeclaredAgent, result: &RegistrationRegisterResult) -> String {
    let key = match result.outcome {
        RegistrationOutcome::Created => "agent.created",
        RegistrationOutcome::Confirmed => "agent.confirmed",
        RegistrationOutcome::AlreadyRegistered => "agent.already-registered",
    };
    let mut out = t(key, &[("agent", &shown(agent))]);
    if let (AgentSupport::Observed, DeclaredAgent::Other { name }) = (result.support, agent) {
        out.push('\n');
        out.push_str(&t("agent.other-notice", &[("name", &sanitize(name))]));
    }
    out
}

/// `raptor agent withdraw <agent> [--worktree <path>]`: reserved.
pub fn withdraw(agent: &str, worktree: Option<PathBuf>) -> ExitCode {
    const CMD: &str = "raptor agent withdraw";
    let agent = declared(agent);
    let mut client = match engine(CMD) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let params = RegistrationWithdrawParams {
        worktree: command_path(worktree).to_string_lossy().into_owned(),
        agent: agent.clone(),
    };
    match client.call::<_, RegistrationWithdrawResult>(methods::REGISTRATION_WITHDRAW, &params) {
        Ok(_) => {
            println!("{}", t("agent.withdrawn", &[("agent", &shown(&agent))]));
            ExitCode::SUCCESS
        }
        Err(err) => fail(CMD, &agent, err),
    }
}

fn fail(command: &str, agent: &DeclaredAgent, err: ClientError) -> ExitCode {
    let message = match err {
        ClientError::Rpc(err) if err.code == code::RESERVED_REFUSED => {
            refusal_text(&err, "agent.refused-agent")
        }
        ClientError::Rpc(err) if err.code == code::REGISTRATION_REJECTED => {
            let reason = err
                .data
                .and_then(|d| serde_json::from_value::<RegistrationRejectedData>(d).ok())
                .map(|d| d.reason);
            match reason {
                Some(reason) => rejection_text(reason, agent),
                None => t("error.registration-rejected", &[]),
            }
        }
        ClientError::Rpc(err) if err.code == code::INVALID_PARAMS => {
            match err
                .data
                .and_then(|d| serde_json::from_value::<InvalidData>(d).ok())
            {
                Some(d) => t(&format!("invalid.{}", wire(&d.reason)), &[]),
                None => t("error.invalid-params", &[]),
            }
        }
        other => error_text(other),
    };
    eprintln!("{command}: {message}");
    ExitCode::FAILURE
}

fn rejection_text(reason: RegistrationRejection, agent: &DeclaredAgent) -> String {
    t(
        &format!("registration.{}", wire(&reason)),
        &[("agent", &shown(agent))],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::has_key;

    #[test]
    fn claude_code_by_its_names_and_any_other_by_its_name() {
        for name in ["Claude Code", "claude-code", "claude", "  CLAUDE   code "] {
            assert_eq!(declared(name), DeclaredAgent::ClaudeCode, "{name}");
        }
        assert_eq!(
            declared(" Codex "),
            DeclaredAgent::Other {
                name: "Codex".into()
            }
        );
    }

    #[test]
    fn every_rejection_and_outcome_has_its_text() {
        for reason in RegistrationRejection::ALL {
            assert!(
                has_key(&format!("registration.{}", wire(&reason))),
                "{reason:?}"
            );
        }
        for key in [
            "agent.created",
            "agent.confirmed",
            "agent.already-registered",
            "agent.other-notice",
            "agent.withdrawn",
            "agent.refused-agent",
            "error.registration-rejected",
        ] {
            assert!(has_key(key), "{key}");
        }
    }

    /// BR-VAL-001: an "other agent" gets the notice; its name is shown
    /// sanitized.
    #[test]
    fn an_other_agent_gets_the_notice() {
        let agent = declared("Codex\u{1b}[31m");
        let result = RegistrationRegisterResult {
            repo_id: "r".into(),
            session_id: "reg:1:0".into(),
            outcome: RegistrationOutcome::Created,
            actor: gitraptor_api::Actor::Unattributed,
            support: AgentSupport::Observed,
        };
        let text = register_text(&agent, &result);
        assert!(text.lines().count() == 2, "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
        let claude = RegistrationRegisterResult {
            support: AgentSupport::Full,
            ..result
        };
        assert_eq!(
            register_text(&DeclaredAgent::ClaudeCode, &claude)
                .lines()
                .count(),
            1
        );
    }
}
