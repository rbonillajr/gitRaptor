//! File system operations the safe API of `std` does not offer on Windows.

use std::io;
use std::path::Path;

/// `ERROR_ACCESS_DENIED`, `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`.
const IN_USE: [i32; 3] = [5, 32, 33];
/// `ERROR_FILE_EXISTS` and `ERROR_ALREADY_EXISTS`.
const EXISTS: [i32; 2] = [80, 183];

/// Renames the entry `from` to `to` (same folder or volume) with `MOVEFILE_WRITE_THROUGH`.
/// Never replaces `to` ([`io::ErrorKind::AlreadyExists`] if it is taken) and never follows a
/// link or junction at `from`: the entry itself moves.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    crate::ffi_file::move_no_replace(from, to).map_err(|e| {
        if e.raw_os_error().is_some_and(|c| EXISTS.contains(&c)) {
            io::Error::new(io::ErrorKind::AlreadyExists, e)
        } else {
            e
        }
    })
}

/// Deletes the file of `file` (opened with `DELETE` access) when this handle closes, and closes
/// it: the entry deleted is the one read through the handle, whatever the path names now.
pub fn delete_through(file: std::fs::File) -> io::Result<()> {
    crate::ffi_file::delete_on_close(&file)?;
    drop(file);
    Ok(())
}

/// Whether `err` means another process holds the file open in a way that forbids the operation
/// (an editor without `FILE_SHARE_DELETE`, an antivirus scan): the file was not touched.
pub fn is_in_use(err: &io::Error) -> bool {
    err.raw_os_error().is_some_and(|c| IN_USE.contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn renames_and_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let err = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        std::fs::remove_file(&b).unwrap();
        rename_no_replace(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
        assert!(!a.exists());
    }

    #[test]
    fn a_file_open_without_share_delete_is_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::write(&a, "a").unwrap();
        // FILE_SHARE_READ | FILE_SHARE_WRITE: what many editors use.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&a)
            .unwrap();
        let err = rename_no_replace(&a, &dir.path().join("b")).unwrap_err();
        assert!(is_in_use(&err), "{err:?}");
        drop(held);
        rename_no_replace(&a, &dir.path().join("b")).unwrap();
    }
}
