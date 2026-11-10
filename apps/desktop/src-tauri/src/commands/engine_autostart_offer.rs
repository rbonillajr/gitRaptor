//! `engine_autostart_offer`: whether the app offers to start the engine at login.

use serde::Deserialize;
use serde::Serialize;

use crate::engine::autostart::{self, NotOfferedReason, Offer};
use crate::engine::launcher;
use crate::ipc::{self, CommandError};

/// Params of `engine_autostart_offer`: none.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineAutostartOfferParams {}

/// The reply: `{ "offer": "offered"|"registered"|"not-offered", "reason": "location"|"other-binary"|null }`.
#[derive(Debug, Serialize)]
pub struct OfferReply {
    offer: &'static str,
    reason: Option<&'static str>,
}

impl From<&Offer> for OfferReply {
    fn from(offer: &Offer) -> Self {
        match offer {
            Offer::Offered => Self {
                offer: "offered",
                reason: None,
            },
            Offer::Registered => Self {
                offer: "registered",
                reason: None,
            },
            Offer::NotOffered { reason } => Self {
                offer: "not-offered",
                reason: Some(match reason {
                    NotOfferedReason::Location => "location",
                    NotOfferedReason::OtherBinary => "other-binary",
                }),
            },
        }
    }
}

/// The current offer, from `raptor daemon ensure --no-start` of the bundle.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] when the body is not `{"params": {}}`;
/// [`CommandError::Failed`] when the bundle's `raptor` cannot report.
#[tauri::command]
pub async fn engine_autostart_offer(
    request: tauri::ipc::Request<'_>,
) -> Result<serde_json::Value, CommandError> {
    let _: EngineAutostartOfferParams = ipc::params(&request)?;
    // `ensure` can take up to 10 s: off the main thread, so the window never freezes.
    let offer = tauri::async_runtime::spawn_blocking(current_offer)
        .await
        .map_err(|_| CommandError::Failed)??
        .1;
    serde_json::to_value(OfferReply::from(&offer)).map_err(|_| CommandError::Failed)
}

/// The bundle's `raptor` and the offer computed from its `ensure --no-start` report.
fn current_offer() -> Result<(std::path::PathBuf, Offer), CommandError> {
    let raptor = launcher::bundled_raptor().ok_or(CommandError::Failed)?;
    let app_exe = std::env::current_exe().map_err(|_| CommandError::Failed)?;
    let report = launcher::run_ensure(&raptor, false).map_err(|_| CommandError::Failed)?;
    let offer = autostart::offer(&app_exe, &report);
    Ok((raptor, offer))
}
