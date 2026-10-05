//! Path checks that run before the file system is touched (SEC-02, ADR-GRP-009 § 2).
//!
//! On Windows, opening a UNC path would make the OS authenticate against a remote host and leak
//! the NTLM hash (M9), so these paths are rejected as plain strings.

use std::path::Path;

use crate::ReadError;

/// Device names that Windows resolves anywhere in a path.
const WINDOWS_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "CONIN$",
    "CONOUT$",
];

/// Validate a repository or worktree path with the rules of the current OS.
pub fn validate(path: &Path) -> Result<(), ReadError> {
    validate_with_rules(path, cfg!(windows))
}

/// Validate `path` with the Windows rules when `windows` is true. Public so both rule sets can be
/// tested on any OS.
pub fn validate_with_rules(path: &Path, windows: bool) -> Result<(), ReadError> {
    let reject = |why: &str| Err(ReadError::InvalidInput(format!("path rejected: {why}")));
    let Some(text) = path.to_str() else {
        return reject("not valid UTF-8");
    };
    if text.contains('\0') {
        return reject("contains NUL");
    }
    // UNC (`\\server\share`, `//server/share`), verbatim (`\\?\`) and device (`\\.\`) paths.
    if text.starts_with("\\\\") || text.starts_with("//") || (windows && text.starts_with("/\\")) {
        return reject("UNC, verbatim or device path");
    }
    if windows {
        if !is_windows_drive_absolute(text) {
            return reject("not an absolute drive path");
        }
        for component in text[3..].split(['\\', '/']).filter(|c| !c.is_empty()) {
            if component.contains(':') {
                return reject("alternate data stream");
            }
            let stem = component.split('.').next().unwrap_or(component).trim_end();
            if WINDOWS_DEVICE_NAMES
                .iter()
                .any(|d| d.eq_ignore_ascii_case(stem))
            {
                return reject("device name");
            }
        }
    } else if !text.starts_with('/') {
        // Lexical, not `Path::is_absolute`: the Unix rules must hold on a Windows host too.
        return reject("not absolute");
    }
    Ok(())
}

fn is_windows_drive_absolute(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(p: &str, windows: bool) -> bool {
        validate_with_rules(Path::new(p), windows).is_ok()
    }

    #[test]
    fn unix_rules() {
        assert!(ok("/home/dev/repo", false));
        assert!(!ok("relative/repo", false));
        assert!(!ok("//server/share/repo", false));
        assert!(!ok("\\\\server\\share", false));
        assert!(!ok(r"C:\Users\dev\repo", false));
    }

    /// The rules of the running OS: `validate` accepts its own absolute paths and refuses UNC.
    #[test]
    fn current_os_rules() {
        #[cfg(unix)]
        {
            assert!(validate(Path::new("/home/dev/repo")).is_ok());
            assert!(validate(Path::new(r"C:\Users\dev\repo")).is_err());
        }
        #[cfg(windows)]
        {
            assert!(validate(Path::new(r"C:\Users\dev\repo")).is_ok());
            assert!(validate(Path::new(r"\\server\share\repo")).is_err());
            assert!(validate(Path::new(r"\\?\C:\repo")).is_err());
            assert!(validate(Path::new("/home/dev/repo")).is_err());
        }
    }

    #[test]
    fn windows_rules_reject_unc_device_and_ads() {
        assert!(ok(r"C:\Users\dev\repo", true));
        assert!(ok("C:/Users/dev/repo", true));
        assert!(!ok(r"\\server\share\repo", true));
        assert!(!ok(r"\\?\C:\repo", true));
        assert!(!ok(r"\\.\PhysicalDrive0", true));
        assert!(!ok("//server/share", true));
        assert!(!ok(r"C:\repo\NUL", true));
        assert!(!ok(r"C:\repo\com1.txt", true));
        assert!(!ok(r"C:\repo\file.txt:stream", true));
        assert!(!ok(r"repo\relative", true));
    }
}
