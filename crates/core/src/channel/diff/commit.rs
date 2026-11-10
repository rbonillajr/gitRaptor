//! `diff.commit` and `diff.between`: validates the oids and the repo, queues the computation in
//! the query pool and answers, out of order, only to the connection that asked (DS-TS-CKP-006).
//!
//! Like `diff.worktree`, the answer carries file content: it is never logged, audited, published
//! or persisted. The log line holds the repo id, the duration, the number of files and the cut;
//! never an oid, a path or a line (D13).

use std::path::PathBuf;
use std::sync::Arc;

use gitraptor_api::diff::commit::{BetweenDiff, BetweenParams, CommitDiff, CommitParams};
use gitraptor_api::diff::{DiffSection, DiffUnavailable, MAX_DIFF_RESPONSE_BYTES};
use gitraptor_api::messages::{EngineStateView, RepoStateView};
use gitraptor_api::methods;
use gitraptor_api::rpc::{ErrorObject, Request, code};
use gitraptor_git::diff::{DiffLimits, Lookup, is_full_oid, relative_components};
use gitraptor_git::{ReadError, ReaderOptions, RepoReader};
use serde::Serialize;

use super::super::ServerCtx;
use super::super::bus::Outbox;
use super::super::query::QueryTable;
use super::fit::fit_sections;
use super::pool::{Answer, cut_name, queue};
use super::{ENVELOPE_MARGIN, invalid, not_found, section_of, unavailable, valid_repo_id};
use crate::daemon::now_ms;

/// The validated request.
enum Ask {
    Commit(String),
    Between {
        old: String,
        new: String,
        path: Option<String>,
    },
}

/// Serves `diff.commit` and `diff.between`: `Err` is the answer, at once; `Ok` means the query is
/// queued and its answer will come from the pool.
pub(in crate::channel) fn start(
    ctx: &ServerCtx,
    queries: &QueryTable,
    outbox: &Arc<Outbox>,
    request: &Request,
) -> Result<(), ErrorObject> {
    // Everything is validated before queueing; a bad input is always `invalid-params` (L-01).
    let (repo_id, ask) = if request.method == methods::DIFF_COMMIT {
        let p: CommitParams = request.params().map_err(|_| invalid())?;
        if !is_full_oid(&p.commit) {
            return Err(invalid());
        }
        (p.repo_id, Ask::Commit(p.commit))
    } else {
        let p: BetweenParams = request.params().map_err(|_| invalid())?;
        if !is_full_oid(&p.old) || !is_full_oid(&p.new) {
            return Err(invalid());
        }
        if let Some(path) = &p.path {
            relative_components(path).map_err(|_| invalid())?;
        }
        (
            p.repo_id,
            Ask::Between {
                old: p.old,
                new: p.new,
                path: p.path,
            },
        )
    };
    if !valid_repo_id(&repo_id) {
        return Err(invalid());
    }
    let common_dir = resolve_repo(ctx, &repo_id)?;
    match ask {
        Ask::Commit(commit) => queue(
            ctx,
            queries,
            outbox,
            request,
            "diff_commit",
            repo_id,
            move |limits, stop| commit_diff(&common_dir, &commit, limits, stop),
        ),
        Ask::Between { old, new, path } => queue(
            ctx,
            queries,
            outbox,
            request,
            "diff_between",
            repo_id,
            move |limits, stop| {
                between_diff(&common_dir, &old, &new, path.as_deref(), limits, stop)
            },
        ),
    }
}

/// The common directory of an observed repo, with the same immediate answers as `diff.worktree`:
/// unknown repo `not-found` (or `repo-unavailable` while the engine has not observed anything
/// yet), not ready `repo-unavailable`.
fn resolve_repo(ctx: &ServerCtx, repo_id: &str) -> Result<PathBuf, ErrorObject> {
    let (_, shared) = ctx.bus.snapshot();
    let Some(repo) = shared.repos.iter().find(|r| r.repo_id == repo_id) else {
        return Err(if shared.engine.state == EngineStateView::Observing {
            not_found()
        } else {
            unavailable(DiffUnavailable::RepoUnavailable)
        });
    };
    if repo.state != RepoStateView::Observed || repo.worktrees.is_empty() {
        return Err(unavailable(DiffUnavailable::RepoUnavailable));
    }
    Ok(PathBuf::from(repo.path.raw()))
}

fn open(common_dir: &std::path::Path) -> Result<RepoReader, ErrorObject> {
    RepoReader::open(common_dir, &ReaderOptions::default()).map_err(|e| match e {
        ReadError::Unavailable(_) | ReadError::Untrusted(_) | ReadError::NotARepository(_) => {
            unavailable(DiffUnavailable::RepoUnavailable)
        }
        _ => ErrorObject::new(code::INTERNAL, "diff failed"),
    })
}

/// `Missing` is `not-found`, the same for every reason (D5); a bad input is `invalid-params`.
fn read_error(e: &ReadError) -> ErrorObject {
    match e {
        ReadError::InvalidInput(_) => invalid(),
        _ => ErrorObject::new(code::INTERNAL, "diff failed"),
    }
}

/// Cuts the only section of an answer to the byte limit.
fn fit_one<T: Serialize>(answer: &mut T, section: impl Fn(&mut T) -> &mut DiffSection) {
    let Ok(whole) = serde_json::to_vec(answer) else {
        return;
    };
    let s = section(answer);
    fit_sections(
        whole.len(),
        vec![(&mut s.files, &mut s.cut)],
        MAX_DIFF_RESPONSE_BYTES - ENVELOPE_MARGIN,
    );
}

fn commit_diff(
    common_dir: &std::path::Path,
    commit: &str,
    limits: &DiffLimits,
    stop: &dyn Fn() -> bool,
) -> Result<Answer<CommitDiff>, ErrorObject> {
    let reader = open(common_dir)?;
    let git = match reader.commit_diff(commit, limits, stop) {
        Ok(Lookup::Found(git)) => git,
        Ok(Lookup::Missing) => return Err(not_found()),
        Err(e) => return Err(read_error(&e)),
    };
    let parents = u32::try_from(git.parents).unwrap_or(u32::MAX);
    let mut result = CommitDiff {
        computed_at_utc_ms: now_ms(),
        parents,
        merge: parents > 1,
        section: section_of(git.section),
    };
    fit_one(&mut result, |r| &mut r.section);
    Ok(Answer {
        files: result.section.files.len(),
        cut: cut_name(&[result.section.cut]),
        result,
    })
}

fn between_diff(
    common_dir: &std::path::Path,
    old: &str,
    new: &str,
    path: Option<&str>,
    limits: &DiffLimits,
    stop: &dyn Fn() -> bool,
) -> Result<Answer<BetweenDiff>, ErrorObject> {
    let reader = open(common_dir)?;
    let section = match reader.between_diff(old, new, path, limits, stop) {
        Ok(Lookup::Found(section)) => section,
        Ok(Lookup::Missing) => return Err(not_found()),
        Err(e) => return Err(read_error(&e)),
    };
    let mut result = BetweenDiff {
        computed_at_utc_ms: now_ms(),
        section: section_of(section),
    };
    fit_one(&mut result, |r| &mut r.section);
    Ok(Answer {
        files: result.section.files.len(),
        cut: cut_name(&[result.section.cut]),
        result,
    })
}
