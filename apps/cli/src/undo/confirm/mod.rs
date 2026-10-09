//! The requester's side of the confirmation (ADR-TMC-005 § 3): when the engine answers a Time
//! Machine write with a one-use challenge, show what would be undone, ask on the terminal and send
//! the same request once more, with the token, on the same connection. The asking is UX; the
//! checks are the engine's.

use std::process::ExitCode;

use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::TmConfirmData;
use gitraptor_core::client::{Client, ClientError};

/// Why the engine cannot take a confirmation here, as the commands explain it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // stub: the commands pick their message from it in the next phase
pub(crate) enum CannotConfirm {
    Windows,
    Terminal,
    Agent,
    Unverified,
}

/// Maps the engine's refusal to the message family that explains it.
#[allow(dead_code)] // stub
pub(crate) fn cannot_kind(_reason: RefusalReason) -> CannotConfirm {
    CannotConfirm::Unverified
}

/// The parameters of the repeated request: the same object plus `confirmation`.
#[allow(dead_code)] // stub
pub(crate) fn resend_params(params: &serde_json::Value, _token: &str) -> serde_json::Value {
    params.clone()
}

/// Whether the command asks the user: only a terminal session, never `--json`, and only when
/// the engine gave a challenge.
#[allow(dead_code)] // stub
pub(crate) fn should_ask(_interactive: bool, _data: &TmConfirmData) -> bool {
    false
}

/// Sends a Time Machine write. When the engine answers with a challenge and `interactive`,
/// shows the plan (`show_plan`), asks on the terminal and sends the same request once more with
/// the token on the same connection. `Err(code)`: not confirmed, already reported.
#[allow(dead_code)] // stub
pub(crate) fn call(
    _client: &mut Client,
    _method: &str,
    _params: serde_json::Value,
    _interactive: bool,
    _show_plan: &dyn Fn(&TmConfirmData),
) -> Result<Result<serde_json::Value, ClientError>, ExitCode> {
    todo!("phase B: challenge, ask, repeat with the token")
}

#[cfg(test)]
mod tests;
