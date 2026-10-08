//! Folders of a recreation held open on Windows. Each one is opened with
//! `FILE_FLAG_OPEN_REPARSE_POINT` (a link or a junction is opened itself, never followed, and then
//! refused) and without `FILE_SHARE_DELETE`, and stays open while the recreation runs: nobody can
//! rename, remove or swap it for a junction meanwhile, so the paths beneath it keep naming what
//! was checked. The handle asks for `FILE_LIST_DIRECTORY`: an open for attributes only takes no
//! part in share access, and would not keep a `DELETE` open (a rename) out.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use gitraptor_winsys::file_id;

use super::super::{Result, WriteError};

const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_LIST_DIRECTORY: u32 = 0x1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// An open folder, pinned.
pub struct Dir {
    handle: File,
    path: PathBuf,
}

/// Opens `path` itself, never following it, and without letting anyone rename or delete it
/// while the handle lives.
fn pin(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// A folder that is neither a link, nor a junction, nor any other reparse point.
fn is_real_dir(file: &File) -> std::io::Result<bool> {
    let attributes = file.metadata()?.file_attributes();
    Ok(
        attributes & FILE_ATTRIBUTE_DIRECTORY != 0
            && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
    )
}

impl Dir {
    /// Pins the folder at the canonical `path`, then checks that `path` still names it.
    pub fn open_canonical(path: &Path) -> Result<Self> {
        let handle = pin(path)?;
        if !is_real_dir(&handle)?
            || crate::paths::canonicalize(path)? != path
            || file_id::of_path(path)? != file_id::of_file(&handle)?
        {
            return Err(moved(path));
        }
        Ok(Self {
            handle,
            path: path.to_owned(),
        })
    }

    /// Pins folder `name`, which must be a real folder: `Ok(None)` if it is a link, a junction or
    /// anything else. With `create`, a missing folder is created first.
    pub fn child_dir(&self, name: &str, create: bool) -> Result<Option<Self>> {
        let path = self.path.join(name);
        let handle = match pin(&path) {
            Err(e) if create && e.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.into()),
                }
                pin(&path)?
            }
            other => other?,
        };
        Ok(is_real_dir(&handle)?.then_some(Self { handle, path }))
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(std::fs::read_dir(&self.path)?.next().is_none())
    }

    /// Creates folder `name`, failing with `AlreadyExists` if anything is there.
    pub fn mkdir_new(&self, name: &str) -> std::io::Result<()> {
        std::fs::create_dir(self.path.join(name))
    }

    /// Writes a new file, exclusively (`CREATE_NEW` fails on an existing link too), and syncs it.
    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.path.join(name))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }

    /// Whether `name` in `parent` is still this folder.
    pub fn is_at(&self, parent: &Self, name: &str) -> Result<bool> {
        self.is_at_path(&parent.path.join(name))
    }

    /// Whether `path` names this folder now, itself and not through a link.
    pub fn is_at_path(&self, path: &Path) -> Result<bool> {
        match file_id::of_path(path) {
            Ok(id) => Ok(id == file_id::of_file(&self.handle)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Removes file `name`; a link is removed itself, never followed.
    pub fn remove_file(&self, name: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.path.join(name))
    }

    /// Removes folder `name` if it is an empty real folder; anything else is left in place.
    pub fn remove_dir(&self, name: &str) -> std::io::Result<()> {
        let path = self.path.join(name);
        if !is_real_dir(&pin(&path)?)? {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        std::fs::remove_dir(path)
    }

    /// Windows has no `fsync` of a folder: the entries are on disk once their files are synced.
    pub fn sync(&self) {}
}

fn moved(path: &Path) -> WriteError {
    WriteError::InvalidInput(format!(
        "cannot recreate worktree: {} moved or is a link",
        path.display()
    ))
}
