//! Is the hook layer of a repo still active? (US-GRD-004, ADR-GRD-005 § 1.) Read-only: the
//! check never writes in the repo (ADR-GRD-005 Validación 11).
//!
//! The layer is active only while the key is ours (H2), the folder and the dispatchers are what
//! the journal of the profile recorded (H3; the manifest of the repo is not the reference, H-04)
//! and the installed `raptor` is there (H4, reduced to its existence: the signature or the
//! fingerprint of the binary is pending, ADR-GRD-001 § 8). The identity (`dev/inode`) of the
//! folder is not a signal: copying or restoring a repo changes it with nothing lost.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use gitraptor_api::guard::{Diagnostic, HooksLayer, HooksStatus, LossCause};
use gitraptor_git::guard_write::FOLDER;

use super::constants::{MANIFEST, TEMPLATE_VERSION};
use super::install::sha256;
use super::journal::Journal;

/// What a check found: the layer and the diagnostics that do not change the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub hooks: HooksLayer,
    pub diagnostics: Vec<Diagnostic>,
}

impl Health {
    fn of(hooks: HooksLayer) -> Self {
        Self {
            hooks,
            diagnostics: Vec::new(),
        }
    }
}

/// The value of `core.hooksPath` the repo's own configuration says; `Err` when the
/// configuration could not be read (that is not "no key": the check says it could not tell).
pub type Key = Result<Option<String>, ()>;

/// Reads the key from the repo (no global or system level).
#[allow(clippy::result_unit_err)]
pub fn read_key(common: &Path) -> Key {
    super::evaluate::open(common)
        .map(|r| r.hooks_path())
        .ok_or(())
}

/// The folder of the dispatchers that belongs to `common`.
fn hooks_folder(common: &Path) -> PathBuf {
    common.join(FOLDER).join("hooks")
}

/// [`check_with`], reading the key from the repo.
pub fn check(common: &Path, journal: Option<&Journal>) -> Health {
    check_with(common, journal, read_key(common))
}

/// The state of the hook layer of the repo at `common`, given the confirmed journal of its
/// install (`None` when there is none) and the key as read.
pub fn check_with(common: &Path, journal: Option<&Journal>, key: Key) -> Health {
    let Some(journal) = journal else {
        return Health::of(HooksLayer::of(orphaned(common, &key)));
    };
    let mut diagnostics = Vec::new();
    // H2: the effective key is ours.
    match &key {
        Err(()) => diagnostics.push(Diagnostic::ConfigUnreadable),
        Ok(value) => {
            if value.as_deref() != Some(journal.hooks_dir.as_str()) {
                return lost(LossCause::HookspathChanged, diagnostics);
            }
            if Path::new(&journal.hooks_dir) != hooks_folder(common) {
                return lost(LossCause::RepoMoved, diagnostics);
            }
        }
    }
    // H3: the folder and every dispatcher against the journal.
    if let Some(cause) = integrity(common, journal) {
        return lost(cause, diagnostics);
    }
    // H4, reduced: the `raptor` the dispatchers start.
    if !Path::new(&journal.raptor).exists() {
        return lost(LossCause::BinaryMissing, diagnostics);
    }
    if journal.template < TEMPLATE_VERSION {
        diagnostics.push(Diagnostic::TemplateOutdated);
    }
    if journal.confirms_base.is_none() {
        diagnostics.push(Diagnostic::BaseUnconfirmed);
    }
    Health {
        hooks: HooksLayer::of(HooksStatus::Active),
        diagnostics,
    }
}

fn lost(cause: LossCause, diagnostics: Vec<Diagnostic>) -> Health {
    Health {
        hooks: HooksLayer::lost(cause),
        diagnostics,
    }
}

/// No journal: `orphaned` when the repo still carries Guardrails' key and the manifest (the
/// profile was deleted: ADR-GRD-005 § 1, J5), `not-installed` otherwise. The manifest is only
/// supporting evidence, never the reference.
fn orphaned(common: &Path, key: &Key) -> HooksStatus {
    let ours = matches!(key, Ok(Some(v)) if Path::new(v) == hooks_folder(common));
    if ours && common.join(FOLDER).join(MANIFEST).is_file() {
        HooksStatus::Orphaned
    } else {
        HooksStatus::NotInstalled
    }
}

/// H3: the first thing that is not what the journal recorded. The manifest is excluded: it is
/// not the integrity reference and editing it changes nothing (ADR-GRD-001 Validación 9).
fn integrity(common: &Path, journal: &Journal) -> Option<LossCause> {
    let folder = common.join(FOLDER);
    if !hooks_folder(common).is_dir() {
        return Some(LossCause::FolderMissing);
    }
    let mut altered = false;
    let mut not_executable = false;
    for file in journal.files.iter().filter(|f| f.path != MANIFEST) {
        if Path::new(&file.path)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            // A path of the journal that leaves the folder is not ours to read.
            altered = true;
            continue;
        }
        let path = folder.join(&file.path);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return Some(LossCause::DispatcherMissing);
        };
        if !meta.is_file() {
            altered = true;
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) if sha256(&bytes) == file.sha256 => {}
            Ok(_) => altered = true,
            Err(_) => return Some(LossCause::DispatcherMissing),
        }
        if file.path.starts_with("hooks/") && !executable(&meta) {
            not_executable = true;
        }
    }
    if altered {
        Some(LossCause::DispatcherAltered)
    } else if not_executable {
        Some(LossCause::DispatcherNotExecutable)
    } else {
        None
    }
}

/// Git skips a hook without the execute permission. Windows has no such bit (its check, the
/// DACL, is pending).
fn executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

/// A cheap fingerprint of what the check reads: size, modification time and mode of the
/// configuration, the folders and every file of the journal, with no gix and no hashing. When it
/// is the one of the last check, the full check is skipped (it still runs now and then, since a
/// time can be forged).
pub fn fingerprint(common: &Path, journal: &Journal) -> u64 {
    let mut h = DefaultHasher::new();
    let folder = common.join(FOLDER);
    let mut paths: Vec<PathBuf> = vec![
        common.join("config"),
        folder.clone(),
        hooks_folder(common),
        PathBuf::from(&journal.raptor),
    ];
    paths.extend(journal.files.iter().map(|f| folder.join(&f.path)));
    for p in paths {
        p.hash(&mut h);
        match std::fs::symlink_metadata(&p) {
            Ok(m) => {
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    m.mode().hash(&mut h);
                    m.ino().hash(&mut h);
                }
            }
            Err(_) => 0u8.hash(&mut h),
        }
    }
    h.finish()
}
