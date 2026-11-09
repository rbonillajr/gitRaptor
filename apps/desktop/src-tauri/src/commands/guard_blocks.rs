//! `guard_blocks`: ask the supervisor to read the open repo's guard log.

use serde::Deserialize;

use crate::engine::EngineHub;
use crate::ipc::{self, CommandError};

/// Params of `guard_blocks`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GuardBlocksParams {
    pub repo_id: String,
}

/// Asks the supervisor to read the guard log of `repoId`.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] for a bad body or id; [`CommandError::NotWatching`] when no
/// `engine_watch` started the supervisor.
#[tauri::command]
pub fn guard_blocks(
    request: tauri::ipc::Request<'_>,
    hub: tauri::State<'_, EngineHub>,
) -> Result<serde_json::Value, CommandError> {
    let params: GuardBlocksParams = ipc::params(&request)?;
    hub.guard_blocks(params.repo_id)?;
    Ok(serde_json::json!({}))
}
