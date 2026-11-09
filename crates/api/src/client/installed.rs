//! What counts as an installed `raptor` (SEC-14, ADR-GRP-014 Enmienda DEP-DSK-5 § D6): the rule
//! that the autostart applies to its own binary and the desktop app applies to the binary it runs
//! and to the process serving the channel (B-3), written once.
//!
//! Pure checks on a path and its metadata: nothing here reads the environment, so a caller that
//! must not read it (the desktop app) passes [`FIXED_TEMP`].

use std::path::{Component, Path, PathBuf};

/// Path components of the npm channel and of the npx cache: a binary there is not an
/// installation (SEC-14, ADR-GRP-014, INF-GRP-004).
const TRANSIENT_COMPONENTS: [&str; 2] = ["node_modules", "_npx"];

/// The temporary folders of a Unix machine, fixed: for whoever cannot read `TMPDIR`.
pub const FIXED_TEMP: &[&str] = &[
    "/tmp",
    "/private/tmp",
    "/var/tmp",
    "/private/var/tmp",
    "/var/folders",
    "/private/var/folders",
];

/// `true` if `path` is not an installation (SEC-14): under the npm channel, the npx, `pnpm dlx`
/// or `bunx` cache, or under one of the `temp` folders. A `temp` entry without a parent (a root)
/// is ignored, so a bad entry never makes every path transient.
pub fn is_transient(path: &Path, temp: &[PathBuf]) -> bool {
    let names: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let cache = names.iter().enumerate().any(|(i, name)| {
        TRANSIENT_COMPONENTS.contains(&name.as_str())
            || name.starts_with("bunx-")
            || (name == "dlx" && i > 0 && names[i - 1].contains("pnpm"))
    });
    let in_temp = temp
        .iter()
        .filter(|t| t.parent().is_some())
        .any(|t| path.starts_with(t));
    cache || in_temp
}

/// `true` if `path` is an installed `raptor` of this user (SEC-14, B-3): not a symbolic link
/// itself; its canonical path is a regular file named `raptor`, owned by this user or by root,
/// writable by neither the group nor others, and not transient under [`FIXED_TEMP`]. Anything
/// that cannot be read is `false` (fail-closed). Not Unix: always `false`.
pub fn is_installed_raptor(path: &Path) -> bool {
    #[cfg(unix)]
    {
        installed_unix(path).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

#[cfg(unix)]
fn installed_unix(path: &Path) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;

    // Canonicalizing follows a link, so the link is refused on the path as given first.
    if std::fs::symlink_metadata(path)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return Some(false);
    }
    let canonical = path.canonicalize().ok()?;
    if canonical.file_name()? != std::ffi::OsStr::new("raptor") {
        return Some(false);
    }
    let meta = std::fs::symlink_metadata(&canonical).ok()?;
    if !meta.file_type().is_file() {
        return Some(false);
    }
    let owner = meta.uid();
    if owner != super::peer::current_uid() && owner != 0 {
        return Some(false);
    }
    if meta.mode() & 0o022 != 0 {
        return Some(false);
    }
    let temp: Vec<PathBuf> = FIXED_TEMP.iter().map(PathBuf::from).collect();
    Some(!is_transient(&canonical, &temp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_and_temporary_folders_are_transient() {
        let temp = vec![PathBuf::from("/tmp")];
        assert!(is_transient(
            Path::new("/a/node_modules/.bin/raptor"),
            &temp
        ));
        assert!(is_transient(Path::new("/a/_npx/1/raptor"), &temp));
        assert!(is_transient(Path::new("/a/bunx-501-raptor/raptor"), &temp));
        assert!(is_transient(Path::new("/a/pnpm/dlx/x/raptor"), &temp));
        assert!(is_transient(Path::new("/tmp/x/raptor"), &temp));
        assert!(!is_transient(Path::new("/opt/bin/raptor"), &temp));
        // A root as a temporary folder would make every path transient: ignored.
        assert!(!is_transient(
            Path::new("/opt/bin/raptor"),
            &[PathBuf::from("/")]
        ));
    }
}
