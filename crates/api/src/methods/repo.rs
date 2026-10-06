//! The observed repos (US-GRP-001).

use super::{Group, method};

pub const REPO_ADD: &str = "repo.add";
pub const REPO_RETIRE: &str = "repo.retire";
/// The observed repo and worktree that contain a path (protocol 6, N4).
pub const REPO_LOCATE: &str = "repo.locate";

pub(super) const GROUP: Group = Group {
    methods: &[
        method(REPO_ADD, true, false),
        // US-GRP-001 stops the observation; US-GRP-006 adds the history kept
        // and recovered.
        method(REPO_RETIRE, true, false),
        // Cockpit (ADR-CKP-003 § 4): carries paths, so not offered to
        // `raptor-mcp` (SEC-12).
        method(REPO_LOCATE, false, false).since(6),
    ],
    ..Group::new("repo")
};
