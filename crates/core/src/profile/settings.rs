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
const MAX_BYTES: u64 = gitraptor_policy::settings::strict::MAX_BYTES as u64;

/// The profile settings document. Absent when there is no file; ignored as
/// a whole when it is not a regular file, is too large or cannot be read.
/// A link is never followed.
pub fn profile_settings(dirs: &ProfileDirs) -> Parsed {
    read_settings(
        &dirs.config.join(PROFILE_SETTINGS_FILE),
        Level::Profile,
        SourceKind::Profile,
    )
}

/// Reads one settings file without following links, bounded to [`MAX_BYTES`]. Never creates
/// anything. Absent only when the file does not exist; every other failure ignores the whole
/// document (a diagnostic without content), so a problem never relaxes a rule.
fn read_settings(path: &std::path::Path, level: Level, kind: SourceKind) -> Parsed {
    let ignored = |code| Parsed::ignored(Diagnostic::new(code, kind));
    let meta = match std::fs::symlink_metadata(path) {
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
    let read = open_no_follow(path).and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes));
    if read.is_err() {
        return ignored(Code::Unreadable);
    }
    if bytes.len() as u64 > MAX_BYTES {
        return ignored(Code::LimitExceeded(Limit::Size));
    }
    parse_document(&bytes, level, kind)
}

/// File name of the local settings of a repo, in `<config>/repos/<repo_id>/`.
pub const LOCAL_SETTINGS_FILE: &str = "settings.local.json";

/// `<config>/repos/<repo_id>/settings.local.json`; `None` when `repo_id` is not 1..=64 of
/// `[0-9a-fA-F-]`.
pub fn local_settings_path(dirs: &ProfileDirs, repo_id: &str) -> Option<std::path::PathBuf> {
    let valid = (1..=64).contains(&repo_id.len())
        && repo_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| {
        dirs.config
            .join("repos")
            .join(repo_id)
            .join(LOCAL_SETTINGS_FILE)
    })
}

/// The local document of a repo, read like the profile one (no link followed, regular file,
/// 64 KiB, `Level::Local`: keys the local level does not admit are dropped by the schema, so
/// `baseBranch` is ignored). Absent when the id is invalid or there is no file; ignored when
/// the repo folder is a link. Never creates the file or the folder.
pub fn local_settings(dirs: &ProfileDirs, repo_id: &str) -> Parsed {
    let Some(path) = local_settings_path(dirs, repo_id) else {
        return Parsed::absent();
    };
    // The repo folder must not be a link either: it could point the read outside the profile.
    if let Some(folder) = path.parent()
        && std::fs::symlink_metadata(folder).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Parsed::ignored(Diagnostic::new(Code::Symlink, SourceKind::Local));
    }
    read_settings(&path, Level::Local, SourceKind::Local)
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

/// `engine.observation` of the profile (TS-GRP-006, N7); the local level
/// of `dormantAfterHours` arrives with US-GRP-013.
pub fn observation(dirs: &ProfileDirs) -> gitraptor_policy::settings::Observation {
    profile_settings(dirs)
        .applicable()
        .and_then(|s| s.engine.as_ref())
        .and_then(|e| e.observation.clone())
        .unwrap_or_default()
}

/// `engine.watcher.backend` of the profile: `fsevents` unless the profile asks for `notify`
/// (ADR-GRP-010, Enmienda 2026-10-08). Read when the daemon starts; an unusable document is
/// the default.
pub fn watch_backend(dirs: &ProfileDirs) -> gitraptor_policy::settings::WatchBackend {
    profile_settings(dirs)
        .applicable()
        .and_then(|s| s.engine.as_ref())
        .and_then(|e| e.watcher.as_ref())
        .and_then(|w| w.backend)
        .unwrap_or_default()
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

    #[test]
    fn the_watcher_backend_defaults_to_fsevents_and_follows_the_profile() {
        use gitraptor_policy::settings::WatchBackend;
        let (_tmp, dirs) = dirs();
        assert_eq!(watch_backend(&dirs), WatchBackend::Fsevents);
        write(&dirs, r#"{"engine": {"watcher": {"backend": "notify"}}}"#);
        assert_eq!(watch_backend(&dirs), WatchBackend::Notify);
        write(&dirs, r#"{"engine": {"watcher": {"backend": "fsevents"}}}"#);
        assert_eq!(watch_backend(&dirs), WatchBackend::Fsevents);
        // A value that is not one of the two: the document is not applied, the default stays.
        write(&dirs, r#"{"engine": {"watcher": {"backend": "kqueue"}}}"#);
        assert_eq!(watch_backend(&dirs), WatchBackend::Fsevents);
    }

    fn local_write(dirs: &ProfileDirs, id: &str, text: &str) {
        let path = local_settings_path(dirs, id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const ID: &str = "0123abcd-ef01";

    #[test]
    fn local_is_absent_without_file_or_with_a_bad_id_and_creates_nothing() {
        let (_tmp, dirs) = dirs();
        assert!(local_settings(&dirs, ID).applicable().is_none());
        assert!(local_settings(&dirs, "../etc").applicable().is_none());
        assert!(local_settings(&dirs, "").applicable().is_none());
        assert!(!dirs.config.join("repos").exists());
    }

    #[test]
    fn local_reads_permissions_and_drops_keys_the_level_does_not_admit() {
        let (_tmp, dirs) = dirs();
        local_write(
            &dirs,
            ID,
            r#"{"permissions":{"deny":["push"]},"engine":{"baseBranch":"develop"}}"#,
        );
        let parsed = local_settings(&dirs, ID);
        let s = parsed.applicable().expect("applicable");
        assert!(s.permissions.is_some());
        assert!(
            s.engine.as_ref().is_none_or(|e| e.base_branch.is_none()),
            "baseBranch must be ignored at the local level"
        );
        assert!(!parsed.diagnostics.is_empty());
    }

    #[test]
    fn local_invalid_or_oversized_is_ignored() {
        let (_tmp, dirs) = dirs();
        local_write(&dirs, ID, "{not json");
        assert!(local_settings(&dirs, ID).applicable().is_none());
        let pad = " ".repeat(usize::try_from(MAX_BYTES).unwrap());
        local_write(
            &dirs,
            ID,
            &format!(r#"{{"permissions":{{"deny":["push"]}}}}{pad}"#),
        );
        let parsed = local_settings(&dirs, ID);
        assert!(parsed.applicable().is_none());
        assert!(!parsed.diagnostics.is_empty());
    }

    #[test]
    fn local_that_is_a_directory_is_ignored() {
        let (_tmp, dirs) = dirs();
        let path = local_settings_path(&dirs, ID).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        let parsed = local_settings(&dirs, ID);
        assert!(parsed.applicable().is_none());
        assert!(!parsed.diagnostics.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn local_links_are_not_followed() {
        let (tmp, dirs) = dirs();
        let target = tmp.path().join("elsewhere.json");
        std::fs::write(&target, r#"{"permissions":{"deny":["push"]}}"#).unwrap();
        let path = local_settings_path(&dirs, ID).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(local_settings(&dirs, ID).applicable().is_none());

        // The repo folder as a link.
        let other = "feed";
        let real = tmp.path().join("real-folder");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(
            real.join(LOCAL_SETTINGS_FILE),
            r#"{"permissions":{"deny":["push"]}}"#,
        )
        .unwrap();
        let folder = local_settings_path(&dirs, other).unwrap();
        std::fs::create_dir_all(folder.parent().unwrap().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, folder.parent().unwrap()).unwrap();
        assert!(local_settings(&dirs, other).applicable().is_none());
    }
}
