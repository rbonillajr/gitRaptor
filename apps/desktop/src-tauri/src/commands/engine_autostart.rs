//! `engine_autostart`: enable or disable the login autostart after the human confirms in a
//! native dialog the webview cannot reach.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::engine::autostart::{self, Prompt};
use crate::engine::launcher;
use crate::ipc::{self, CommandError};

/// Params of `engine_autostart`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EngineAutostartParams {
    pub enable: bool,
}

/// One native dialog at a time: a second call while one is open fails instead of stacking dialogs
/// (a compromised webview cannot wear the human down with a loop of prompts).
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct InFlight;

impl InFlight {
    fn take() -> Option<Self> {
        IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// Recomputes the offer, asks the human and runs `raptor daemon enable|disable`.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] for a bad body; [`CommandError::NotOffered`] when the change
/// does not apply; [`CommandError::Cancelled`] on "Cancel"; [`CommandError::Failed`] when
/// `raptor` fails.
#[tauri::command]
pub async fn engine_autostart(
    app: tauri::AppHandle,
    request: tauri::ipc::Request<'_>,
) -> Result<serde_json::Value, CommandError> {
    let params: EngineAutostartParams = ipc::params(&request)?;
    let enable = params.enable;
    let in_flight = InFlight::take().ok_or(CommandError::Failed)?;
    let registered = tauri::async_runtime::spawn_blocking(move || {
        let _in_flight = in_flight;
        let raptor = launcher::bundled_raptor().ok_or(CommandError::Failed)?;
        let app_exe = std::env::current_exe().map_err(|_| CommandError::Failed)?;
        let report = launcher::run_ensure(&raptor, false).map_err(|_| CommandError::Failed)?;
        let current = autostart::offer(&app_exe, &report);
        // The offer is computed again after the "Yes" and before anything runs, so a change made
        // elsewhere while the dialog was open (another install's autostart) is never overwritten.
        let mut confirm = |prompt: &Prompt| {
            let yes = app
                .dialog()
                .message(prompt.body.clone())
                .title(prompt.title.clone())
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    prompt.accept.clone(),
                    prompt.cancel.clone(),
                ))
                .blocking_show();
            yes && launcher::run_ensure(&raptor, false)
                .is_ok_and(|report| autostart::offer(&app_exe, &report) == current)
        };
        autostart::run_choice(
            &raptor,
            enable,
            &current,
            autostart::system_lang(),
            &mut confirm,
        )
    })
    .await
    .map_err(|_| CommandError::Failed)??;
    Ok(serde_json::json!({ "registered": registered }))
}
