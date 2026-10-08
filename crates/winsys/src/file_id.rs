//! Identity of a file or folder: what `(dev, ino)` is on Unix.

use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};

/// `(volume serial number, file index)` of `path` itself: a symbolic link or junction is never
/// followed, and a folder works too. Opened for attributes only and sharing everything, so it
/// never blocks another process. The index is 64 bits: unique on NTFS, not guaranteed on ReFS.
pub fn of_path(path: &Path) -> io::Result<(u32, u64)> {
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    crate::ffi_file::file_index(&file)
}

/// `(volume serial number, file index)` of what `path` names once links are followed: the
/// identity of an executable reached through a link is the file the link points to.
pub fn of_target(path: &Path) -> io::Result<(u32, u64)> {
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    crate::ffi_file::file_index(&file)
}

/// The stable identity of a file: what `(dev, ino, birth time)` is on Unix. The NTFS index
/// carries the MFT record's sequence number, so a freed record handed to a new file gives another
/// index. The creation time alone proves nothing: NTFS "tunneling" gives a file created under a
/// name deleted a moment ago the old file's creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub volume: u32,
    pub index: u64,
    /// `CreationTime`, 100 ns intervals since 1601-01-01 UTC.
    pub created_100ns: i64,
}

/// One directory entry opened by its path itself (a link or junction is never followed), sharing
/// everything so it never blocks another process. What it reads and what it deletes is the entry
/// the handle pins, whatever the path names afterwards.
pub struct Entry {
    file: std::fs::File,
    identity: Identity,
    attributes: u32,
    ntfs: bool,
}

impl Entry {
    /// Opens `path` itself. With `delete`, also with `DELETE` access, so [`Entry::delete`] works;
    /// a process holding the file open without `FILE_SHARE_DELETE` then makes this fail (see
    /// [`crate::fs::is_in_use`]).
    pub fn open(path: &Path, delete: bool) -> io::Result<Self> {
        let access = if delete {
            FILE_READ_ATTRIBUTES | DELETE
        } else {
            FILE_READ_ATTRIBUTES
        };
        let file = std::fs::OpenOptions::new()
            .access_mode(access)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let (volume, index) = crate::ffi_file::file_index(&file)?;
        let (created_100ns, attributes) = crate::ffi_file::creation_and_attributes(&file)?;
        let ntfs = crate::ffi_file::file_system_name(&file)?.eq_ignore_ascii_case("NTFS");
        Ok(Self {
            file,
            identity: Identity {
                volume,
                index,
                created_100ns,
            },
            attributes,
            ntfs,
        })
    }

    pub fn identity(&self) -> Identity {
        self.identity
    }

    /// Whether the entry lives on NTFS, the only file system where [`Identity`] is stable: on
    /// FAT and exFAT the index is the entry's slot in its folder, reused by the next file, and
    /// on ReFS the 64-bit index is not unique.
    pub fn is_ntfs(&self) -> bool {
        self.ntfs
    }

    /// A plain file: neither a folder nor a link, junction or other reparse point.
    pub fn is_regular_file(&self) -> bool {
        self.attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
    }

    /// Deletes this entry (opened with `delete`) and closes it. Fails if it was opened without.
    pub fn delete(self) -> io::Result<()> {
        crate::fs::delete_through(self.file)
    }
}

/// `ChangeTime` of `path` itself (never following a link): it moves on every write and cannot
/// be set back, so equal write time and size no longer hide a change.
pub fn change_time_of_path(path: &Path) -> io::Result<i64> {
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    crate::ffi_file::change_time(&file)
}

/// `ChangeTime` of an already open file.
pub fn change_time(file: &std::fs::File) -> io::Result<i64> {
    crate::ffi_file::change_time(file)
}

/// `(volume serial number, file index)` of an already open file or folder.
pub fn of_file(file: &std::fs::File) -> io::Result<(u32, u64)> {
    crate::ffi_file::file_index(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_file_has_another_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, "x").unwrap();
        let first = of_path(&path).unwrap();
        assert_eq!(of_path(&path).unwrap(), first);
        // A new file renamed over the old one while both exist.
        let new = dir.path().join("b");
        std::fs::write(&new, "x").unwrap();
        std::fs::rename(&new, &path).unwrap();
        assert_ne!(of_path(&path).unwrap(), first);
        assert!(of_path(dir.path()).is_ok(), "folders have an identity too");
        assert!(of_path(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn a_write_moves_the_change_time_even_with_the_old_write_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, "first").unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let before = change_time_of_path(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "secnd").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(mtime).unwrap();
        assert_ne!(change_time(&file).unwrap(), before);
        assert_eq!(
            change_time(&file).unwrap(),
            change_time_of_path(&path).unwrap()
        );
    }

    #[test]
    fn an_entry_reads_its_identity_and_deletes_exactly_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.lock");
        std::fs::write(&path, "").unwrap();
        let entry = Entry::open(&path, false).unwrap();
        let first = entry.identity();
        assert!(entry.is_regular_file());
        assert!(entry.is_ntfs(), "the test machine's temp folder is on NTFS");
        assert_eq!((first.volume, first.index), of_path(&path).unwrap());
        assert!(first.created_100ns > 0);
        assert!(entry.delete().is_err(), "opened without DELETE");

        // The same name taken again right away: NTFS tunneling may keep the creation time, the
        // index changes.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "").unwrap();
        let again = Entry::open(&path, true).unwrap();
        assert_ne!(again.identity().index, first.index);
        again.delete().unwrap();
        assert!(!path.exists());

        assert!(!Entry::open(dir.path(), false).unwrap().is_regular_file());
        assert_eq!(
            Entry::open(&dir.path().join("missing"), true)
                .err()
                .map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn two_spellings_of_one_path_are_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Index.Lock");
        std::fs::write(&path, "").unwrap();
        let upper = dir.path().join("INDEX.LOCK");
        assert_eq!(of_path(&upper).unwrap(), of_path(&path).unwrap());
        assert_eq!(
            Entry::open(&upper, false).unwrap().identity(),
            Entry::open(&path, false).unwrap().identity()
        );
        // The 8.3 name, where the volume keeps them.
        let long = dir.path().join("a long file name.lock");
        std::fs::write(&long, "").unwrap();
        let short = dir.path().join("ALONGF~1.LOC");
        if short.exists() {
            assert_eq!(of_path(&short).unwrap(), of_path(&long).unwrap());
        }
        assert_eq!(of_target(&path).unwrap(), of_path(&path).unwrap());
    }
}
