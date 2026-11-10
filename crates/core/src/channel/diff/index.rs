//! `diff.index`: resolves the worktree like `diff.worktree`, queues the computation in the query
//! pool and answers, out of order, only to the connection that asked (DS-TS-CKP-006 D7).
//!
//! The answer carries file content: never logged, audited, published or persisted (D13).

use std::sync::Arc;

use gitraptor_api::Untrusted;
use gitraptor_api::diff::index::{
    CONFLICTS_RESERVE_BYTES, ContentId, IndexDiff, IndexFileDiff, IndexSection,
};
use gitraptor_api::diff::{DiffUnavailable, MAX_DIFF_FILES, MAX_DIFF_RESPONSE_BYTES};
use gitraptor_api::rpc::{ErrorObject, Request, code};
use gitraptor_git::ReadError;
use gitraptor_git::diff::{DiffLimits, GitContentIds, GitIndexSection};

use super::super::ServerCtx;
use super::super::bus::Outbox;
use super::super::query::QueryTable;
use super::commit::{Answer, cut_name, queue};
use super::fit::fit_sections;
use super::{ENVELOPE_MARGIN, Resolved, file_of, resolve, section_of, unavailable};
use crate::daemon::now_ms;

/// Serves `diff.index`: `Err` is the answer, at once; `Ok` means the query is queued and its
/// answer will come from the pool.
pub(in crate::channel) fn start(
    ctx: &ServerCtx,
    queries: &QueryTable,
    outbox: &Arc<Outbox>,
    request: &Request,
) -> Result<(), ErrorObject> {
    let resolved = resolve(ctx, request)?;
    let repo_id = resolved.repo_id.clone();
    queue(
        ctx,
        queries,
        outbox,
        request,
        "diff_index",
        repo_id,
        move |limits, stop| compute(&resolved, limits, stop),
    )
}

fn compute(
    resolved: &Resolved,
    limits: &DiffLimits,
    stop: &dyn Fn() -> bool,
) -> Result<Answer<IndexDiff>, ErrorObject> {
    let reader = crate::observe::open_worktree(
        &resolved.common_dir,
        &resolved.root,
        resolved.main,
        resolved.admin_name.as_deref(),
    )
    .map_err(|e| match e {
        ReadError::Unavailable(_) | ReadError::Untrusted(_) | ReadError::NotARepository(_) => {
            unavailable(DiffUnavailable::WorktreeUnavailable)
        }
        _ => ErrorObject::new(code::INTERNAL, "diff failed"),
    })?;
    let git = reader.index_diff(limits, stop).map_err(|e| match e {
        // The index kept changing while it was read (L-02).
        ReadError::TemporarilyUnavailable(_) => unavailable(DiffUnavailable::WorktreeUnavailable),
        _ => ErrorObject::new(code::INTERNAL, "diff failed"),
    })?;
    let mut result = IndexDiff {
        computed_at_utc_ms: now_ms(),
        staged: index_section(git.staged),
        unstaged: index_section(git.unstaged),
        conflicts: reserved_conflicts(git.conflicts),
        conflicts_total: u32::try_from(git.conflicts_total).unwrap_or(u32::MAX),
    };
    // M-01: `conflicts` took its reserve first; the sections share what is left.
    if let Ok(whole) = serde_json::to_vec(&result) {
        let IndexDiff {
            staged, unstaged, ..
        } = &mut result;
        fit_sections(
            whole.len(),
            vec![
                (&mut staged.files, &mut staged.cut),
                (&mut unstaged.files, &mut unstaged.cut),
            ],
            MAX_DIFF_RESPONSE_BYTES - ENVELOPE_MARGIN,
        );
    }
    Ok(Answer {
        files: result.staged.files.len() + result.unstaged.files.len(),
        cut: cut_name(&[result.staged.cut, result.unstaged.cut]),
        result,
    })
}

/// The conflicted paths, in order, while their serialized list fits in
/// [`CONFLICTS_RESERVE_BYTES`] and [`MAX_DIFF_FILES`]; `conflicts_total` keeps counting the rest.
fn reserved_conflicts(paths: Vec<String>) -> Vec<Untrusted> {
    // `[` and `]`, then each entry and its comma.
    let mut used: usize = 2;
    let mut kept = Vec::new();
    for path in paths.into_iter().take(MAX_DIFF_FILES) {
        let path = Untrusted::new(path);
        let size = serde_json::to_vec(&path).map_or(usize::MAX, |b| b.len()) + 1;
        if used.saturating_add(size) > CONFLICTS_RESERVE_BYTES {
            break;
        }
        used += size;
        kept.push(path);
    }
    kept
}

fn index_section(git: GitIndexSection) -> IndexSection {
    let GitIndexSection { mut section, mut ids } = git;
    let files = std::mem::take(&mut section.files)
        .into_iter()
        .map(|file| {
            let GitContentIds { old, new } = ids.remove(&file.path).unwrap_or_default();
            IndexFileDiff {
                file: file_of(file),
                old: content_id(old),
                new: content_id(new),
            }
        })
        .collect();
    let section = section_of(section);
    IndexSection {
        files,
        files_total: section.files_total,
        cut: section.cut,
    }
}

fn content_id(id: Option<String>) -> ContentId {
    match id {
        Some(id) => ContentId::Blob { id },
        None => ContentId::None {},
    }
}
