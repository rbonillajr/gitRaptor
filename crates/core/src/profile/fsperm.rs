//! Private folders and files of the profile (SEC-06).
//!
//! On Unix every folder is 0700 and every file 0600, created with those
//! modes from the start (no window with open permissions). A pre-existing
//! folder with another owner, another mode or that is a symlink stops the
//! engine; it is never "fixed" with chmod.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

use super::error::{ProfileError, Result};

/// Something the caller must surface to the user without failing the open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileWarning {
    /// Windows only: the ACL of the profile folders was not checked for
    /// entries of other users. Pending requirement before any Windows
    /// release (SEC-06; decision of 2026-10-04).
    AclNotVerified,
}

impl std::fmt::Display for ProfileWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AclNotVerified => write!(
                f,
                "profile folder ACL not verified: entries of other users are not checked on Windows yet"
            ),
        }
    }
}

/// Sets the process umask to 077 so anything created afterwards (including
/// files SQLite creates itself) starts private. Process-wide by design: the
/// daemon is the only writer of the profile (ADR-GRP-005).
pub fn set_restrictive_umask() {
    #[cfg(unix)]
    {
        use rustix::fs::Mode;
        rustix::process::umask(Mode::from_raw_mode(0o077));
    }
}

/// Creates `path` (and any missing ancestors) with mode 0700, or verifies it
/// if it already exists.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => verify_private_dir(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            create_dir_0700(path)?;
            verify_private_dir(path)
        }
        Err(err) => Err(err.into()),
    }
}

/// Fails unless `path` is a real folder owned by the current user with mode
/// exactly 0700.
pub fn verify_private_dir(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let insecure = |reason: String| ProfileError::InsecureDir {
        path: path.to_path_buf(),
        reason,
    };
    if meta.file_type().is_symlink() {
        return Err(insecure("is a symbolic link".into()));
    }
    if !meta.is_dir() {
        return Err(insecure("is not a directory".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let euid = rustix::process::geteuid().as_raw();
        if meta.uid() != euid {
            return Err(insecure(format!(
                "owned by uid {}, expected {euid}",
                meta.uid()
            )));
        }
        let mode = meta.mode() & 0o777;
        if mode != 0o700 {
            return Err(insecure(format!("mode is {mode:o}, expected 700")));
        }
    }
    Ok(())
}

/// Checks the Windows ACL of the profile folders.
///
/// Not implemented yet: verifying that the inherited ACL of `%LOCALAPPDATA%`
/// has no ACE for other users needs Win32 FFI (forbidden `unsafe`) or a new
/// crate, and cannot be tested from macOS. Instead of passing silently it
/// returns [`ProfileWarning::AclNotVerified`] and writes it to stderr.
/// Pending requirement before any Windows release (SEC-06).
#[cfg(windows)]
pub fn verify_windows_acl(_dirs: &[std::path::PathBuf]) -> Vec<ProfileWarning> {
    let warning = ProfileWarning::AclNotVerified;
    eprintln!("gitraptor: warning: {warning}");
    vec![warning]
}

/// Creates a new file with mode 0600, failing if it already exists and
/// without following a symlink at `path`. For the instance lock and logs.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // `create_new` already refuses a dangling symlink; O_NOFOLLOW makes
        // the intent explicit.
        options
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    options.open(path)
}

/// Forces mode 0600 on an existing file (used after moving a file into
/// quarantine).
pub fn set_private_file_mode(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn create_dir_0700(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn created_dirs_and_files_are_private() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let file = dir.join("lock");
        create_private_file(&file).unwrap();
        assert_eq!(mode(&file), 0o600);
        assert!(create_private_file(&file).is_err(), "must not reuse a file");
    }

    #[test]
    fn open_dir_is_rejected_not_fixed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("open");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let err = ensure_private_dir(&dir).unwrap_err();
        assert!(matches!(err, ProfileError::InsecureDir { .. }), "{err}");
        assert_eq!(mode(&dir), 0o755, "the folder must not be chmod-ed");
    }

    #[test]
    fn symlink_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        ensure_private_dir(&target).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            ensure_private_dir(&link),
            Err(ProfileError::InsecureDir { .. })
        ));
    }

    #[test]
    fn private_file_does_not_follow_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(create_private_file(&link).is_err());
        assert!(!target.exists());
    }
}
