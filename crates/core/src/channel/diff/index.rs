//! `diff.index`: resolves the worktree like `diff.worktree`, queues the computation in the query
//! pool and answers, out of order, only to the connection that asked (DS-TS-CKP-006).

use std::sync::Arc;

use gitraptor_api::rpc::{ErrorObject, Request, code};

use super::super::ServerCtx;
use super::super::bus::Outbox;
use super::super::query::QueryTable;

/// Serves `diff.index`: `Err` is the answer, at once; `Ok` means the query is queued and its
/// answer will come from the pool.
pub(in crate::channel) fn start(
    _ctx: &ServerCtx,
    _queries: &QueryTable,
    _outbox: &Arc<Outbox>,
    _request: &Request,
) -> Result<(), ErrorObject> {
    Err(ErrorObject::new(code::INTERNAL, "not implemented"))
}
