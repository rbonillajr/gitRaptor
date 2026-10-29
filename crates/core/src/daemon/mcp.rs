//! The MCP allowlist in the daemon's loop (US-MCP-002, ADR-GRP-006
//! Enmienda (2026-10-05, MCP)): the loop owns the profile, so it is the only
//! writer of the mark; the channel reads the set in memory.

use std::path::Path;

use gitraptor_api::guard::ProtectionState;
use gitraptor_api::messages::SessionsListParams;
use gitraptor_api::methods::McpRepoResult;

use super::{Daemon, Field, McpContext, now_ms, profile_error_kind};
use crate::guardrails::install as guard_install;
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
            self.log_allowlist_change(&entry.repo_id, &entry.canonical_path, enabled);
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

    /// Records the change of the mark as a transition of the protection state (BR-WF-002): the
    /// hooks half is read from the repo's install, the MCP half is the mark before and after. A
    /// failure to write is logged and never undoes the mark. The developer made the change with a
    /// reserved command, so there is no alert.
    fn log_allowlist_change(&mut self, repo_id: &str, common: &Path, enabled: bool) {
        self.wake_for_request(repo_id);
        let Some((_, store)) = self.stores.iter_mut().find(|(id, _)| id == repo_id) else {
            self.logger
                .error("guard_mcp_log_failed", &[("repo", Field::id(repo_id))]);
            return;
        };
        let hooks = guard_install::status(repo_id, common, store)
            .state
            .hooks_installed();
        let (now, offset) = crate::watch::wall_now();
        let entry = crate::guardrails::log::mcp_allowlist_entry(
            &common.to_string_lossy(),
            now,
            offset,
            ProtectionState::of(hooks, !enabled),
            ProtectionState::of(hooks, enabled),
        );
        if store.record_guard_decision(&entry).is_err() {
            self.logger
                .error("guard_mcp_log_failed", &[("repo", Field::id(repo_id))]);
        }
    }

    /// What the full `mcp.status` reads of an observed repo: its present sessions, its gaps and
    /// its protection status. Read only; `None` if the repo or its store is not there.
    pub(super) fn mcp_context(&self, repo_id: &str) -> Option<McpContext> {
        let entry = self
            .profile
            .repo(repo_id)
            .ok()
            .flatten()
            .filter(|e| e.state == RepoState::Observed)?;
        let (_, store) = self.stores.iter().find(|(id, _)| id == repo_id)?;
        let (detection_available, sessions) = self
            .sessions_list(&SessionsListParams {
                repo_id: Some(repo_id.to_owned()),
                include_ended: false,
                limit: None,
            })
            .ok()?;
        let mut guard = guard_install::status(repo_id, entry.canonical_path.as_path(), store);
        // The same function as `guard.status`: the MCP view and the CLI never disagree.
        guard.state =
            ProtectionState::of(guard.state.hooks_installed(), self.mcp_repos.contains(repo_id));
        Some(McpContext {
            detection_available,
            sessions,
            gaps: store.gaps().ok()?,
            guard,
        })
    }
}
