//! `diff.worktree`: resolves the worktree and the confirmed base in the engine's state, queues
//! the computation in the query pool and answers, out of order, to the connection that asked.
//!
//! The answer carries file content: it goes only to that connection's outbox. It is never
//! logged, audited, published on the bus or written to the profile. The log line of a query holds
//! the repo id, the duration, the number of files and the cut, nothing else.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitraptor_api::diff::{
    CommittedDiff, CommittedGap, DIFF_DEADLINE_MS, DiffCut, DiffLineView, DiffParams, DiffSection,
    DiffUnavailable, DiffUnavailableData, FileBody, FileChange, FileDiff, Hunk, LineKind,
    MAX_DIFF_BLOB_BYTES, MAX_DIFF_FILES, MAX_DIFF_LINES_PER_FILE, MAX_DIFF_RESPONSE_BYTES,
    WorktreeDiff,
};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, EngineStateView, RepoStateView, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::DIFF_UNAVAILABLE;
use gitraptor_api::rpc::{ErrorObject, Request, code};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_git::ReadError;
use gitraptor_git::RefName;
use gitraptor_git::diff::{
    CommittedGapKind, CutKind, DiffLimits, DiffRequest, GitBody, GitChange, GitFileDiff,
    GitLineKind, GitSection,
};

use super::ServerCtx;
use super::bus::Outbox;
use super::query::QueryTable;
use super::validate;
use crate::daemon::{Field, now_ms};

pub(super) mod commit;
pub(super) mod index;

/// Slack under [`MAX_DIFF_RESPONSE_BYTES`] for the envelope of the response.
const ENVELOPE_MARGIN: usize = 256;

fn invalid() -> ErrorObject {
    ErrorObject::new(code::INVALID_PARAMS, "invalid params")
}

fn not_found() -> ErrorObject {
    ErrorObject::new(code::NOT_FOUND, "not found")
}

fn unavailable(reason: DiffUnavailable) -> ErrorObject {
    ErrorObject::new(DIFF_UNAVAILABLE.code, DIFF_UNAVAILABLE.name)
        .with_data(DiffUnavailableData { reason })
}

fn valid_repo_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// What the arm of `dispatch` resolved: owned data only, so the job outlives the connection's
/// reader.
struct Resolved {
    repo_id: String,
    common_dir: PathBuf,
    root: PathBuf,
    main: bool,
    admin_name: Option<String>,
    base: Option<RefName>,
    base_name: Option<String>,
    base_confirmed: bool,
}

fn resolve(ctx: &ServerCtx, request: &Request) -> Result<Resolved, ErrorObject> {
    let params: DiffParams = request.params()?;
    if !valid_repo_id(&params.repo_id) {
        return Err(invalid());
    }
    let root = validate::client_path(&params.worktree).map_err(|_| invalid())?;
    let (_, shared) = ctx.bus.snapshot();
    let Some(repo) = shared.repos.iter().find(|r| r.repo_id == params.repo_id) else {
        // The engine has not observed anything yet: the repo may be about to appear.
        return Err(if shared.engine.state == EngineStateView::Observing {
            not_found()
        } else {
            unavailable(DiffUnavailable::RepoUnavailable)
        });
    };
    if repo.state != RepoStateView::Observed || repo.worktrees.is_empty() {
        return Err(unavailable(DiffUnavailable::RepoUnavailable));
    }
    let worktree: &WorktreeView = repo
        .worktrees
        .iter()
        .find(|w| Path::new(w.path.raw()) == root)
        .ok_or_else(not_found)?;
    if matches!(worktree.status, WorktreeStatus::Unavailable { .. }) {
        return Err(unavailable(DiffUnavailable::WorktreeUnavailable));
    }
    let BaseBranchView { name, status } = &repo.base;
    let base_name = name.as_ref().map(|n| n.raw().to_owned());
    Ok(Resolved {
        repo_id: repo.repo_id.clone(),
        common_dir: PathBuf::from(repo.path.raw()),
        root,
        main: worktree.main,
        admin_name: worktree.admin_name.as_ref().map(|n| n.raw().to_owned()),
        base: base_name
            .as_ref()
            .and_then(|n| RefName::new(&format!("refs/heads/{n}")).ok()),
        base_name,
        base_confirmed: *status == BaseStatusView::Confirmed,
    })
}

/// Serves `diff.worktree`: `Err` is the answer, at once; `Ok` means the query is queued and its
/// answer will come from the pool.
pub(super) fn start(
    ctx: &ServerCtx,
    queries: &QueryTable,
    outbox: &Arc<Outbox>,
    request: &Request,
) -> Result<(), ErrorObject> {
    let resolved = resolve(ctx, request)?;
    let token = queries.register(&request.id).map_err(|busy| busy.error())?;
    let (table, outbox, id) = (queries.clone(), Arc::clone(outbox), request.id.clone());
    let logger = ctx.logger.clone();
    let job_token = token.clone();
    let deadline = Duration::from_millis(DIFF_DEADLINE_MS);
    #[cfg(debug_assertions)]
    let deadline = ctx.config.test_diff_deadline.unwrap_or(deadline);
    let job = Box::new(move || {
        let started = Instant::now();
        table.run(&outbox, id.clone(), &job_token, |token| {
            let diff = compute(&resolved, deadline, &|| token.is_cancelled())?;
            let (files, cut) = summary(&diff);
            logger.info(
                "diff_worktree",
                &[
                    ("repo", Field::Id(resolved.repo_id.clone())),
                    (
                        "ms",
                        Field::Int(
                            i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
                        ),
                    ),
                    ("files", Field::Int(i64::from(files))),
                    ("cut", Field::Text(cut)),
                ],
            );
            Ok(diff)
        });
    });
    if let Err(busy) = ctx.query.submit(job) {
        queries.finish(&request.id);
        return Err(busy.error());
    }
    Ok(())
}

fn compute(
    resolved: &Resolved,
    deadline: Duration,
    stop: &dyn Fn() -> bool,
) -> Result<WorktreeDiff, ErrorObject> {
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
    let limits = DiffLimits {
        max_files: MAX_DIFF_FILES,
        max_lines_per_file: MAX_DIFF_LINES_PER_FILE,
        max_blob_bytes: MAX_DIFF_BLOB_BYTES,
        max_bytes: MAX_DIFF_RESPONSE_BYTES,
        deadline: Instant::now() + deadline,
    };
    let request = DiffRequest {
        base: resolved.base.as_ref(),
        base_confirmed: resolved.base_confirmed,
    };
    let git = reader
        .worktree_diff(&request, &limits, stop)
        .map_err(|_| ErrorObject::new(code::INTERNAL, "diff failed"))?;
    let committed = match git.committed {
        Ok(section) => CommittedDiff::Ready {
            base: UntrustedName::new(resolved.base_name.clone().unwrap_or_default()),
            section: section_of(section),
        },
        Err(gap) => CommittedDiff::NotComputable {
            reason: match gap {
                CommittedGapKind::BaseUnconfirmed => CommittedGap::BaseUnconfirmed,
                CommittedGapKind::NoBase => CommittedGap::NoBase,
                CommittedGapKind::BaseMissing => CommittedGap::BaseMissing,
                CommittedGapKind::NoMergeBase => CommittedGap::NoMergeBase,
                CommittedGapKind::TooSlow => CommittedGap::TooSlow,
            },
        },
    };
    let mut diff = WorktreeDiff {
        computed_at_utc_ms: now_ms(),
        committed,
        uncommitted: section_of(git.uncommitted),
    };
    fit(&mut diff, MAX_DIFF_RESPONSE_BYTES - ENVELOPE_MARGIN);
    Ok(diff)
}

fn section_of(section: GitSection) -> DiffSection {
    DiffSection {
        files: section.files.into_iter().map(file_of).collect(),
        files_total: u32::try_from(section.files_total).unwrap_or(u32::MAX),
        cut: section.cut.map(|c| match c {
            CutKind::Files => DiffCut::Files,
            CutKind::Bytes => DiffCut::Bytes,
            CutKind::Time => DiffCut::Time,
        }),
    }
}

fn file_of(file: GitFileDiff) -> FileDiff {
    FileDiff {
        path: Untrusted::new(file.path),
        change: match file.change {
            GitChange::Added => FileChange::Added,
            GitChange::Modified => FileChange::Modified,
            GitChange::Deleted => FileChange::Deleted,
            GitChange::TypeChanged => FileChange::TypeChanged,
        },
        added: file.added,
        removed: file.removed,
        body: match file.body {
            GitBody::Text { hunks, truncated } => FileBody::Text {
                hunks: hunks
                    .into_iter()
                    .map(|h| Hunk {
                        old_start: h.old_start,
                        old_len: h.old_len,
                        new_start: h.new_start,
                        new_len: h.new_len,
                        lines: h
                            .lines
                            .into_iter()
                            .map(|l| DiffLineView {
                                kind: match l.kind {
                                    GitLineKind::Context => LineKind::Context,
                                    GitLineKind::Added => LineKind::Added,
                                    GitLineKind::Removed => LineKind::Removed,
                                },
                                text: Untrusted::new(l.text),
                            })
                            .collect(),
                    })
                    .collect(),
                truncated,
            },
            GitBody::Binary => FileBody::Binary {},
            GitBody::TooLarge => FileBody::TooLarge {},
            GitBody::Submodule => FileBody::Submodule {},
            GitBody::Symlink => FileBody::Symlink {},
            GitBody::Omitted => FileBody::Omitted {},
        },
    }
}

fn sections(diff: &mut WorktreeDiff) -> [Option<&mut DiffSection>; 2] {
    [
        match &mut diff.committed {
            CommittedDiff::Ready { section, .. } => Some(section),
            CommittedDiff::NotComputable { .. } => None,
        },
        Some(&mut diff.uncommitted),
    ]
}

/// Cuts `diff` so its serialization is at most `limit` bytes: the first files keep their
/// content, the ones after the cut come out as omitted (and, if even that does not fit, are left
/// out of the list), and the section says `bytes`. `files_total` is never touched.
fn fit(diff: &mut WorktreeDiff, limit: usize) {
    let size = |v: &FileDiff| serde_json::to_vec(v).map_or(0, |b| b.len() + 1);
    let Ok(whole) = serde_json::to_vec(diff) else {
        return;
    };
    if whole.len() <= limit {
        return;
    }
    let files_size: usize = sections(diff)
        .iter()
        .flatten()
        .flat_map(|s| s.files.iter())
        .map(size)
        .sum();
    let mut used = whole.len().saturating_sub(files_size);
    let mut over = false;
    for section in sections(diff).into_iter().flatten() {
        let mut kept = Vec::with_capacity(section.files.len());
        for mut file in std::mem::take(&mut section.files) {
            let mut sz = size(&file);
            if !over && used + sz <= limit {
                used += sz;
                kept.push(file);
                continue;
            }
            over = true;
            section.cut = Some(DiffCut::Bytes);
            file.body = FileBody::Omitted {};
            (file.added, file.removed) = (0, 0);
            sz = size(&file);
            if used + sz <= limit {
                used += sz;
                kept.push(file);
            }
        }
        section.files = kept;
    }
}

/// Files in the answer and the cut of the committed or uncommitted section, for the log.
fn summary(diff: &WorktreeDiff) -> (u32, &'static str) {
    let mut files = u32::try_from(diff.uncommitted.files.len()).unwrap_or(u32::MAX);
    let mut cuts = vec![diff.uncommitted.cut];
    if let CommittedDiff::Ready { section, .. } = &diff.committed {
        files = files.saturating_add(u32::try_from(section.files.len()).unwrap_or(u32::MAX));
        cuts.push(section.cut);
    }
    let cut = cuts
        .into_iter()
        .flatten()
        .map(|c| match c {
            DiffCut::Files => "files",
            DiffCut::Bytes => "bytes",
            DiffCut::Time => "time",
        })
        .next()
        .unwrap_or("none");
    (files, cut)
}

#[cfg(test)]
mod unavailable_tests {
    use super::*;

    #[test]
    fn diff_unavailable_carries_its_code_and_the_reason() {
        let w = serde_json::to_value(unavailable(DiffUnavailable::WorktreeUnavailable)).unwrap();
        assert_eq!(w["code"], DIFF_UNAVAILABLE.code);
        assert_eq!(w["data"]["reason"], "worktree-unavailable");
        let r = serde_json::to_value(unavailable(DiffUnavailable::RepoUnavailable)).unwrap();
        assert_eq!(r["data"]["reason"], "repo-unavailable");
    }
}
