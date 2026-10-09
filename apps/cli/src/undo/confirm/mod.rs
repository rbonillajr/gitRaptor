//! The requester's side of the confirmation (ADR-TMC-005 § 3): when the engine answers a Time
//! Machine write with a one-use challenge, show what would be undone, ask on the terminal and send
//! the same request once more, with the token, on the same connection. The asking is UX; the
//! checks are the engine's. The token travels only in the engine's answer and in the repeated
//! request: it is never printed, and it is stripped from every error this module hands back.

use std::io::IsTerminal;
use std::process::ExitCode;

use gitraptor_api::messages::RefusalReason;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{TmConfirmData, TmRejectReason};
use gitraptor_core::client::{Client, ClientError};

use crate::i18n::t;

/// Why the engine cannot take a confirmation here, as the commands explain it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CannotConfirm {
    Windows,
    Terminal,
    Agent,
    Unverified,
}

/// Maps the engine's refusal to the message family that explains it.
pub(crate) fn cannot_kind(reason: RefusalReason) -> CannotConfirm {
    match reason {
        RefusalReason::Unsupported => CannotConfirm::Windows,
        RefusalReason::NoControllingTerminal => CannotConfirm::Terminal,
        RefusalReason::AgentAncestry | RefusalReason::SessionLeaderAgent => CannotConfirm::Agent,
        _ => CannotConfirm::Unverified,
    }
}

/// The message key of a `confirmation-required` rejection in `group` (`undo` or `restore`):
/// why the engine gave no challenge, that `--json` cannot ask, or that the engine is too old to
/// say (it answered the old shape).
pub(crate) fn required_key(group: &str, data: Option<&TmConfirmData>) -> String {
    let tail = match data {
        Some(TmConfirmData {
            cannot_confirm: Some(reason),
            ..
        }) => match cannot_kind(*reason) {
            CannotConfirm::Windows => "confirmation-unavailable-windows",
            CannotConfirm::Terminal => "confirmation-needs-terminal",
            CannotConfirm::Agent => "confirmation-agent",
            CannotConfirm::Unverified => "confirmation-unverified",
        },
        Some(TmConfirmData {
            challenge: Some(_), ..
        }) => "confirmation-not-asked",
        _ => "confirmation-required",
    };
    format!("{group}.reason.{tail}")
}

/// The parameters of the repeated request: the same object plus `confirmation`.
pub(crate) fn resend_params(params: &serde_json::Value, token: &str) -> serde_json::Value {
    let mut again = params.clone();
    if let Some(object) = again.as_object_mut() {
        object.insert("confirmation".into(), token.into());
    }
    again
}

/// Whether the command asks the user: only a terminal session, never `--json`, and only when
/// the engine gave a challenge.
pub(crate) fn should_ask(interactive: bool, data: &TmConfirmData) -> bool {
    interactive && data.reason == TmRejectReason::ConfirmationRequired && data.challenge.is_some()
}

/// The rejection's `data` as a confirmation answer, if it is one.
pub(crate) fn confirm_data(err: &ClientError) -> Option<TmConfirmData> {
    match err {
        ClientError::Rpc(err) if err.code == code::OPERATION_REJECTED => err
            .data
            .clone()
            .and_then(|d| serde_json::from_value(d).ok()),
        _ => None,
    }
}

/// The error without the challenge: the token must not reach a log, a message or a pipe.
fn without_token(err: ClientError) -> ClientError {
    match err {
        ClientError::Rpc(mut rpc) => {
            if let Some(object) = rpc.data.as_mut().and_then(|d| d.as_object_mut()) {
                object.remove("challenge");
            }
            ClientError::Rpc(rpc)
        }
        other => other,
    }
}

fn prefix(method: &str) -> &'static str {
    match method {
        methods::TM_UNDO => "raptor undo",
        methods::TM_RESTORE => "raptor restore",
        _ => "raptor",
    }
}

/// Sends a Time Machine write. When the engine answers with a challenge and `interactive`,
/// shows the plan (`show_plan`), asks on the terminal and sends the same request once more with
/// the token on the same connection. `Err(code)`: not confirmed, already reported.
pub(crate) fn call(
    client: &mut Client,
    method: &str,
    params: serde_json::Value,
    interactive: bool,
    show_plan: &dyn Fn(&TmConfirmData),
) -> Result<Result<serde_json::Value, ClientError>, ExitCode> {
    let first = match client.call::<_, serde_json::Value>(method, params.clone()) {
        Ok(value) => return Ok(Ok(value)),
        Err(err) => err,
    };
    let challenge = confirm_data(&first).filter(|d| should_ask(interactive, d));
    let Some((data, token)) = challenge.and_then(|d| {
        let token = d.challenge.as_ref().map(|c| c.token.clone())?;
        Some((d, token))
    }) else {
        return Ok(Err(without_token(first)));
    };
    show_plan(&data);
    if !std::io::stdin().is_terminal() {
        eprintln!("{}: {}", prefix(method), t("tmconfirm.needs-terminal", &[]));
        return Err(ExitCode::FAILURE);
    }
    if !crate::confirm(&t("tmconfirm.ask", &[])) {
        eprintln!("{}: {}", prefix(method), t("tmconfirm.declined", &[]));
        return Err(ExitCode::FAILURE);
    }
    Ok(client
        .call::<_, serde_json::Value>(method, resend_params(&params, &token))
        .map_err(without_token))
}

#[cfg(test)]
mod tests;
