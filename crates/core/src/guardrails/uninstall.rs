//! Uninstall of the hook layer (US-GRD-003; ADR-GRD-001 § 4, Desinstalación), run by the daemon
//! loop once the window of the reserved command closed without a cancellation (ADR-GRD-007 § 1,
//! D5). The inverse of the install: the journal says `uninstalling`, then the key goes back to
//! what the repo had, then the listed files and the empty folder go, then the journal. Each step
//! is idempotent, and the recovery at startup finishes or keeps it (ADR-GRD-001 § 4,
//! Recuperación).

use std::path::Path;

use gitraptor_api::guard::GuardStatus;
use gitraptor_git::guard_write::GuardWriter;

use super::cut::{When, trip};
use super::install::{self, GuardCtx};
use super::journal::{Journal, Stage, snapshot_path};
use super::registry::GuardRegistry;
use crate::profile::{ProfileDirs, RepoStore};

/// Why an uninstall did not happen.
#[derive(Debug)]
pub enum UninstallError {
    /// No confirmed install of this profile in the repo.
    NotInstalled,
    /// A step failed: the protection is still complete (the key was not touched) or the next
    /// start finishes the uninstall (the key was already back).
    Failed(String),
}

fn save(store: &mut RepoStore, journal: &Journal) -> Result<(), UninstallError> {
    store
        .set_guard_keys(Some(Some(&journal.to_json())), None, None, None)
        .map(|_| ())
        .map_err(|e| UninstallError::Failed(format!("journal: {e:?}")))
}

/// Whether the repo's own `core.hooksPath` is still the one the install wrote.
fn key_is_ours(writer: &GuardWriter<'_>, common: &Path, journal: &Journal) -> Option<bool> {
    writer
        .local_hooks_path(common)
        .ok()
        .map(|v| v.is_some_and(|v| Path::new(&v) == Path::new(&journal.hooks_dir)))
}

/// Puts the key back (ADR-GRD-001 § 4, Desinstalación, paso 1): the prior value when it was
/// local, no key otherwise. A key another manager changed after the install is not touched.
fn restore_key(
    writer: &GuardWriter<'_>,
    common: &Path,
    journal: &Journal,
) -> Result<(), UninstallError> {
    if !key_is_ours(writer, common, journal).ok_or_else(|| UninstallError::Failed("key".into()))? {
        return Ok(());
    }
    let written = match (journal.prior.level.as_str(), &journal.prior.value) {
        ("local", Some(value)) => writer.restore_hooks_path(common, value),
        _ => writer.unset_hooks_path(common),
    };
    written.map_err(|e| UninstallError::Failed(format!("key: {e}")))
}

/// Removes what the journal lists, then the empty folders (never recursive: a foreign file
/// stays), the leftovers of an interrupted install, the read-only snapshot and the registry
/// entry, and finally the journal.
fn remove_rest(
    writer: &GuardWriter<'_>,
    dirs: &ProfileDirs,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    journal: &Journal,
) -> Result<(), UninstallError> {
    let listed = journal.listed();
    trip("uninstall-folder", When::Before);
    let expected = match journal.folder {
        Some(id) => Some(id.into()),
        None => writer
            .folder_id(common)
            .map_err(|e| UninstallError::Failed(format!("folder: {e}")))?,
    };
    if let Some(expected) = expected {
        // What a killed upgrade left next to a listed file goes first: the folder only goes
        // when it is empty.
        writer
            .remove_file_temporaries(common, expected, &listed)
            .map_err(|e| UninstallError::Failed(format!("folder: {e}")))?;
        writer
            .remove_folder(common, &listed, expected)
            .map_err(|e| UninstallError::Failed(format!("folder: {e}")))?;
    }
    let _ = writer.remove_temporaries(common, &listed);
    trip("uninstall-folder", When::After);
    trip("uninstall-clear", When::Before);
    registry.remove(repo_id);
    let _ = std::fs::remove_file(snapshot_path(&dirs.state, repo_id));
    store
        .set_guard_keys(Some(None), None, None, None)
        .map_err(|e| UninstallError::Failed(format!("journal: {e:?}")))?;
    trip("uninstall-clear", When::After);
    Ok(())
}

/// Removes the hook layer of a repo. The window of the reserved command already closed: this is
/// the transaction itself.
pub fn uninstall(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
) -> Result<GuardStatus, UninstallError> {
    let keys = store.guard_keys().unwrap_or_default();
    let Some(mut journal) = install::journal(&keys).filter(|j| j.stage == Stage::Confirmed) else {
        return Err(UninstallError::NotInstalled);
    };
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    // The `config` must be a regular file before its key is touched (M-03). Its inode changes
    // with every legitimate `git config`, so only the kind is checked.
    if !matches!(writer.config_id(common), Ok(Some(_))) {
        return Err(UninstallError::Failed(
            "config is not a regular file".into(),
        ));
    }
    trip("uninstall-journal", When::Before);
    journal.stage = Stage::Uninstalling;
    save(store, &journal)?;
    trip("uninstall-journal", When::After);
    trip("uninstall-key", When::Before);
    if let Err(e) = restore_key(&writer, common, &journal) {
        // The key is ours still: the protection is complete. Back to confirmed.
        journal.stage = Stage::Confirmed;
        let _ = save(store, &journal);
        return Err(e);
    }
    trip("uninstall-key", When::After);
    // Past this point the protection is gone: a failure is finished by the next start.
    remove_rest(
        &writer, ctx.dirs, repo_id, common, store, registry, &journal,
    )?;
    Ok(install::status(repo_id, common, store))
}

/// What the recovery did with an unfinished uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovered {
    /// The key was back: the folder and the journal went; the repo is as before the install.
    Completed,
    /// The key was still ours: the protection is complete and stays (`no completada`).
    Kept,
}

/// Startup: an uninstall left in `uninstalling` (ADR-GRD-001 § 4, Recuperación). A key that
/// another manager changed counts as already restored.
pub fn recover(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    mut journal: Journal,
) -> Recovered {
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    match key_is_ours(&writer, common, &journal) {
        // Unknown: keep the protection; the next start tries again.
        None | Some(true) => {
            journal.stage = Stage::Confirmed;
            let _ = save(store, &journal);
            let confirmed = store.confirmed_team_baseline().ok().flatten();
            install::publish(ctx.dirs, repo_id, &journal, registry, confirmed);
            Recovered::Kept
        }
        Some(false) => {
            // The key is already back: nothing protects the repo any more. A failure here
            // leaves the journal in `uninstalling`, and the next start retries.
            let _ = remove_rest(
                &writer, ctx.dirs, repo_id, common, store, registry, &journal,
            );
            Recovered::Completed
        }
    }
}
