//! Advisory locks on one byte of an open file.
//!
//! `File::lock` of std locks the whole file on Windows, and a byte-range lock there is
//! mandatory: no other handle can read the locked bytes. Locking a single byte far past the
//! content leaves the content readable (the PID in `daemon.lock`) while the lock still conflicts
//! as usual. Windows releases the lock when the handle closes, so a dead process never leaves
//! one behind.

use std::fs::File;
use std::io;

/// Takes a lock on the byte at `offset` without waiting: exclusive, or shared with other shared
/// locks. `Ok(false)` when another handle holds a conflicting lock.
pub fn try_lock_byte(file: &File, offset: u64, exclusive: bool) -> io::Result<bool> {
    crate::ffi_file::try_lock_byte(file, offset, exclusive)
}

/// Releases the lock on the byte at `offset` that this handle holds.
pub fn unlock_byte(file: &File, offset: u64) -> io::Result<()> {
    crate::ffi_file::unlock_byte(file, offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};

    const AT: u64 = 1 << 62;

    #[test]
    fn the_lock_conflicts_and_leaves_the_content_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let mut held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        assert!(try_lock_byte(&held, AT, true).unwrap());
        held.write_all(b"1234").unwrap();

        let mut other = std::fs::File::open(&path).unwrap();
        assert!(!try_lock_byte(&other, AT, true).unwrap());
        assert!(!try_lock_byte(&other, AT, false).unwrap());
        let mut text = String::new();
        other.seek(SeekFrom::Start(0)).unwrap();
        other.read_to_string(&mut text).unwrap();
        assert_eq!(text, "1234");

        unlock_byte(&held, AT).unwrap();
        assert!(try_lock_byte(&other, AT, false).unwrap());
        assert!(
            try_lock_byte(&held, AT, false).unwrap(),
            "shared locks coexist"
        );
    }

    #[test]
    fn closing_the_handle_releases_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let held = std::fs::File::create(&path).unwrap();
        assert!(try_lock_byte(&held, AT, true).unwrap());
        drop(held);
        let other = std::fs::File::open(&path).unwrap();
        assert!(try_lock_byte(&other, AT, true).unwrap());
    }
}
