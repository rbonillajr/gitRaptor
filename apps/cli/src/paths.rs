//! The one place where the client turns a folder into the path it sends to the engine.
//!
//! `std::fs::canonicalize` answers `\\?\C:\…` on Windows, which the engine refuses as a device
//! path (`channel::validate::client_path`): `raptor undo` failed with "invalid parameters" and
//! `repo add` and the MCP before it (#153, #187). Every path that leaves the client for the
//! engine goes through [`canonicalize`].

use std::path::{Path, PathBuf};

/// `std::fs::canonicalize` in the form the engine accepts: a `\\?\C:\…` path drops its prefix.
/// Anything else (`\\?\UNC\…`, Unix paths) is kept as it is.
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(without_verbatim_drive)
}

/// `\\?\C:\…` as `C:\…`; any other path unchanged.
pub fn without_verbatim_drive(path: PathBuf) -> PathBuf {
    let plain = path.to_str().and_then(|text| {
        let rest = text.strip_prefix(r"\\?\")?;
        let b = rest.as_bytes();
        (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\')
            .then(|| PathBuf::from(rest))
    });
    plain.unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_drive_path_loses_its_prefix() {
        let plain = |p: &str| without_verbatim_drive(PathBuf::from(p));
        assert_eq!(plain(r"\\?\C:\src\repo"), PathBuf::from(r"C:\src\repo"));
        assert_eq!(
            plain(r"\\?\UNC\host\share"),
            PathBuf::from(r"\\?\UNC\host\share")
        );
        assert_eq!(plain("/Users/u/repo"), PathBuf::from("/Users/u/repo"));
    }

    /// What the engine receives from the client never has the verbatim prefix (the `raptor
    /// undo` bug of Windows): on every OS the canonical form of a real folder is a plain one.
    #[test]
    fn the_canonical_path_of_a_real_folder_is_never_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonicalize(dir.path()).unwrap();
        assert!(!canonical.to_string_lossy().starts_with(r"\\?\"));
        assert!(canonical.is_absolute());
    }
}
