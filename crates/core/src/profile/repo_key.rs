//! Repo key normalization (ADR-GRP-006 § 3) and input path checks (SEC-02).
//!
//! A repo is identified by the canonical path of its Git common directory,
//! which every worktree of the repo shares. The caller resolves the common
//! directory (`crates/git`); this module only validates and normalizes it.

use std::path::{Path, PathBuf};

use super::error::{ProfileError, Result};

/// The two forms of a normalized common directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPath {
    /// Unique lookup key: canonical path, lowercased on case-insensitive
    /// systems (macOS, Windows).
    pub key_path: String,
    /// Canonical path as found on disk, for display.
    pub canonical_path: PathBuf,
}

/// Validates and normalizes the Git common directory of a repo.
pub fn normalize_common_dir(common_dir: &Path) -> Result<NormalizedPath> {
    validate_input_path(common_dir)?;
    let canonical_path = common_dir.canonicalize()?;
    let display = canonical_path
        .to_str()
        .ok_or_else(|| ProfileError::InvalidPath {
            path: common_dir.to_path_buf(),
            reason: "path is not valid UTF-8".into(),
        })?;
    Ok(NormalizedPath {
        key_path: key_from_canonical(display, case_insensitive_fs()),
        canonical_path,
    })
}

/// Paths are compared case-insensitively where the default file system
/// ignores case. Known limitation: a case-sensitive APFS volume on macOS is
/// treated as case-insensitive too.
fn case_insensitive_fs() -> bool {
    cfg!(any(target_os = "macos", windows))
}

fn key_from_canonical(canonical: &str, case_insensitive: bool) -> String {
    if case_insensitive {
        canonical.to_lowercase()
    } else {
        canonical.to_owned()
    }
}

/// Rejects paths that must never reach the file system: relative paths
/// everywhere and, on Windows, UNC, `\\?\`, `\\.\`, reserved devices and
/// alternate data streams.
pub fn validate_input_path(path: &Path) -> Result<()> {
    let invalid = |reason: &str| ProfileError::InvalidPath {
        path: path.to_path_buf(),
        reason: reason.into(),
    };
    if !path.is_absolute() {
        return Err(invalid("path must be absolute"));
    }
    if cfg!(windows) {
        let text = path
            .to_str()
            .ok_or_else(|| invalid("path is not valid UTF-8"))?;
        if let Some(reason) = windows_path_problem(text) {
            return Err(invalid(reason));
        }
    }
    Ok(())
}

/// Pure check of a Windows path spelling, so it is tested on every OS.
fn windows_path_problem(path: &str) -> Option<&'static str> {
    let normalized = path.replace('/', "\\");
    if normalized.starts_with("\\\\?\\") || normalized.starts_with("\\\\.\\") {
        return Some("device and verbatim paths are not allowed");
    }
    if normalized.starts_with("\\\\") {
        return Some("UNC paths are not allowed");
    }
    // A drive letter is the only place a colon may appear.
    let bytes = normalized.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let rest = if has_drive {
        &normalized[2..]
    } else {
        &normalized[..]
    };
    if rest.contains(':') {
        return Some("alternate data streams are not allowed");
    }
    const DEVICES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "COM", "LPT"];
    for component in rest.split('\\').filter(|c| !c.is_empty()) {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let is_device = DEVICES.iter().any(|d| {
            stem == *d
                || (matches!(*d, "COM" | "LPT")
                    && stem.len() == 4
                    && stem.starts_with(d)
                    && stem.as_bytes()[3].is_ascii_digit())
        });
        if is_device {
            return Some("reserved device names are not allowed");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_path_validation() {
        for ok in [
            r"C:\Users\u\repo\.git",
            "C:/Users/u/repo/.git",
            r"D:\src\console\.git",
        ] {
            assert_eq!(windows_path_problem(ok), None, "{ok}");
        }
        for bad in [
            r"\\server\share\repo\.git",
            r"\\?\C:\repo\.git",
            r"\\.\pipe\x",
            r"C:\repo\.git:stream",
            r"C:\repo\CON\.git",
            r"C:\repo\nul.txt",
            r"C:\repo\COM1\.git",
            "//server/share/.git",
        ] {
            assert!(windows_path_problem(bad).is_some(), "{bad}");
        }
    }

    #[test]
    fn relative_paths_are_rejected() {
        assert!(matches!(
            validate_input_path(Path::new("repo/.git")),
            Err(ProfileError::InvalidPath { .. })
        ));
    }

    #[test]
    fn key_is_lowercased_only_when_case_insensitive() {
        assert_eq!(key_from_canonical("/A/Repo/.git", true), "/a/repo/.git");
        assert_eq!(key_from_canonical("/A/Repo/.git", false), "/A/Repo/.git");
    }

    #[test]
    fn symlinks_resolve_to_the_same_key() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real.git");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link.git");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();
        assert_eq!(
            normalize_common_dir(&real).unwrap().key_path,
            normalize_common_dir(&link).unwrap().key_path
        );
    }
}
