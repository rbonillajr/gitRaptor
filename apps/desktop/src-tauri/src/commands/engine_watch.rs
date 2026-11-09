//! `engine_watch`: STUB of the parameter type only (DS-US-DSK-003 T005); the command comes later.

use serde::Deserialize;
use tauri::ipc::JavaScriptChannelId;

/// Params of `engine_watch`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineWatchParams {
    pub channel: JavaScriptChannelId,
}
