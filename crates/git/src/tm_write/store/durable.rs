//! Durability and permission helpers of the store (ADR-TMC-001 § 4, SEC-TMC-01).
//!
//! A *plain* `fsync` per loose object: on macOS `std::fs::File::sync_all` issues `F_FULLFSYNC`,
//! which costs about 4 ms per object, so it is never used for objects. The full barrier is
//! issued once per capture.

use std::io;
use std::path::Path;

#[cfg(unix)]
use super::{Result, StoreError};

/// Plain `fsync` of a file, opened read-only.
pub(super) fn fsync_file(path: &Path) -> io::Result<()> {
    let file = std::fs::File::open(path)?;
    #[cfg(unix)]
    {
        rustix::fs::fsync(&file)?;
    }
    #[cfg(not(unix))]
    {
        file.sync_all()?;
    }
    Ok(())
}

/// `fsync` of a folder, so new entries in it survive a power loss (needed on Linux).
pub(super) fn fsync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let dir = std::fs::File::open(path)?;
        rustix::fs::fsync(&dir)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// One barrier that flushes the drive cache: `F_FULLFSYNC` on Apple systems, `fsync` on others.
pub(super) fn full_barrier(path: &Path) -> io::Result<()> {
    let file = std::fs::File::open(path)?;
    #[cfg(target_vendor = "apple")]
    {
        rustix::fs::fcntl_fullfsync(&file)?;
    }
    #[cfg(all(unix, not(target_vendor = "apple")))]
    {
        rustix::fs::fsync(&file)?;
    }
    #[cfg(not(unix))]
    {
        file.sync_all()?;
    }
    Ok(())
}

/// Nanoseconds of the wall clock, to name temporary folders.
#[cfg(unix)]
pub(super) fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Creates a folder with mode 0700 from the start.
#[cfg(unix)]
pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

/// Writes a new file with mode 0600 from the start, synced.
#[cfg(unix)]
pub(super) fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    rustix::fs::fsync(&file)?;
    Ok(())
}

/// A folder of the store must be a real folder (not a symlink) of the current user, closed to
/// everyone else. Never fixed with `chmod` (ADR-GRP-006 § 1).
#[cfg(unix)]
pub(super) fn check_private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = path
        .symlink_metadata()
        .map_err(|e| StoreError::Untrusted(format!("{}: {e}", path.display())))?;
    if !meta.is_dir() {
        return Err(StoreError::Untrusted(format!(
            "{} is not a folder",
            path.display()
        )));
    }
    if meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(StoreError::Untrusted(format!(
            "{} has another owner",
            path.display()
        )));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(StoreError::Untrusted(format!(
            "{} is open to other users",
            path.display()
        )));
    }
    Ok(())
}
