//! The profile's `settings.json` (ADR-GRP-007), parsed with the common
//! loader of TS-GRD-001 at the profile level.
//!
//! Minimal reader for the profile-only keys the engine needs now
//! (`timeMachine.includeCredentialFiles`, US-TMC-001). US-GRP-013 adds the
//! local level. Read on every use: the developer can change it without
//! restarting the daemon.

use std::io::Read;

use gitraptor_policy::settings::{
    Code, Diagnostic, Level, Limit, Parsed, SourceKind, parse_document,
};

use super::ProfileDirs;

/// File name of the profile settings in the profile's config folder.
pub const PROFILE_SETTINGS_FILE: &str = "settings.json";

/// Largest settings document read (L-03).
const MAX_BYTES: u64 = 64 * 1024;

/// The profile settings document. Absent when there is no file; ignored as
/// a whole when it is not a regular file, is too large or cannot be read.
/// A link is never followed.
pub fn profile_settings(dirs: &ProfileDirs) -> Parsed {
    let path = dirs.config.join(PROFILE_SETTINGS_FILE);
    let ignored = |code| Parsed::ignored(Diagnostic::new(code, SourceKind::Profile));
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Parsed::absent(),
        Err(_) => return ignored(Code::Unreadable),
    };
    if meta.file_type().is_symlink() {
        return ignored(Code::Symlink);
    }
    if !meta.file_type().is_file() {
        return ignored(Code::NotAFile);
    }
    if meta.len() > MAX_BYTES {
        return ignored(Code::LimitExceeded(Limit::Size));
    }
    let mut bytes = Vec::new();
    let read = open_no_follow(&path).and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes));
    if read.is_err() {
        return ignored(Code::Unreadable);
    }
    if bytes.len() as u64 > MAX_BYTES {
        return ignored(Code::LimitExceeded(Limit::Size));
    }
    parse_document(&bytes, Level::Profile, SourceKind::Profile)
}

#[cfg(unix)]
fn open_no_follow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits()).unwrap_or(0))
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// `timeMachine.includeCredentialFiles` of the profile; `false` when it is
/// not set or the document cannot be applied (fail-safe: credentials stay
/// out, BR-TMC-CONS-002).
pub fn include_credential_files(dirs: &ProfileDirs) -> bool {
    profile_settings(dirs)
        .applicable()
        .and_then(|s| s.time_machine.as_ref())
        .and_then(|tm| tm.include_credential_files)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> (tempfile::TempDir, ProfileDirs) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path());
        std::fs::create_dir_all(&dirs.config).unwrap();
        (tmp, dirs)
    }

    fn write(dirs: &ProfileDirs, text: &str) {
        std::fs::write(dirs.config.join(PROFILE_SETTINGS_FILE), text).unwrap();
    }

    #[test]
    fn absent_file_means_credentials_stay_out() {
        let (_tmp, dirs) = dirs();
        assert!(!include_credential_files(&dirs));
    }

    #[test]
    fn the_profile_can_include_credentials() {
        let (_tmp, dirs) = dirs();
        write(
            &dirs,
            r#"{"timeMachine": {"includeCredentialFiles": true}}"#,
        );
        assert!(include_credential_files(&dirs));
        write(
            &dirs,
            r#"{"timeMachine": {"includeCredentialFiles": false}}"#,
        );
        assert!(!include_credential_files(&dirs));
    }

    #[test]
    fn an_invalid_or_oversized_document_keeps_credentials_out() {
        let (_tmp, dirs) = dirs();
        write(
            &dirs,
            r#"{"timeMachine": {"includeCredentialFiles": "yes"}}"#,
        );
        assert!(!include_credential_files(&dirs));
        write(&dirs, "{not json");
        assert!(!include_credential_files(&dirs));
        let pad = " ".repeat(usize::try_from(MAX_BYTES).unwrap());
        write(
            &dirs,
            &format!(r#"{{"timeMachine": {{"includeCredentialFiles": true}}}}{pad}"#),
        );
        assert!(!include_credential_files(&dirs));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed() {
        let (tmp, dirs) = dirs();
        let target = tmp.path().join("elsewhere.json");
        std::fs::write(
            &target,
            r#"{"timeMachine": {"includeCredentialFiles": true}}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, dirs.config.join(PROFILE_SETTINGS_FILE)).unwrap();
        assert!(!include_credential_files(&dirs));
    }
}
