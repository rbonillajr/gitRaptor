//! `fleet_open`: open a repo's scope to follow its worktrees and sessions (DS-US-DSK-004 T002).

use serde::Deserialize;

/// Params of `fleet_open`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FleetOpenParams {
    pub repo_id: String,
}
