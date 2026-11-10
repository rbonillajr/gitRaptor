//! The diff of a worktree, on demand (types in [`crate::diff`]). Never offered to `raptor-mcp`:
//! the content of a diff does not leave through the MCP (Q-CKP-8).

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, method};
use crate::capability::CAPABILITIES_PROTOCOL;
use crate::rpc::ErrorSpec;

/// What a worktree would integrate and what it has uncommitted. Answered out of order.
pub const DIFF_WORKTREE: &str = "diff.worktree";

// Seven blocks after `FIRST_ERROR_BLOCK` (ADR-GRP-016 § 3).
const BLOCK: i64 = FIRST_ERROR_BLOCK - 7 * ERROR_BLOCK_LEN;

/// The diff cannot be computed now: `data` is [`crate::diff::DiffUnavailableData`].
pub const DIFF_UNAVAILABLE: ErrorSpec = ErrorSpec::new(BLOCK, "diff-unavailable");

pub(super) const GROUP: Group = Group {
    methods: &[method(DIFF_WORKTREE, false, false).since(CAPABILITIES_PROTOCOL)],
    error_block: Some(BLOCK),
    errors: &[DIFF_UNAVAILABLE],
    ..Group::new("diff")
};
