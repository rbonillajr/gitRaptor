//! Discovery in code roots (US-GRP-020; ADR-GRP-010, Enmienda 2026-10-07,
//! N6; SEC-15): which folder can be a root, and which entries of its first
//! level are Git repos.
//!
//! Discovering is not observing: nothing here opens a repo with the read
//! layer, reads its configuration, follows a link or runs a process. From each
//! entry it reads only `<entry>/.git` (and `HEAD` inside it), or the `gitdir:`
//! file of a linked worktree, at most 4 KiB.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gitraptor_api::discovery::{BroadReason, RootRejectedData, RootRejection};

use crate::profile::normalize_common_dir;

#[cfg(test)]
mod tests;

/// At most this many roots (⚠️ ASSUMPTION of N6).
pub const MAX_ROOTS: usize = 16;
/// More first-level entries than this make a root broad (⚠️ ASSUMPTION of
/// N6: an eighth of [`MAX_ENTRIES`]).
pub const BROAD_ENTRIES: usize = 512;
/// The first level of a root is truncated here, with a diagnostic.
pub const MAX_ENTRIES: usize = 4096;
/// A `.git` file longer than this is not a linked worktree.
const GITDIR_FILE_MAX: u64 = 4096;

/// Folders of the home folder that are never looked at when the root is the
/// home folder itself: other applications' data and the folders macOS
/// protects with TCC (reading their `.git` would make macOS ask the user for
/// access on behalf of the daemon).
#[cfg(target_os = "macos")]
const HOME_EXCLUDED: &[&str] = &[
    "Library",
    "Desktop",
    "Documents",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
];
#[cfg(windows)]
const HOME_EXCLUDED: &[&str] = &["AppData", "OneDrive"];
#[cfg(not(any(target_os = "macos", windows)))]
const HOME_EXCLUDED: &[&str] = &["snap"];

/// What a root is checked against.
#[derive(Debug, Clone, Default)]
pub struct RootContext {
    /// The developer's home folder.
    pub home: Option<PathBuf>,
    /// GitRaptor's own folders: a root cannot be inside them.
    pub profile: Vec<PathBuf>,
}

/// A path that can be a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidRoot {
    /// Canonical.
    pub path: PathBuf,
    /// Why it is broad, and the first-level entries counted (up to
    /// [`MAX_ENTRIES`]).
    pub broad: Option<BroadReason>,
    pub entries: u32,
}

fn rejected(reason: RootRejection) -> RootRejectedData {
    RootRejectedData {
        reason,
        real_path: None,
    }
}

/// Checks a path the developer wants to declare as a root (SEC-15). It reads
/// only the path's own metadata and that of its ancestors' `.git`, and counts
/// its first-level entries.
pub fn validate_root(raw: &Path, ctx: &RootContext) -> Result<ValidRoot, RootRejectedData> {
    if !raw.is_absolute() {
        return Err(rejected(RootRejection::NotAbsolute));
    }
    // Before any file system call: touching a UNC path opens an SMB
    // connection.
    if is_unc(raw) {
        return Err(rejected(RootRejection::Network));
    }
    let meta = match fs::symlink_metadata(raw) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(rejected(RootRejection::Missing));
        }
        Err(_) => return Err(rejected(RootRejection::Unreadable)),
    };
    if is_link(&meta) {
        return Err(RootRejectedData {
            reason: RootRejection::Symlink,
            real_path: gitraptor_git::paths::canonicalize(raw)
                .ok()
                .and_then(|p| p.to_str().map(str::to_owned)),
        });
    }
    if !meta.is_dir() {
        return Err(rejected(RootRejection::NotADirectory));
    }
    let path =
        gitraptor_git::paths::canonicalize(raw).map_err(|_| rejected(RootRejection::Unreadable))?;
    let volume = match volume_kind(&path) {
        VolumeKind::SystemRoot => return Err(rejected(RootRejection::FilesystemRoot)),
        VolumeKind::OtherVolume => true,
        VolumeKind::Folder => false,
    };
    let home = ctx
        .home
        .as_deref()
        .and_then(|h| gitraptor_git::paths::canonicalize(h).ok());
    if let Some(home) = &home
        && home != &path
        && home.starts_with(&path)
    {
        return Err(rejected(RootRejection::HomeAncestor));
    }
    for owned in &ctx.profile {
        let owned = gitraptor_git::paths::canonicalize(owned).unwrap_or_else(|_| owned.clone());
        if path.starts_with(&owned) {
            return Err(rejected(RootRejection::Profile));
        }
    }
    if is_network(&path) {
        return Err(rejected(RootRejection::Network));
    }
    if path
        .ancestors()
        .any(|a| fs::symlink_metadata(a.join(".git")).is_ok())
    {
        return Err(rejected(RootRejection::InsideRepo));
    }
    let entries = count_entries(&path).map_err(|_| rejected(RootRejection::Unreadable))?;
    let broad = if home.as_ref() == Some(&path) {
        Some(BroadReason::Home)
    } else if volume {
        Some(BroadReason::Volume)
    } else if entries > BROAD_ENTRIES {
        Some(BroadReason::Entries)
    } else {
        None
    };
    Ok(ValidRoot {
        path,
        broad,
        entries: u32::try_from(entries).unwrap_or(u32::MAX),
    })
}

fn count_entries(path: &Path) -> io::Result<usize> {
    Ok(fs::read_dir(path)?.take(MAX_ENTRIES).count())
}

/// A Git repo found in the first level of a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The entry's folder.
    pub path: PathBuf,
    pub name: String,
    /// The repo key (ADR-GRP-006): its common Git directory.
    pub key_path: String,
}

/// One listing of a root's first level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub found: Vec<Found>,
    /// More than [`MAX_ENTRIES`] entries: the rest was not looked at.
    pub truncated: bool,
    /// First-level entries seen, up to [`MAX_ENTRIES`].
    pub entries: usize,
}

/// Lists the first level of `root` and keeps the entries that are Git repos,
/// one per repo key (a linked worktree of a repo already found is not a
/// second repo). `home_root` adds the home folder's exclusions.
pub fn list_first_level(root: &Path, home_root: bool) -> io::Result<Listing> {
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut seen = 0;
    for entry in fs::read_dir(root)? {
        if seen >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        seen += 1;
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if excluded_name(&name, home_root) {
            continue;
        }
        // `DirEntry::file_type` does not follow links: a link is not a folder.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if entry.metadata().is_ok_and(|m| excluded_meta(&m)) {
            continue;
        }
        entries.push((name, entry.path()));
    }
    entries.sort();
    let mut found: Vec<(bool, Found)> = Vec::new();
    for (name, path) in entries {
        let Some((main, common)) = probe_git(&path) else {
            continue;
        };
        let Ok(key) = normalize_common_dir(&common) else {
            continue;
        };
        let item = Found {
            path,
            name,
            key_path: key.key_path,
        };
        match found.iter_mut().find(|(_, f)| f.key_path == item.key_path) {
            // The main worktree wins over a linked one of the same repo.
            Some(slot) if main && !slot.0 => *slot = (main, item),
            Some(_) => {}
            None => found.push((main, item)),
        }
    }
    Ok(Listing {
        found: found.into_iter().map(|(_, f)| f).collect(),
        truncated,
        entries: seen,
    })
}

fn excluded_name(name: &str, home_root: bool) -> bool {
    name.starts_with('.') || (home_root && HOME_EXCLUDED.contains(&name))
}

/// Whether `<entry>/.git` makes `entry` a repo: a folder with `HEAD`, or a
/// `gitdir:` file of at most 4 KiB. Returns whether it is a main worktree
/// and the repo's common Git directory. Nothing else is read.
fn probe_git(entry: &Path) -> Option<(bool, PathBuf)> {
    let dot_git = entry.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        // A clone in progress has no `HEAD` yet: the next listing sees it.
        let head = fs::symlink_metadata(dot_git.join("HEAD")).ok()?;
        return head.is_file().then_some((true, dot_git));
    }
    if !meta.is_file() || meta.len() > GITDIR_FILE_MAX {
        return None;
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if target.is_empty() {
        return None;
    }
    let gitdir = entry.join(target);
    // `<common>/worktrees/<name>`: a linked worktree. Anything else (a
    // submodule's `modules/<name>`) is its own repo.
    let common = match gitdir.parent() {
        Some(parent) if parent.file_name().is_some_and(|n| n == "worktrees") => {
            parent.parent()?.to_path_buf()
        }
        _ => gitdir,
    };
    Some((false, common))
}

fn is_unc(path: &Path) -> bool {
    cfg!(windows)
        && path.to_str().is_some_and(|p| {
            (p.starts_with(r"\\") || p.starts_with("//"))
                && !p.starts_with(r"\\?\")
                && !p.starts_with(r"\\.\")
        })
        || path.to_str().is_some_and(|p| p.starts_with(r"\\?\UNC\"))
}

#[cfg(unix)]
fn is_link(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const REPARSE_POINT: u32 = 0x400;
    meta.file_type().is_symlink() || meta.file_attributes() & REPARSE_POINT != 0
}

/// Entries never looked at whatever the root: on Windows, hidden or system
/// folders, reparse points and cloud placeholders that download when read
/// (OneDrive); on macOS, iCloud entries without a local copy (dataless).
#[cfg(windows)]
fn excluded_meta(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    const REPARSE_POINT: u32 = 0x400;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    meta.file_attributes()
        & (HIDDEN | SYSTEM | REPARSE_POINT | OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS)
        != 0
}

#[cfg(target_os = "macos")]
fn excluded_meta(meta: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const SF_DATALESS: u32 = 0x4000_0000;
    meta.st_flags() & SF_DATALESS != 0
}

#[cfg(not(any(target_os = "macos", windows)))]
fn excluded_meta(_: &fs::Metadata) -> bool {
    false
}

enum VolumeKind {
    /// `/` or the system drive.
    SystemRoot,
    /// The root of another volume.
    OtherVolume,
    Folder,
}

#[cfg(windows)]
fn volume_kind(path: &Path) -> VolumeKind {
    if path.parent().is_some() {
        return VolumeKind::Folder;
    }
    let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let drive = path.to_str().unwrap_or_default();
    if drive
        .get(..2)
        .is_some_and(|d| d.eq_ignore_ascii_case(&system))
    {
        VolumeKind::SystemRoot
    } else {
        VolumeKind::OtherVolume
    }
}

#[cfg(unix)]
fn volume_kind(path: &Path) -> VolumeKind {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = path.parent() else {
        return VolumeKind::SystemRoot;
    };
    if cfg!(target_os = "macos") && parent == Path::new("/Volumes") {
        return VolumeKind::OtherVolume;
    }
    // A mount point: its device differs from its parent's.
    match (fs::metadata(path), fs::metadata(parent)) {
        (Ok(a), Ok(b)) if a.dev() != b.dev() => VolumeKind::OtherVolume,
        _ => VolumeKind::Folder,
    }
}

/// A network file system (macOS: not `MNT_LOCAL`; Linux: NFS, SMB or CIFS).
#[cfg(target_os = "macos")]
fn is_network(path: &Path) -> bool {
    const MNT_LOCAL: u32 = 0x1000;
    rustix::fs::statfs(path).is_ok_and(|s| s.f_flags & MNT_LOCAL == 0)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_network(path: &Path) -> bool {
    const NFS: i64 = 0x6969;
    const SMB: i64 = 0x517B;
    const SMB2: i64 = 0xFE53_4D42;
    const CIFS: i64 = 0xFF53_4D42;
    rustix::fs::statfs(path).is_ok_and(|s| {
        #[allow(clippy::useless_conversion, clippy::unnecessary_cast)]
        let kind = s.f_type as i64;
        [NFS, SMB, SMB2, CIFS].contains(&kind)
    })
}

/// On Windows only UNC paths are recognized; a mapped network drive is
/// pending the cross-OS validation stage.
#[cfg(windows)]
fn is_network(_: &Path) -> bool {
    false
}
