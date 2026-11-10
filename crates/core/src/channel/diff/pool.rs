//! What `diff.commit`, `diff.between` and `diff.index` share to run in the query pool: the
//! registration on the connection, the deadline, the caps and the log line (DS-TS-CKP-006 D1,
//! D13).

use std::sync::Arc;
use std::time::{Duration, Instant};

use gitraptor_api::diff::{
    DIFF_DEADLINE_MS, DiffCut, MAX_DIFF_BLOB_BYTES, MAX_DIFF_FILES, MAX_DIFF_LINES_PER_FILE,
    MAX_DIFF_RESPONSE_BYTES,
};
use gitraptor_api::rpc::{ErrorObject, Request};
use gitraptor_git::diff::DiffLimits;
use serde::Serialize;

use super::super::ServerCtx;
use super::super::bus::Outbox;
use super::super::query::{QueryTable, QueryToken};
use crate::daemon::Field;

/// What one query answers and what its log line says: files in the answer and the first cut.
pub(super) struct Answer<T> {
    pub(super) result: T,
    pub(super) files: usize,
    pub(super) cut: &'static str,
}

/// Registers the query on the connection and queues `work` in the pool, with the deadline of
/// `diff.worktree` (overridable in debug builds, for the tests). `event` names the log line,
/// which holds only the repo id, the duration, the number of files and the cut.
pub(super) fn queue<T: Serialize + 'static>(
    ctx: &ServerCtx,
    queries: &QueryTable,
    outbox: &Arc<Outbox>,
    request: &Request,
    event: &'static str,
    repo_id: String,
    work: impl FnOnce(&DiffLimits, &dyn Fn() -> bool) -> Result<Answer<T>, ErrorObject> + Send + 'static,
) -> Result<(), ErrorObject> {
    let token = queries.register(&request.id).map_err(|busy| busy.error())?;
    let (table, outbox, id) = (queries.clone(), Arc::clone(outbox), request.id.clone());
    let logger = ctx.logger.clone();
    let job_token = token.clone();
    let deadline = Duration::from_millis(DIFF_DEADLINE_MS);
    #[cfg(debug_assertions)]
    let deadline = ctx.config.test_diff_deadline.unwrap_or(deadline);
    let job = Box::new(move || {
        let started = Instant::now();
        table.run(&outbox, id.clone(), &job_token, |token: &QueryToken| {
            let limits = limits(started + deadline);
            let answer = work(&limits, &|| token.is_cancelled())?;
            let ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            logger.info(
                event,
                &[
                    ("repo", Field::Id(repo_id)),
                    ("ms", Field::Int(ms)),
                    (
                        "files",
                        Field::Int(i64::try_from(answer.files).unwrap_or(i64::MAX)),
                    ),
                    ("cut", Field::Text(answer.cut)),
                ],
            );
            Ok(answer.result)
        });
    });
    if let Err(busy) = ctx.query.submit(job) {
        queries.finish(&request.id);
        return Err(busy.error());
    }
    Ok(())
}

/// The caps of US-CKP-012, ending at `deadline`.
fn limits(deadline: Instant) -> DiffLimits {
    DiffLimits {
        max_files: MAX_DIFF_FILES,
        max_lines_per_file: MAX_DIFF_LINES_PER_FILE,
        max_blob_bytes: MAX_DIFF_BLOB_BYTES,
        max_bytes: MAX_DIFF_RESPONSE_BYTES,
        deadline,
    }
}

/// The log name of the first cut of `cuts`, or `none`.
pub(super) fn cut_name(cuts: &[Option<DiffCut>]) -> &'static str {
    cuts.iter()
        .flatten()
        .map(|c| match c {
            DiffCut::Files => "files",
            DiffCut::Bytes => "bytes",
            DiffCut::Time => "time",
        })
        .next()
        .unwrap_or("none")
}
