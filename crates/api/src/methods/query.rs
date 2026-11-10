//! On-demand queries: the cancellation and the codes every query shares (types in
//! [`crate::query`]). Never offered to `raptor-mcp`.

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, method};
use crate::capability::CAPABILITIES_PROTOCOL;
use crate::rpc::ErrorSpec;

/// Cancels a query in flight of the same connection.
pub const QUERY_CANCEL: &str = "query.cancel";

// Six blocks after `FIRST_ERROR_BLOCK` (ADR-GRP-016 § 3).
const BLOCK: i64 = FIRST_ERROR_BLOCK - 6 * ERROR_BLOCK_LEN;

/// The connection already has a query in flight, or the shared queue is full: `data` is
/// [`crate::query::QueryBusyData`].
pub const QUERY_BUSY: ErrorSpec = ErrorSpec::new(BLOCK, "query-busy");
/// The query was cancelled by `query.cancel` or by its connection closing.
pub const QUERY_CANCELLED: ErrorSpec = ErrorSpec::new(BLOCK - 1, "query-cancelled");

pub(super) const GROUP: Group = Group {
    methods: &[method(QUERY_CANCEL, false, false).since(CAPABILITIES_PROTOCOL)],
    error_block: Some(BLOCK),
    errors: &[QUERY_BUSY, QUERY_CANCELLED],
    ..Group::new("query")
};
