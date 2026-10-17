//! Uninstall of the hook layer (US-GRD-003; ADR-GRD-001 § 4, Desinstalación), run by the daemon
//! loop once the window of the reserved command closed without a cancellation (ADR-GRD-007 § 1,
//! D5). The inverse of the install: what can refuse is checked first (the `config`, the folder,
//! the leftovers of a killed write), then the journal says `uninstalling`, then the key goes back
//! to what the repo had, then the files Guardrails wrote and the empty folder go, then the
//! journal. Each step is idempotent, and the recovery at startup finishes or keeps it
//! (ADR-GRD-001 § 4, Recuperación). An uninstall whose cleanup stopped after the key is finished
//! by the next `raptor guard uninstall`, and a folder this profile does not know is removed by it
//! when it is recognizably Guardrails' (BR-CONS-005, "retirar").

use std::path::Path;

use gitraptor_api::guard::{GuardStatus, GuardStep};
use gitraptor_git::guard_write::{FOLDER, GuardWriter};

use super::constants::{Constants, DISPATCH_CONF, MANIFEST};
use super::cut::{When, trip};
use super::install::{self, Anchor, Failure, GuardCtx, sha256};
use super::journal::{FileHash, Journal, Prior, Stage, snapshot_path};
use super::registry::GuardRegistry;
use crate::profile::{ProfileDirs, RepoStore};

/// Why an uninstall did not happen.
#[derive(Debug)]
pub enum UninstallError {
    /// No install of this profile in the repo, and no folder recognizable as Guardrails'.
    NotInstalled,
    /// A step failed: before the key, the protection is as it was; after it, the journal stays
    /// `uninstalling` and the next uninstall (or start) finishes the cleanup.
    Failed(Failure),
}

fn fail(step: GuardStep, detail: impl std::fmt::Display) -> UninstallError {
    UninstallError::Failed(Failure {
        step,
        detail: detail.to_string(),
    })
}

fn save(store: &mut RepoStore, journal: &Journal) -> Result<(), UninstallError> {
    store
        .set_guard_keys(Some(Some(&journal.to_json())), None, None, None)
        .map(|_| ())
        .map_err(|e| fail(GuardStep::Journal, format!("{e:?}")))
}

/// Where the record of what is removed comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The confirmed install of this profile.
    Confirmed,
    /// An uninstall that stopped after it started (its cleanup failed, or it was cut).
    Resumed,
    /// A folder this profile does not know, recognized by its content.
    Orphan,
}

/// What an uninstall of the repo would remove.
#[derive(Debug, Clone)]
pub struct Target {
    pub journal: Journal,
    pub origin: Origin,
}

impl Target {
    /// The local `core.hooksPath` an orphan's manifest says the repo had, which its removal
    /// writes back: shown before the window (ADR-GRD-005 § 1, retirar).
    pub fn restores_hooks_path(&self) -> Option<&str> {
        (self.journal.orphan && self.journal.prior.level == "local")
            .then_some(self.journal.prior.value.as_deref())
            .flatten()
    }
}

/// What `raptor guard uninstall` would remove in the repo, without writing anything: the
/// confirmed install, an uninstall that did not finish, or a folder recognized as Guardrails'.
/// An install that did not reach its confirmation is the recovery's at startup, not this.
pub fn target(ctx: &GuardCtx<'_>, common: &Path, store: &RepoStore) -> Option<Target> {
    let keys = store.guard_keys().unwrap_or_default();
    match install::journal(&keys) {
        Some(journal) if journal.stage == Stage::Confirmed => Some(Target {
            journal,
            origin: Origin::Confirmed,
        }),
        Some(journal) if journal.stage == Stage::Uninstalling => Some(Target {
            journal,
            origin: Origin::Resumed,
        }),
        Some(_) => None,
        None => orphan(ctx, common).map(|journal| Target {
            journal,
            origin: Origin::Orphan,
        }),
    }
}

/// The largest manifest read, and the most files it may list.
const MAX_MANIFEST: usize = 64 * 1024;
const MAX_LISTED: usize = 64;

/// A folder this profile does not know, read as Guardrails' only by what proves it: a real
/// folder (never a link) whose `manifest.json` carries the mark, names this repo's hooks folder
/// and lists plain relative paths. The manifest is in the repo and anyone could have written
/// it, so it is never trusted for what is removed: only a dispatcher identical byte by byte to
/// the installed one, a `dispatch.conf` of the strict format that names this repo, and the
/// manifest itself are listed; anything else stays (NFR-01). Its `prior_hooks_path` is restored
/// only when it was local and representable, and shown before the window.
fn orphan(ctx: &GuardCtx<'_>, common: &Path) -> Option<Journal> {
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    let read = writer.read_folder(common, &[MANIFEST]).ok()??;
    let bytes = read.files.first()?.1.as_ref()?;
    if bytes.len() > MAX_MANIFEST {
        return None;
    }
    let manifest: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let hooks_dir = common.join(FOLDER).join("hooks");
    let note = manifest.get("note")?.as_str()?;
    let named = manifest.get("hooks_dir")?.as_str()?;
    if !note.starts_with("GitRaptor Guardrails") || Path::new(named) != hooks_dir {
        return None;
    }
    let listed: Vec<&str> = manifest
        .get("files")?
        .as_array()?
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<_>>()?;
    if listed.len() > MAX_LISTED || !listed.contains(&MANIFEST) {
        return None;
    }
    // Plain relative paths only: the read refuses anything else.
    let read = writer.read_folder(common, &listed).ok()??;
    let stub = ctx.stub().and_then(|s| std::fs::read(s).ok());
    let this_repo = gitraptor_policy::guard::fastpath::simplified(common.to_path_buf());
    let recognized = |path: &str, bytes: &[u8]| -> bool {
        if path.starts_with("hooks/") {
            return stub.as_deref() == Some(bytes);
        }
        match path {
            DISPATCH_CONF => std::str::from_utf8(bytes)
                .ok()
                .and_then(Constants::parse)
                .is_some_and(|c| c.common == this_repo),
            MANIFEST => true,
            _ => false,
        }
    };
    let files: Vec<FileHash> = read
        .files
        .iter()
        .filter_map(|(path, bytes)| {
            let bytes = bytes.as_deref()?;
            recognized(path, bytes).then(|| FileHash {
                path: path.clone(),
                sha256: sha256(bytes),
            })
        })
        .collect();
    let prior = manifest
        .get("prior_hooks_path")
        .filter(|p| p.get("level").and_then(|l| l.as_str()) == Some("local"))
        .and_then(|p| p.get("value")?.as_str())
        .filter(|v| !v.is_empty() && !v.starts_with('-') && !v.chars().any(char::is_control))
        .map_or_else(
            || Prior {
                value: None,
                level: "none".into(),
                dir: String::new(),
            },
            |v| Prior {
                value: Some(v.to_owned()),
                level: "local".into(),
                dir: String::new(),
            },
        );
    Some(Journal {
        version: Journal::VERSION,
        stage: Stage::Uninstalling,
        at_ms: 0,
        common_dir: common.to_string_lossy().into_owned(),
        hooks_dir: hooks_dir.to_string_lossy().into_owned(),
        files,
        folder: Some(read.id.into()),
        config: None,
        raptor: String::new(),
        template: manifest
            .get("template")
            .and_then(serde_json::Value::as_u64)
            .and_then(|t| u32::try_from(t).ok())
            .unwrap_or(0),
        instance: ctx.instance.to_owned(),
        prior,
        chained: Vec::new(),
        confirms_base: None,
        protected_bases: Vec::new(),
        upgrade: None,
        orphan: true,
    })
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
    if !key_is_ours(writer, common, journal)
        .ok_or_else(|| fail(GuardStep::Key, "the key could not be read"))?
    {
        return Ok(());
    }
    let written = match (journal.prior.level.as_str(), &journal.prior.value) {
        ("local", Some(value)) => writer.restore_hooks_path(common, value),
        _ => writer.unset_hooks_path(common),
    };
    written.map_err(|e| fail(GuardStep::Key, e))
}

/// Removes the files Guardrails wrote (the ones whose content has a hash the journal records;
/// anything else stays, and the folder with it: NFR-01), then the empty folders (never
/// recursive), the leftovers of an interrupted install, the read-only snapshot and the registry
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
            .map_err(|e| fail(GuardStep::FolderIdentity, e))?,
    };
    if let Some(expected) = expected {
        // What a killed upgrade left next to a listed file goes first: the folder only goes
        // when it is empty.
        writer
            .remove_file_temporaries(common, expected, &listed)
            .map_err(|e| fail(GuardStep::Temporaries, e))?;
        writer
            .remove_recognized(common, &listed, expected, &|path, bytes| {
                // The manifest is recovery data, never a hook, and not the integrity reference
                // (ADR-GRD-001 Validación 9): an edited one goes too.
                path == MANIFEST || install::accepts(journal, path, bytes)
            })
            .map_err(|e| fail(GuardStep::Folder, e))?;
    }
    let _ = writer.remove_temporaries(common, &listed);
    trip("uninstall-folder", When::After);
    trip("uninstall-clear", When::Before);
    registry.remove(repo_id);
    let _ = std::fs::remove_file(snapshot_path(&dirs.state, repo_id));
    store
        .set_guard_keys(Some(None), None, None, None)
        .map_err(|e| fail(GuardStep::Clear, format!("{e:?}")))?;
    trip("uninstall-clear", When::After);
    Ok(())
}

/// Removes the hook layer of a repo: the confirmed install, the rest of an uninstall that
/// stopped, or a folder recognized as Guardrails' that this profile does not know. The window of
/// the reserved command already closed: this is the transaction itself.
pub fn uninstall(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
) -> Result<GuardStatus, UninstallError> {
    apply(ctx, repo_id, common, store, registry, None)
}

/// What the announcement of the reserved command showed (ADR-GRD-007 § 1): the uninstall
/// applies only what was shown before the window.
#[derive(Debug, Clone, Copy)]
pub struct Announced<'a> {
    /// The client has `guard.recovery`: it can be shown an orphan or an unfinished uninstall.
    /// Without it only the confirmed install is removed, as before.
    pub recovery: bool,
    /// The `core.hooksPath` the announcement said it writes back (an orphan's manifest).
    pub restores: Option<&'a str>,
}

/// [`uninstall`], checked against what its announcement showed: a manifest rewritten during
/// the window (another value to write back) refuses it before anything is touched.
pub fn apply(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    announced: Option<Announced<'_>>,
) -> Result<GuardStatus, UninstallError> {
    let Some(target) = target(ctx, common, store) else {
        return Err(UninstallError::NotInstalled);
    };
    if let Some(announced) = announced {
        if !announced.recovery && target.origin != Origin::Confirmed {
            return Err(UninstallError::NotInstalled);
        }
        if target.restores_hooks_path() != announced.restores {
            return Err(fail(
                GuardStep::FolderIdentity,
                "the manifest changed since the announcement",
            ));
        }
    }
    let Target {
        mut journal,
        origin,
    } = target;
    let writer = GuardWriter::new(ctx.git, ctx.invoker);
    // Everything that can refuse comes before the key, so a refusal leaves the protection as it
    // was. The `config` must be a regular file before its key is touched (M-03); its inode
    // changes with every legitimate `git config`, so only the kind is checked.
    if !matches!(writer.config_id(common), Ok(Some(_))) {
        return Err(fail(GuardStep::Config, "config is not a regular file"));
    }
    // The folder as it is now: only the files whose hash the journal records are removed, so a
    // real folder whose identity changed (a reboot, a restored copy) is followed.
    match install::reanchor(&writer, common, &journal, false).map_err(UninstallError::Failed)? {
        Anchor::Same(id) | Anchor::Moved(id) => journal.folder = Some(id.into()),
        Anchor::Absent => {}
    }
    if let Some(folder) = journal.folder {
        writer
            .remove_file_temporaries(common, folder.into(), &journal.listed())
            .map_err(|e| fail(GuardStep::Temporaries, e))?;
    }
    trip("uninstall-journal", When::Before);
    journal.stage = Stage::Uninstalling;
    save(store, &journal)?;
    trip("uninstall-journal", When::After);
    trip("uninstall-key", When::Before);
    if let Err(e) = restore_key(&writer, common, &journal) {
        match origin {
            // The key is ours still: the protection is complete. Back to confirmed.
            Origin::Confirmed => {
                journal.stage = Stage::Confirmed;
                let _ = save(store, &journal);
            }
            // The orphan stays what it was: nothing of this profile records it.
            Origin::Orphan => {
                let _ = store.set_guard_keys(Some(None), None, None, None);
            }
            Origin::Resumed => {}
        }
        return Err(e);
    }
    trip("uninstall-key", When::After);
    // Past this point the protection is gone: a failure is finished by the next uninstall or
    // start, and the status says a folder is left.
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
    /// The key was still ours: the protection is complete and stays (`no completada`), or the
    /// orphan stays as it was.
    Kept,
    /// The key was back but the cleanup failed at this step: the journal stays `uninstalling`,
    /// and `raptor guard uninstall` (or the next start) finishes it.
    CleanupPending(GuardStep),
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
        // Unknown: keep the protection (or the orphan); the next start tries again.
        None | Some(true) if journal.orphan => {
            let _ = store.set_guard_keys(Some(None), None, None, None);
            Recovered::Kept
        }
        None | Some(true) => {
            journal.stage = Stage::Confirmed;
            let _ = save(store, &journal);
            let confirmed = store.confirmed_team_baseline().ok().flatten();
            install::publish(ctx.dirs, repo_id, &journal, registry, confirmed);
            Recovered::Kept
        }
        // The key is already back: nothing protects the repo any more. A failure here leaves
        // the journal in `uninstalling`, and says so.
        Some(false) => match remove_rest(
            &writer, ctx.dirs, repo_id, common, store, registry, &journal,
        ) {
            Ok(()) => Recovered::Completed,
            Err(UninstallError::Failed(f)) => Recovered::CleanupPending(f.step),
            Err(UninstallError::NotInstalled) => Recovered::CleanupPending(GuardStep::Clear),
        },
    }
}
