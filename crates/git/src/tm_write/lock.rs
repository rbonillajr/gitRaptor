//! Git's lock protocol (ADR-TMC-002 § 3, step 3): `<file>.lock` created exclusively, written,
//! synced and renamed over `<file>`. A lock that is already there belongs to someone else: it
//! is reported as "Git busy" and never removed.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Result, WriteError};

/// Identity of a lock file: device and inode (and birth time where the OS keeps it), so a lock
/// is only released or committed while it is still ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockIdentity {
    pub dev: u64,
    pub inode: u64,
}

/// A Git lock this process holds.
#[derive(Debug)]
pub struct GitLock {
    target: PathBuf,
    lock: PathBuf,
    file: Option<File>,
    identity: Option<LockIdentity>,
    done: bool,
}

impl GitLock {
    /// Takes `<target>.lock`. Fails with [`WriteError::Busy`] if it exists; never follows a link.
    pub fn acquire(target: &Path) -> Result<Self> {
        let mut name = target
            .file_name()
            .ok_or_else(|| WriteError::InvalidInput("lock target without a name".into()))?
            .to_os_string();
        name.push(".lock");
        let lock = target.with_file_name(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o666)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
        }
        let file = match options.open(&lock) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(WriteError::Busy(lock));
            }
            Err(e) => return Err(e.into()),
        };
        let identity = identity_of(&file);
        Ok(Self {
            target: target.to_owned(),
            lock,
            file: Some(file),
            identity,
            done: false,
        })
    }

    /// Path of the `.lock` file.
    pub fn path(&self) -> &Path {
        &self.lock
    }

    pub fn identity(&self) -> Option<LockIdentity> {
        self.identity
    }

    /// Whether the file at the lock path is still the one this process created.
    pub fn is_ours(&self) -> bool {
        match (self.identity, std::fs::symlink_metadata(&self.lock)) {
            (Some(id), Ok(meta)) => meta_identity(&meta) == Some(id),
            // Without an identity (Windows, pending) only presence can be checked.
            (None, Ok(meta)) => meta.is_file(),
            _ => false,
        }
    }

    /// Writes `bytes` into the lock, syncs it and renames it over the target, then syncs the
    /// folder. The lock must still be ours.
    pub fn commit(mut self, bytes: &[u8]) -> Result<()> {
        let mut file = self.file.take().expect("lock file open until commit");
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if !self.is_ours() {
            self.done = true;
            return Err(WriteError::Busy(self.lock.clone()));
        }
        std::fs::rename(&self.lock, &self.target)?;
        self.done = true;
        sync_parent(&self.target);
        Ok(())
    }

    /// Removes the lock without touching the target, only if it is still ours.
    pub fn release(mut self) -> Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.done = true;
        self.file.take();
        if self.is_ours() {
            std::fs::remove_file(&self.lock)?;
        }
        Ok(())
    }
}

impl Drop for GitLock {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

#[cfg(unix)]
fn identity_of(file: &File) -> Option<LockIdentity> {
    file.metadata().ok().and_then(|m| meta_identity(&m))
}

#[cfg(not(unix))]
fn identity_of(_file: &File) -> Option<LockIdentity> {
    None
}

#[cfg(unix)]
fn meta_identity(meta: &std::fs::Metadata) -> Option<LockIdentity> {
    use std::os::unix::fs::MetadataExt;
    meta.is_file().then(|| LockIdentity {
        dev: meta.dev(),
        inode: meta.ino(),
    })
}

#[cfg(not(unix))]
fn meta_identity(_meta: &std::fs::Metadata) -> Option<LockIdentity> {
    None
}

/// `fsync` of the folder that holds `path`, so a rename survives a power loss.
pub(crate) fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = rustix::fs::fsync(&dir);
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foreign_lock_is_busy_and_stays() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("index");
        std::fs::write(dir.path().join("index.lock"), b"theirs").unwrap();
        let err = GitLock::acquire(&target).unwrap_err();
        assert!(matches!(err, WriteError::Busy(_)));
        assert_eq!(
            std::fs::read(dir.path().join("index.lock")).unwrap(),
            b"theirs"
        );
    }

    #[test]
    fn commit_renames_and_drop_releases() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("HEAD");
        std::fs::write(&target, b"old").unwrap();
        let lock = GitLock::acquire(&target).unwrap();
        lock.commit(b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!dir.path().join("HEAD.lock").exists());

        let lock = GitLock::acquire(&target).unwrap();
        drop(lock);
        assert!(!dir.path().join("HEAD.lock").exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn a_replaced_lock_is_never_removed_or_committed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("index");
        let lock = GitLock::acquire(&target).unwrap();
        // Someone removes ours and takes their own at the same path.
        std::fs::remove_file(lock.path()).unwrap();
        std::fs::write(dir.path().join("tmp"), b"theirs").unwrap();
        std::fs::rename(dir.path().join("tmp"), dir.path().join("index.lock")).unwrap();
        assert!(!lock.is_ours());
        assert!(matches!(lock.commit(b"x"), Err(WriteError::Busy(_))));
        assert_eq!(
            std::fs::read(dir.path().join("index.lock")).unwrap(),
            b"theirs"
        );
        assert!(!target.exists());
    }
}
