//! `engine_watch`: hands the webview's `Channel` to the engine supervisor, which pushes every
//! `EngineMessage` through it. Called once per webview load; a later call replaces the channel.

use serde::Deserialize;
use tauri::ipc::JavaScriptChannelId;

use crate::engine::EngineHub;
use crate::engine::supervisor::Sink;
use crate::ipc::{self, CommandError};

/// Params of `engine_watch`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EngineWatchParams {
    pub channel: JavaScriptChannelId,
}

/// Starts (or redirects) the supervisor and streams its messages to the webview.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] when the body is not `{"params": {"channel": …}}`.
#[tauri::command]
pub fn engine_watch(
    request: tauri::ipc::Request<'_>,
    webview: tauri::Webview,
    hub: tauri::State<'_, EngineHub>,
) -> Result<serde_json::Value, CommandError> {
    let params: EngineWatchParams = ipc::params(&request)?;
    hub.watch(Sink::Channel(params.channel.channel_on(webview)));
    Ok(serde_json::json!({}))
}
