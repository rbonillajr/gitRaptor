//! File operations of the Guardrails write layer without descriptor-relative calls (Windows):
//! every path is checked not to be a link right before it is used, and files are created
//! exclusively. Weaker than the Unix variant (a check-then-use window), which the identity of
//! the folder (`winsys::file_id`, M-03) narrows. On Windows the folder is created with the
//! protected DACL of ADR-GRD-001 § 1 (M-07): the user, SYSTEM and Administrators, inherited by
//! everything written inside, so no ACE of write for `Everyone`, `Users` or `Authenticated
//! Users` reaches the dispatchers.

use std::io::Write;
use std::path::Path;

use super::{
    FileId, GuardWriteError, NewFile, Result, is_file_temporary, is_temporary, temporary_name,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Dir,
}

/// `(volume, index)` of `path` itself, never following a link; zeros where the platform has no
/// such identity.
fn id_of(path: &Path) -> Result<FileId> {
    #[cfg(windows)]
    {
        let (dev, ino) = gitraptor_winsys::file_id::of_path(path)?;
        Ok(FileId {
            dev: u64::from(dev),
            ino,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(FileId { dev: 0, ino: 0 })
    }
}

/// A folder only the user (plus SYSTEM and Administrators) can write, where the platform has
/// the rule (M-07); the plain folder elsewhere.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        gitraptor_winsys::acl::create_private_dir(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::create_dir(path)
    }
}

fn not_link(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(GuardWriteError::Changed("a link")),
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn entry_id(common: &Path, name: &str, kind: Kind) -> Result<Option<FileId>> {
    not_link(common)?;
    match not_link(&common.join(name))? {
        None => Ok(None),
        Some(m) if (kind == Kind::Dir) == m.is_dir() => Ok(Some(id_of(&common.join(name))?)),
        Some(_) => Err(GuardWriteError::Changed("an entry of the common directory")),
    }
}

pub(super) fn write_folder(common: &Path, files: &[NewFile<'_>]) -> Result<FileId> {
    not_link(common)?;
    if not_link(&common.join(super::FOLDER))?.is_some() {
        return Err(GuardWriteError::Exists);
    }
    let temp = temporary_name();
    let root = common.join(&temp);
    create_private_dir(&root)?;
    let id = id_of(&root)?;
    let fill = || -> Result<()> {
        for f in files {
            let path = root.join(f.path);
            if let Some(parent) = path.parent()
                && parent != root
            {
                match std::fs::create_dir(parent) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        not_link(parent)?;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(f.bytes)?;
            file.sync_all()?;
        }
        Ok(())
    };
    if let Err(e) = fill() {
        let listed: Vec<&str> = files.iter().map(|f| f.path).collect();
        let _ = remove_folder(common, &temp, &listed, None);
        return Err(e);
    }
    if not_link(&common.join(super::FOLDER))?.is_some() {
        let listed: Vec<&str> = files.iter().map(|f| f.path).collect();
        let _ = remove_folder(common, &temp, &listed, None);
        return Err(GuardWriteError::Exists);
    }
    std::fs::rename(&root, common.join(super::FOLDER))?;
    Ok(id)
}

pub(super) fn replace_files(common: &Path, expected: FileId, files: &[NewFile<'_>]) -> Result<()> {
    not_link(common)?;
    let root = common.join(super::FOLDER);
    match not_link(&root)? {
        Some(m) if m.is_dir() => {}
        _ => return Err(GuardWriteError::Changed("the guardrails folder")),
    }
    if id_of(&root)? != expected {
        return Err(GuardWriteError::Changed("the guardrails folder"));
    }
    // An upgrade writes new dispatchers into the folder: its DACL is still the private one.
    #[cfg(windows)]
    gitraptor_winsys::acl::verify_private_dir(&root)
        .map_err(|_| GuardWriteError::Changed("the DACL of the guardrails folder"))?;
    for f in files {
        let path = root.join(f.path);
        if let Some(parent) = path.parent()
            && parent != root
        {
            match std::fs::create_dir(parent) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    not_link(parent)?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        let temp = path.with_file_name(format!(
            "{}.{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
            temporary_name()
        ));
        let written = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(f.bytes)?;
            file.sync_all()?;
            not_link(&path)?;
            std::fs::rename(&temp, &path)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written?;
    }
    Ok(())
}

pub(super) fn remove_file_temporaries(
    common: &Path,
    expected: FileId,
    listed: &[&str],
) -> Result<()> {
    not_link(common)?;
    let root = common.join(super::FOLDER);
    match not_link(&root)? {
        None => return Ok(()),
        Some(m) if m.is_dir() => {}
        Some(_) => return Err(GuardWriteError::Changed("the guardrails folder")),
    }
    if id_of(&root)? != expected {
        return Err(GuardWriteError::Changed("the guardrails folder"));
    }
    for path in listed {
        let (dir, name) = match path.split_once('/') {
            None => (root.clone(), *path),
            Some((sub, name)) => {
                let dir = root.join(sub);
                if not_link(&dir)?.is_none() {
                    continue;
                }
                (dir, name)
            }
        };
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(candidate) = file_name.to_str() else {
                continue;
            };
            if !is_file_temporary(name, candidate) {
                continue;
            }
            let file = dir.join(candidate);
            // Regular files only: a link or a folder with the name is left in place.
            if std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()) {
                std::fs::remove_file(&file)?;
            }
        }
    }
    Ok(())
}

pub(super) fn remove_folder(
    common: &Path,
    name: &str,
    listed: &[&str],
    expected: Option<FileId>,
) -> Result<()> {
    not_link(common)?;
    let root = common.join(name);
    if not_link(&root)?.is_none() {
        return Ok(());
    }
    if let Some(expected) = expected
        && id_of(&root)? != expected
    {
        return Err(GuardWriteError::Changed("the guardrails folder"));
    }
    let mut subs = Vec::new();
    for path in listed {
        let file = root.join(path);
        if let Some((sub, _)) = path.split_once('/') {
            not_link(&root.join(sub))?;
            if !subs.contains(&sub) {
                subs.push(sub);
            }
        }
        if not_link(&file)?.is_some() {
            std::fs::remove_file(&file)?;
        }
    }
    for sub in subs {
        let _ = std::fs::remove_dir(root.join(sub));
    }
    let _ = std::fs::remove_dir(&root);
    Ok(())
}

pub(super) fn temporaries(common: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(common)? {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str()
            && is_temporary(name)
        {
            out.push(name.to_owned());
        }
    }
    Ok(out)
}
