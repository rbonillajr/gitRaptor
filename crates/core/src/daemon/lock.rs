//! Single daemon per user (ADR-GRP-005 § 2).
//!
//! An exclusive advisory lock of the OS (`flock` on Unix, `LockFileEx` on
//! Windows) on `daemon.lock` in the state folder of the profile. The OS
//! releases it when the process dies, so a crash never leaves a stale lock
//! that needs manual cleanup. The file itself is never deleted: deleting a
//! lock file races with a daemon that already opened it.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::DaemonError;

/// File name of the instance lock inside the state folder.
pub const LOCK_FILE: &str = "daemon.lock";

/// The held instance lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct InstanceLock {
    file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Takes the lock or fails with [`DaemonError::AlreadyRunning`] without
    /// waiting. On success the holder's PID is written into the file so
    /// `raptor daemon stop` can find it.
    pub fn acquire(state_dir: &Path) -> Result<Self, DaemonError> {
        let path = state_dir.join(LOCK_FILE);
        let mut file = open_lock_file(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(DaemonError::AlreadyRunning {
                    pid: read_pid(&mut file),
                });
            }
            Err(TryLockError::Error(err)) => return Err(err.into()),
        }
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(std::process::id().to_string().as_bytes())?;
        file.sync_all()?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Clears the PID (so a later `stop` never signals a reused PID) and
    /// releases the lock.
    pub fn release(self) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.sync_all()?;
        self.file.unlock()
    }
}

/// PID of the running daemon, or `None` if no daemon holds the lock.
///
/// Probes with a shared lock, which conflicts with the daemon's exclusive
/// one. If no daemon runs, the probe holds the shared lock for an instant;
/// a daemon starting in that instant exits as "already running" and the
/// client retries (TS-GRP-004).
pub fn running_pid(state_dir: &Path) -> Result<Option<u32>, DaemonError> {
    let path = state_dir.join(LOCK_FILE);
    let mut file = match open_existing(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    verify_lock_file(&file, &path)?;
    match file.try_lock_shared() {
        Ok(()) => {
            file.unlock()?;
            Ok(None)
        }
        Err(TryLockError::WouldBlock) => Ok(read_pid(&mut file)),
        Err(TryLockError::Error(err)) => Err(err.into()),
    }
}

/// Waits until no daemon holds the lock of `state_dir`, polling every
/// 50 ms. Returns `false` if it is still held after `timeout`.
pub fn wait_until_released(
    state_dir: &Path,
    timeout: std::time::Duration,
) -> Result<bool, DaemonError> {
    let start = std::time::Instant::now();
    loop {
        if running_pid(state_dir)?.is_none() {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Opens or creates the lock file: 0600, never through a symlink, and
/// refused unless it is a regular file of the current user with mode 0600
/// (SEC-06: never "fixed" with chmod).
fn open_lock_file(path: &Path) -> Result<File, DaemonError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    no_follow(&mut options);
    let file = options.open(path)?;
    verify_lock_file(&file, path)?;
    Ok(file)
}

fn open_existing(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    no_follow(&mut options);
    options.open(path)
}

fn no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    #[cfg(not(unix))]
    let _ = options;
}

fn verify_lock_file(file: &File, path: &Path) -> Result<(), DaemonError> {
    let meta = file.metadata()?;
    let insecure = |reason: &str| DaemonError::InsecureLock {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if !meta.is_file() {
        return Err(insecure("is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(insecure("is owned by another user"));
        }
        if meta.mode() & 0o777 != 0o600 {
            return Err(insecure("mode is not 600"));
        }
    }
    Ok(())
}

fn read_pid(file: &mut File) -> Option<u32> {
    let mut text = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    file.take(16).read_to_string(&mut text).ok()?;
    text.trim().parse().ok().filter(|pid| *pid > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::fsperm::ensure_private_dir;

    fn state_dir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        ensure_private_dir(&state).unwrap();
        (tmp, state)
    }

    #[test]
    fn second_acquire_fails_and_names_the_holder() {
        let (_tmp, state) = state_dir();
        let held = InstanceLock::acquire(&state).unwrap();
        match InstanceLock::acquire(&state) {
            Err(DaemonError::AlreadyRunning { pid }) => {
                assert_eq!(pid, Some(std::process::id()));
            }
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
        assert_eq!(running_pid(&state).unwrap(), Some(std::process::id()));
        held.release().unwrap();
        assert_eq!(running_pid(&state).unwrap(), None);
        InstanceLock::acquire(&state).unwrap();
    }

    #[test]
    fn no_lock_file_means_not_running() {
        let (_tmp, state) = state_dir();
        assert_eq!(running_pid(&state).unwrap(), None);
    }

    /// A child process (a `git` launched by the daemon) must not inherit the
    /// lock: otherwise it would keep it after the daemon dies.
    #[cfg(unix)]
    #[test]
    fn children_do_not_inherit_the_lock() {
        let (_tmp, state) = state_dir();
        let held = InstanceLock::acquire(&state).unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        drop(held);
        // On Linux, `spawn` returns once the child's address space is replaced,
        // a moment before `execve` closes its `O_CLOEXEC` descriptors, so the
        // lock can look held for an instant. A child that really inherited it
        // keeps it for the whole sleep, well past this deadline.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let again = loop {
            match InstanceLock::acquire(&state) {
                Err(DaemonError::AlreadyRunning { .. }) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                other => break other,
            }
        };
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(again.is_ok(), "the child kept the lock: {again:?}");
    }

    #[cfg(unix)]
    #[test]
    fn insecure_lock_file_is_refused_not_fixed() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, state) = state_dir();
        let path = state.join(LOCK_FILE);
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            InstanceLock::acquire(&state),
            Err(DaemonError::InsecureLock { .. })
        ));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_lock_file_is_refused() {
        let (tmp, state) = state_dir();
        let target = tmp.path().join("elsewhere");
        std::os::unix::fs::symlink(&target, state.join(LOCK_FILE)).unwrap();
        assert!(InstanceLock::acquire(&state).is_err());
        assert!(!target.exists());
    }
}
