//! File operations of the Guardrails write layer on Unix (M-03): everything relative to a
//! descriptor of the common directory, `O_NOFOLLOW`, exclusive creation and `fsync`.

use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use rustix::fs::{AtFlags, FileType, Mode, OFlags};

use super::{FileId, GuardWriteError, NewFile, Result, is_temporary, temporary_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Dir,
}

fn open_dir(path: &Path) -> Result<OwnedFd> {
    rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| match e {
        rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR => {
            GuardWriteError::Changed("a folder of the common directory")
        }
        e => GuardWriteError::Io(e.into()),
    })
}

fn open_subdir(dir: impl AsFd, name: &str) -> Result<OwnedFd> {
    rustix::fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| match e {
        rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR => {
            GuardWriteError::Changed("the guardrails folder")
        }
        e => GuardWriteError::Io(e.into()),
    })
}

// `st_dev`/`st_ino` have other widths on other Unix systems.
#[allow(clippy::unnecessary_cast)]
fn id(stat: &rustix::fs::Stat) -> FileId {
    FileId {
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
    }
}

pub(super) fn entry_id(common: &Path, name: &str, kind: Kind) -> Result<Option<FileId>> {
    let dir = open_dir(common)?;
    match rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            let ft = FileType::from_raw_mode(stat.st_mode);
            let ok = match kind {
                Kind::File => ft == FileType::RegularFile,
                Kind::Dir => ft == FileType::Directory,
            };
            if ok {
                Ok(Some(id(&stat)))
            } else {
                Err(GuardWriteError::Changed("an entry of the common directory"))
            }
        }
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(e) => Err(GuardWriteError::Io(e.into())),
    }
}

fn write_file(dir: impl AsFd, name: &str, bytes: &[u8], executable: bool) -> Result<()> {
    let mode = if executable {
        Mode::RWXU
    } else {
        Mode::RUSR | Mode::WUSR
    };
    let fd = rustix::fs::openat(
        dir,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        mode,
    )
    .map_err(|e| GuardWriteError::Io(e.into()))?;
    // The umask may have dropped bits: the mode is exactly the one of ADR-GRD-001 § 1.
    rustix::fs::fchmod(&fd, mode).map_err(|e| GuardWriteError::Io(e.into()))?;
    let mut file = std::fs::File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn write_folder(common: &Path, files: &[NewFile<'_>]) -> Result<FileId> {
    let root = open_dir(common)?;
    match rustix::fs::statat(&root, super::FOLDER, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => return Err(GuardWriteError::Exists),
        Err(rustix::io::Errno::NOENT) => {}
        Err(e) => return Err(GuardWriteError::Io(e.into())),
    }
    let temp = temporary_name();
    rustix::fs::mkdirat(&root, temp.as_str(), Mode::RWXU)
        .map_err(|e| GuardWriteError::Io(e.into()))?;
    let result = fill(&root, &temp, files);
    if let Err(e) = result {
        let listed: Vec<&str> = files.iter().map(|f| f.path).collect();
        let _ = remove_folder(common, &temp, &listed, None);
        return Err(e);
    }
    let renamed = rustix::fs::renameat_with(
        &root,
        temp.as_str(),
        &root,
        super::FOLDER,
        rustix::fs::RenameFlags::NOREPLACE,
    );
    match renamed {
        Ok(()) => {}
        // A file system without the flag: the absence was checked above.
        Err(rustix::io::Errno::INVAL | rustix::io::Errno::NOSYS) => {
            rustix::fs::renameat(&root, temp.as_str(), &root, super::FOLDER)
                .map_err(|e| GuardWriteError::Io(e.into()))?;
        }
        Err(e) => {
            let listed: Vec<&str> = files.iter().map(|f| f.path).collect();
            let _ = remove_folder(common, &temp, &listed, None);
            return Err(if e == rustix::io::Errno::EXIST {
                GuardWriteError::Exists
            } else {
                GuardWriteError::Io(e.into())
            });
        }
    }
    rustix::fs::fsync(&root).map_err(|e| GuardWriteError::Io(e.into()))?;
    let folder = open_subdir(&root, super::FOLDER)?;
    let stat = rustix::fs::fstat(&folder).map_err(|e| GuardWriteError::Io(e.into()))?;
    Ok(id(&stat))
}

fn fill(root: &OwnedFd, temp: &str, files: &[NewFile<'_>]) -> Result<()> {
    let dir = open_subdir(root, temp)?;
    for f in files {
        match f.path.split_once('/') {
            None => write_file(&dir, f.path, f.bytes, f.executable)?,
            Some((sub, name)) => {
                match rustix::fs::mkdirat(&dir, sub, Mode::RWXU) {
                    Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                    Err(e) => return Err(GuardWriteError::Io(e.into())),
                }
                let subdir = open_subdir(&dir, sub)?;
                write_file(&subdir, name, f.bytes, f.executable)?;
                rustix::fs::fsync(&subdir).map_err(|e| GuardWriteError::Io(e.into()))?;
            }
        }
    }
    rustix::fs::fsync(&dir).map_err(|e| GuardWriteError::Io(e.into()))?;
    Ok(())
}

pub(super) fn remove_folder(
    common: &Path,
    name: &str,
    listed: &[&str],
    expected: Option<FileId>,
) -> Result<()> {
    let root = open_dir(common)?;
    let dir = match open_subdir(&root, name) {
        Ok(dir) => dir,
        Err(GuardWriteError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if let Some(expected) = expected {
        let stat = rustix::fs::fstat(&dir).map_err(|e| GuardWriteError::Io(e.into()))?;
        if id(&stat) != expected {
            return Err(GuardWriteError::Changed("the guardrails folder"));
        }
    }
    let mut subs = Vec::new();
    for path in listed {
        let removed = match path.split_once('/') {
            None => unlink(&dir, path),
            Some((sub, file)) => {
                if !subs.contains(&sub) {
                    subs.push(sub);
                }
                match open_subdir(&dir, sub) {
                    Ok(subdir) => unlink(&subdir, file),
                    Err(GuardWriteError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
        };
        removed?;
    }
    for sub in subs {
        rmdir_if_empty(&dir, sub)?;
    }
    rmdir_if_empty(&root, name)?;
    rustix::fs::fsync(&root).map_err(|e| GuardWriteError::Io(e.into()))?;
    Ok(())
}

fn unlink(dir: impl AsFd, name: &str) -> Result<()> {
    match rustix::fs::unlinkat(dir, name, AtFlags::empty()) {
        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(e) => Err(GuardWriteError::Io(e.into())),
    }
}

fn rmdir_if_empty(dir: impl AsFd, name: &str) -> Result<()> {
    match rustix::fs::unlinkat(dir, name, AtFlags::REMOVEDIR) {
        Ok(()) | Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTEMPTY) => Ok(()),
        // Some systems report a non-empty folder as EEXIST.
        Err(rustix::io::Errno::EXIST) => Ok(()),
        Err(e) => Err(GuardWriteError::Io(e.into())),
    }
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
