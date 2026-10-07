//! The MCP allowlist in the daemon's loop (US-MCP-002, ADR-GRP-006
//! Enmienda (2026-10-05, MCP)): the loop owns the profile, so it is the only
//! writer of the mark; the channel reads the set in memory.

use std::path::Path;

use gitraptor_api::methods::McpRepoResult;

use super::{Daemon, Field, now_ms, profile_error_kind};
use crate::profile::RepoState;

/// Why the loop did not change the mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMarkError {
    /// The repo is not observed: observe it first.
    NotObserved,
    /// The profile could not be written (or the loop did not answer).
    Internal,
}

/// The client kind the mark records: only the developer's CLI or TUI pass
/// the reserved checks.
const MARKED_BY: &str = "cli";

impl Daemon {
    /// Puts (`enabled`) or takes the MCP mark of the observed repo of
    /// `common_dir`. Idempotent.
    pub(super) fn mcp_mark(
        &mut self,
        common_dir: &Path,
        enabled: bool,
    ) -> Result<McpRepoResult, McpMarkError> {
        let failed = |daemon: &Self, err: &crate::profile::ProfileError| {
            daemon.logger.error(
                "mcp_mark_failed",
                &[("error", profile_error_kind(err).into())],
            );
            McpMarkError::Internal
        };
        let entry = self
            .profile
            .repo_by_common_dir(common_dir)
            .map_err(|err| failed(self, &err))?
            .filter(|e| e.state == RepoState::Observed)
            .ok_or(McpMarkError::NotObserved)?;
        let changed = self
            .profile
            .set_mcp_enabled(&entry.repo_id, enabled, MARKED_BY, now_ms())
            .map_err(|err| failed(self, &err))?
            .ok_or(McpMarkError::NotObserved)?;
        self.mcp_repos.set(&entry.repo_id, enabled);
        if changed {
            self.logger.info(
                if enabled {
                    "mcp_enabled"
                } else {
                    "mcp_disabled"
                },
                &[("repo", Field::id(&entry.repo_id))],
            );
        }
        Ok(McpRepoResult {
            repo_id: entry.repo_id,
            enabled,
            changed,
        })
    }
}
