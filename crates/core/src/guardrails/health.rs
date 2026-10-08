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

/// The largest file the check reads (the dispatchers are a few KB and the native one under a
/// MB): a file over it is not what the journal recorded.
const MAX_FILE: u64 = 32 * 1024 * 1024;

/// Reads a regular file for the integrity check, the way a file somebody else can swap may be
/// read: never through a link, never blocking on a pipe (a FIFO put where a dispatcher was would
/// freeze the loop), never more than [`MAX_FILE`]. `Ok(None)`: not a regular file, or too big.
pub(crate) fn read_regular(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = {
        if !std::fs::symlink_metadata(path)?.is_file() {
            return Ok(None);
        }
        std::fs::File::open(path)?
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= MAX_FILE).then_some(bytes))
}

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
        // The repo moved with Guardrails' key in its config: it names a folder that is not this
        // repo's. Nothing records an install here (a moved repo is another entry of the
        // profile), but the key does not lie: the hooks it points to are not this repo's.
        if let Ok(Some(value)) = &key
            && Path::new(value).ends_with(Path::new(FOLDER).join("hooks"))
            && Path::new(value) != hooks_folder(common)
        {
            return lost(LossCause::RepoMoved, Vec::new());
        }
        return Health::of(HooksLayer::of(orphaned(common, &key)));
    };
    let mut diagnostics = Vec::new();
    // H2: the effective key is ours.
    match &key {
        Err(()) => diagnostics.push(Diagnostic::ConfigUnreadable),
        Ok(value) => {
            // As paths, like the uninstall compares them: a trailing slash is the same key.
            if value.as_deref().map(Path::new) != Some(Path::new(&journal.hooks_dir)) {
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
        match read_regular(&path) {
            Ok(Some(bytes)) if sha256(&bytes) == file.sha256 => {}
            Ok(_) => altered = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Some(LossCause::DispatcherMissing);
            }
            // A link swapped in after the check above, or anything else that cannot be read.
            Err(_) => altered = true,
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
                    // The change time cannot be set back by an ordinary user, unlike the
                    // modification time (`touch -r`).
                    m.ctime().hash(&mut h);
                    m.ctime_nsec().hash(&mut h);
                }
            }
            Err(_) => 0u8.hash(&mut h),
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_naming_the_hooks_of_another_folder_is_a_moved_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let common = tmp.path().join(".git");
        std::fs::create_dir_all(common.join(FOLDER).join("hooks")).unwrap();
        let elsewhere = "/old/place/.git/gitraptor/hooks".to_owned();
        let h = check_with(&common, None, Ok(Some(elsewhere)));
        assert_eq!(h.hooks.cause, Some(LossCause::RepoMoved));
        // Another tool's folder is not ours to call moved.
        let h = check_with(&common, None, Ok(Some(".husky/_".into())));
        assert_eq!(h.hooks.status, HooksStatus::NotInstalled);
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_a_link_or_a_huge_file_is_never_read_blocking() {
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("fifo");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // Opened without blocking and refused: it is not a regular file.
        assert_eq!(read_regular(&fifo).unwrap(), None);
        let target = tmp.path().join("target");
        std::fs::write(&target, b"x").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_regular(&link).is_err());
        let huge = tmp.path().join("huge");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert_eq!(read_regular(&huge).unwrap(), None);
        assert_eq!(read_regular(&target).unwrap().as_deref(), Some(&b"x"[..]));
    }
}
