//! `engine_retry`: connect to the engine now; with `start`, start it first.

use serde::Deserialize;

use crate::engine::EngineHub;
use crate::ipc::{self, CommandError};

/// Params of `engine_retry`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EngineRetryParams {
    pub start: bool,
}

/// Asks the supervisor to reconnect right away.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] when the body is not `{"params": {"start": bool}}`;
/// [`CommandError::NotWatching`] when no `engine_watch` started the supervisor.
#[tauri::command]
pub fn engine_retry(
    request: tauri::ipc::Request<'_>,
    hub: tauri::State<'_, EngineHub>,
) -> Result<serde_json::Value, CommandError> {
    let params: EngineRetryParams = ipc::params(&request)?;
    hub.retry(params.start)
        .map_err(|_| CommandError::NotWatching)?;
    Ok(serde_json::json!({}))
}
