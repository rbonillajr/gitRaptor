//! The target index (ADR-TMC-002 § 3, step 7): built in a temporary file of the profile with
//! `update-index --index-info` (conflict stages included) and `--skip-worktree`, then written
//! into the worktree's own `index.lock` and renamed over the index, with Git's lock protocol.
//!
//! The installed index carries no stat data: the first `git status` compares content again.
//! `intent-to-add` has no plumbing in Git 2.38, so those entries are left to the caller to
//! report.

use super::lock::GitLock;
use super::worktree::WriteWorktree;
use super::{Result, WriteContext, WriteError};
use crate::Oid;

/// One entry of the target index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub path: Vec<u8>,
    /// `100644`, `100755`, `120000` or `160000`.
    pub mode: u32,
    pub id: Oid,
    /// 0, or 1 to 3 for a conflict.
    pub stage: u8,
}

/// The bytes of a built index; `None` for an index without entries (Git reads a missing index
/// as an empty one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltIndex(Option<Vec<u8>>);

/// Builds the index of `entries` for `worktree` in the scratch folder of the profile.
pub fn build(
    ctx: &WriteContext,
    worktree: &WriteWorktree,
    entries: &[IndexEntry],
    skip_worktree: &[Vec<u8>],
) -> Result<BuiltIndex> {
    for e in entries {
        super::tree_path::check(&e.path).map_err(|r| WriteError::InvalidInput(r.to_string()))?;
        if !matches!(e.mode, 0o100644 | 0o100755 | 0o120000 | 0o160000) || e.stage > 3 {
            return Err(WriteError::InvalidInput("index entry mode or stage".into()));
        }
    }
    if entries.is_empty() {
        return Ok(BuiltIndex(None));
    }
    let tmp = ctx.scratch_path("index");
    let result = (|| {
        let mut payload = Vec::new();
        for e in entries {
            payload.extend_from_slice(format!("{:o} {} {}\t", e.mode, e.id, e.stage).as_bytes());
            payload.extend_from_slice(&e.path);
            payload.push(0);
        }
        super::cli::index_info(ctx, worktree.git_dir(), worktree.root(), &tmp, &payload)?;
        if !skip_worktree.is_empty() {
            let mut payload = Vec::new();
            for p in skip_worktree {
                super::tree_path::check(p).map_err(|r| WriteError::InvalidInput(r.to_string()))?;
                payload.extend_from_slice(p);
                payload.push(0);
            }
            super::cli::skip_worktree(ctx, worktree.git_dir(), worktree.root(), &tmp, &payload)?;
        }
        Ok(std::fs::read(&tmp)?)
    })();
    let _ = std::fs::remove_file(&tmp);
    result.map(|b| BuiltIndex(Some(b)))
}

/// Installs `built` through `lock` (the worktree's own `index.lock`, taken in step 3). The lock
/// must still be ours; installing releases it.
pub fn install(lock: GitLock, worktree: &WriteWorktree, built: &BuiltIndex) -> Result<()> {
    if !lock.is_ours() {
        return Err(WriteError::Busy(lock.path().to_owned()));
    }
    match &built.0 {
        Some(bytes) => lock.commit(bytes),
        None => {
            match std::fs::remove_file(worktree.index_path()) {
                Ok(()) => super::lock::sync_parent(&worktree.index_path()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            lock.release()
        }
    }
}
