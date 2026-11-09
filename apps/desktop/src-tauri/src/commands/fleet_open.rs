//! `fleet_open`: open a repo's scope to follow its worktrees and sessions (DS-US-DSK-004 T002).

use serde::Deserialize;

use crate::engine::EngineHub;
use crate::ipc::{self, CommandError};

/// Params of `fleet_open`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FleetOpenParams {
    pub repo_id: String,
}

/// Asks the supervisor to follow `repoId`.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] when the body is not `{"params": {"repoId": string}}` or the
/// id is empty, over 128 bytes or has control characters; [`CommandError::NotWatching`] when no
/// `engine_watch` started the supervisor.
#[tauri::command]
pub fn fleet_open(
    request: tauri::ipc::Request<'_>,
    hub: tauri::State<'_, EngineHub>,
) -> Result<serde_json::Value, CommandError> {
    let params: FleetOpenParams = ipc::params(&request)?;
    hub.open(params.repo_id)?;
    Ok(serde_json::json!({}))
}
