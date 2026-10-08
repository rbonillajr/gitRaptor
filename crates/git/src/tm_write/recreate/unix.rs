//! Folders of a recreation held by descriptor: every entry is opened or created relative to an
//! open folder with `O_NOFOLLOW`, so a link put in place after a check is refused, never followed.

use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, Mode, OFlags};
use rustix::io::Errno;

use super::super::{Result, WriteError};

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

fn io(e: Errno) -> WriteError {
    WriteError::Io(e.into())
}

/// An open folder.
pub struct Dir {
    fd: OwnedFd,
}

/// `(device, inode)` of a `stat`.
fn identity(stat: &rustix::fs::Stat) -> (u64, u64) {
    // `st_dev` is `i32` on macOS and `u64` on Linux: the cast is needed on one of them.
    #[allow(clippy::unnecessary_cast)]
    (stat.st_dev as u64, stat.st_ino as u64)
}

impl Dir {
    /// Opens the folder at the canonical `path`, then checks that `path` still names it: a folder
    /// on the way swapped for a link meanwhile would give another one.
    pub fn open_canonical(path: &Path) -> Result<Self> {
        let fd = match rustix::fs::open(path, DIR_FLAGS, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::LOOP | Errno::NOTDIR) => return Err(moved(path)),
            Err(e) => return Err(io(e)),
        };
        let opened = identity(&rustix::fs::fstat(&fd).map_err(io)?);
        if crate::paths::canonicalize(path)? != path
            || identity(&rustix::fs::lstat(path).map_err(io)?) != opened
        {
            return Err(moved(path));
        }
        Ok(Self { fd })
    }

    /// Opens folder `name`, which must be a real folder: `Ok(None)` if it is a link or anything
    /// else. With `create`, a missing folder is created first (mode from the umask).
    pub fn child_dir(&self, name: &str, create: bool) -> Result<Option<Self>> {
        let open = || rustix::fs::openat(&self.fd, name, DIR_FLAGS, Mode::empty());
        let fd = match open() {
            Err(Errno::NOENT) if create => {
                match rustix::fs::mkdirat(&self.fd, name, Mode::from_raw_mode(0o777)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err(io(e)),
                }
                open()
            }
            other => other,
        };
        match fd {
            Ok(fd) => Ok(Some(Self { fd })),
            Err(Errno::LOOP | Errno::NOTDIR) => Ok(None),
            Err(e) => Err(io(e)),
        }
    }

    pub fn is_empty(&self) -> Result<bool> {
        let mut dir = rustix::fs::Dir::read_from(&self.fd).map_err(io)?;
        while let Some(entry) = dir.read() {
            let name = entry.map_err(io)?;
            if !matches!(name.file_name().to_bytes(), b"." | b"..") {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Creates folder `name`, failing with `AlreadyExists` if anything is there.
    pub fn mkdir_new(&self, name: &str) -> std::io::Result<()> {
        rustix::fs::mkdirat(&self.fd, name, Mode::from_raw_mode(0o777)).map_err(Into::into)
    }

    /// Writes a new file, exclusively and without following a link, and syncs it.
    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let fd = rustix::fs::openat(
            &self.fd,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
        )
        .map_err(io)?;
        let mut file = std::fs::File::from(fd);
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }

    /// Whether `name` in `parent` is still this folder.
    pub fn is_at(&self, parent: &Self, name: &str) -> Result<bool> {
        let here = identity(&rustix::fs::fstat(&self.fd).map_err(io)?);
        match rustix::fs::statat(&parent.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Ok(identity(&stat) == here),
            Err(Errno::NOENT) => Ok(false),
            Err(e) => Err(io(e)),
        }
    }

    /// Whether `path` names this folder now, itself and not through a link.
    pub fn is_at_path(&self, path: &Path) -> Result<bool> {
        let here = identity(&rustix::fs::fstat(&self.fd).map_err(io)?);
        match rustix::fs::lstat(path) {
            Ok(stat) => Ok(identity(&stat) == here),
            Err(Errno::NOENT) => Ok(false),
            Err(e) => Err(io(e)),
        }
    }

    /// Removes file `name`; a link is removed itself, never followed.
    pub fn remove_file(&self, name: &str) -> std::io::Result<()> {
        rustix::fs::unlinkat(&self.fd, name, AtFlags::empty()).map_err(Into::into)
    }

    /// Removes folder `name` if it is an empty folder; anything else is left in place.
    pub fn remove_dir(&self, name: &str) -> std::io::Result<()> {
        rustix::fs::unlinkat(&self.fd, name, AtFlags::REMOVEDIR).map_err(Into::into)
    }

    /// `fsync` of the folder, so what was created in it survives a power loss.
    pub fn sync(&self) {
        let _ = rustix::fs::fsync(&self.fd);
    }
}

fn moved(path: &Path) -> WriteError {
    WriteError::InvalidInput(format!(
        "cannot recreate worktree: {} moved or is a link",
        path.display()
    ))
}
