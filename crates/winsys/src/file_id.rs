//! Identity of a file or folder: what `(dev, ino)` is on Unix.

use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
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
}
