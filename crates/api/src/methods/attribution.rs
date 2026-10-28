//! Corrections of a detected attribution: reserved to the developer and never offered to
//! `raptor-mcp` (types in [`crate::attribution`]).

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, method};
use crate::rpc::ErrorSpec;

/// Corrects the agent of a detected session, from its start.
pub const ATTRIBUTION_CORRECT: &str = "attribution.correct";
/// Withdraws the active correction of a session: it returns to the detected attribution.
pub const ATTRIBUTION_WITHDRAW: &str = "attribution.withdraw-correction";

// Eight blocks after `FIRST_ERROR_BLOCK` (ADR-GRP-016 § 3).
const BLOCK: i64 = FIRST_ERROR_BLOCK - 8 * ERROR_BLOCK_LEN;

/// A correction or its withdrawal changed nothing: `data` is
/// [`crate::attribution::AttributionRejectedData`].
pub const ATTRIBUTION_REJECTED: ErrorSpec = ErrorSpec::new(BLOCK, "attribution-rejected");

pub(super) const GROUP: Group = Group {
    methods: &[
        method(ATTRIBUTION_CORRECT, true, false),
        method(ATTRIBUTION_WITHDRAW, true, false),
    ],
    error_block: Some(BLOCK),
    errors: &[ATTRIBUTION_REJECTED],
    ..Group::new("attribution")
};
