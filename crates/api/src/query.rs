//! On-demand queries (N8 of ADR-CKP-003 § 4): the cancellation every query shares.
//!
//! A query answers out of order, by the id of its request, while the connection keeps reading
//! requests and the bus keeps pushing events. `query.cancel` stops one of them; the cancelled
//! request itself is answered with `query-cancelled`. Diff, timeline and graph queries reuse
//! this module instead of declaring their own cancellation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rpc::Id;

/// `query.cancel` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelParams {
    /// The JSON-RPC id of the request to cancel, from the same connection.
    pub request: Id,
}

/// `query.cancel` result: answered at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelResult {
    /// The request was in flight and is now cancelled; `false` when it was not (not an error).
    pub cancelled: bool,
}

/// `data` of `query-busy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryBusyData {
    /// Queries one connection may have in flight.
    pub limit: u32,
}
