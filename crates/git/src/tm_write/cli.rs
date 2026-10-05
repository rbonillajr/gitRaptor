//! Typed Git invocations of the write layer. Each one maps to a variant of the closed list in
//! `invoke.rs` and passes its data only through standard input.

use std::path::Path;

use super::{Result, WriteContext, WriteError};
use crate::invoke::{Input, Output, WriteSubcommand, is_dubious_ownership};

fn run(
    ctx: &WriteContext,
    git_dir: &Path,
    work_tree: Option<&Path>,
    index_file: Option<&Path>,
    sub: WriteSubcommand,
    input: Input<'_>,
    stdout_to: Option<std::fs::File>,
) -> Result<Output> {
    let target = ctx.target(git_dir, work_tree, index_file);
    Ok(ctx
        .invoker()
        .run_write(&ctx.git().path, &target, sub, input, stdout_to)?)
}

fn failure(what: &str, out: &Output) -> WriteError {
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_dubious_ownership(&stderr) {
        return WriteError::Untrusted(format!("{what}: dubious ownership"));
    }
    WriteError::Git(format!(
        "{what} failed with code {:?}: {}",
        out.code,
        stderr.lines().next().unwrap_or("")
    ))
}

/// `update-ref --stdin -z` with `payload`. A ref that is not at its expected old value, or a
/// lock of someone else, fails the whole transaction.
pub(super) fn update_ref(ctx: &WriteContext, common_dir: &Path, payload: &[u8]) -> Result<()> {
    let out = run(
        ctx,
        common_dir,
        None,
        None,
        WriteSubcommand::UpdateRef,
        Input::Bytes(payload),
        None,
    )?;
    if out.success {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("but expected")
        || stderr.contains("reference already exists")
        || stderr.contains("unable to resolve reference")
        || stderr.contains("cannot lock ref")
    {
        return Err(WriteError::RefMoved(
            stderr.lines().next().unwrap_or("").to_owned(),
        ));
    }
    Err(failure("update-ref", &out))
}

/// `update-index -z --index-info` on the temporary index `index_file`.
pub(super) fn index_info(
    ctx: &WriteContext,
    git_dir: &Path,
    work_tree: &Path,
    index_file: &Path,
    payload: &[u8],
) -> Result<()> {
    let out = run(
        ctx,
        git_dir,
        Some(work_tree),
        Some(index_file),
        WriteSubcommand::IndexInfo,
        Input::Bytes(payload),
        None,
    )?;
    if out.success {
        Ok(())
    } else {
        Err(failure("update-index --index-info", &out))
    }
}

/// `update-index --skip-worktree -z --stdin` on the temporary index `index_file`.
pub(super) fn skip_worktree(
    ctx: &WriteContext,
    git_dir: &Path,
    work_tree: &Path,
    index_file: &Path,
    payload: &[u8],
) -> Result<()> {
    let out = run(
        ctx,
        git_dir,
        Some(work_tree),
        Some(index_file),
        WriteSubcommand::SkipWorktree,
        Input::Bytes(payload),
        None,
    )?;
    if out.success {
        Ok(())
    } else {
        Err(failure("update-index --skip-worktree", &out))
    }
}

/// `pack-objects --revs --stdout` on the store, into `pack`.
pub(super) fn pack_objects(
    ctx: &WriteContext,
    store_dir: &Path,
    revs: &[u8],
    pack: std::fs::File,
) -> Result<()> {
    let out = run(
        ctx,
        store_dir,
        None,
        None,
        WriteSubcommand::PackObjects,
        Input::Bytes(revs),
        Some(pack),
    )?;
    if out.success {
        Ok(())
    } else {
        Err(failure("pack-objects", &out))
    }
}

/// `index-pack --stdin --strict --keep` in the user's repository; returns the pack's hash.
pub(super) fn index_pack(
    ctx: &WriteContext,
    common_dir: &Path,
    pack: std::fs::File,
) -> Result<String> {
    let out = run(
        ctx,
        common_dir,
        None,
        None,
        WriteSubcommand::IndexPack,
        Input::File(pack),
        None,
    )?;
    if !out.success {
        return Err(failure("index-pack", &out));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let hash = text
        .trim()
        .split('\t')
        .nth(1)
        .filter(|h| h.len() >= 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| WriteError::Git("index-pack: unexpected output".into()))?;
    Ok(hash.to_owned())
}

/// `repack` or `prune` on the store, with their fixed options.
pub(super) fn maintain_store(
    ctx: &WriteContext,
    store_dir: &Path,
    sub: WriteSubcommand,
) -> Result<()> {
    debug_assert!(matches!(
        sub,
        WriteSubcommand::Repack | WriteSubcommand::Prune
    ));
    let out = run(ctx, store_dir, None, None, sub, Input::None, None)?;
    if out.success {
        Ok(())
    } else {
        Err(failure(sub.words()[0], &out))
    }
}
