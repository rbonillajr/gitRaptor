//! The desktop app's connection class and its read profile (TS-GRP-009,
//! ADR-GRP-017 § 3, DS-TS-GRP-009).
//!
//! The daemon deduces the class from the kernel's executable of the peer,
//! never from what the client declares, and fixes its profile once, at
//! `hello`. In doubt, the most restrictive profile.

use std::path::{Component, Path, PathBuf};

use gitraptor_api::desktop::{DesktopIdentity, DesktopReason};
use gitraptor_api::messages::{ClientKind, ConnectionProfile};

use gitraptor_api::timemachine::ResolvedVia;

use super::authz::{AcceptedPeer, Checks, check_reserved};
use super::marks::ExecutorMarks;
use super::requester::resolve_traced;

/// File names of the app's main binary inside `GitRaptor.app/Contents/MacOS` (D4).
pub const APP_BINARIES: &[&str] = &["GitRaptor", "gitraptor-desktop"];

/// What the daemon could prove about the peer's executable by its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathIdentity {
    Verified,
    Absent,
    Unverifiable,
}

/// What [`AppFs::metadata`] reports, without following the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsMeta {
    pub is_file: bool,
    pub is_dir: bool,
    pub mode: u32,
    pub uid: u32,
    pub dev: u64,
}

/// The profile of a desktop-class connection, fixed at `hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopDecision {
    pub profile: ConnectionProfile,
    pub reason: DesktopReason,
    pub identity: DesktopIdentity,
}

/// Read-only file system port; [`SystemFs`] in production, doubles in tests.
pub trait AppFs {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf>;
    fn metadata(&self, path: &Path) -> std::io::Result<FsMeta>;
}

/// The bundle's name inside an applications folder.
const BUNDLE: &str = "GitRaptor.app";

/// Write bits for group and others: never on the executable or its bundle.
const GROUP_OR_OTHER_WRITE: u32 = 0o022;
/// Write bit for others: never on the applications folder (`/Applications`
/// is `root:admin 0775` on macOS, so group write is admitted there).
const OTHER_WRITE: u32 = 0o002;

/// The real file system.
pub struct SystemFs;

impl AppFs for SystemFs {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    #[cfg(unix)]
    fn metadata(&self, path: &Path) -> std::io::Result<FsMeta> {
        use std::os::unix::fs::MetadataExt;

        let meta = std::fs::metadata(path)?;
        Ok(FsMeta {
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            mode: meta.mode(),
            uid: meta.uid(),
            dev: meta.dev(),
        })
    }

    /// No owner nor mode bits to prove anything with: refused, so the
    /// identity can never be verified here.
    #[cfg(not(unix))]
    fn metadata(&self, _path: &Path) -> std::io::Result<FsMeta> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

/// Whether the kernel's executable of the peer is the app's (class `desktop`).
///
/// Compares the file name exactly (case-sensitive); on Windows a trailing
/// `.exe` is ignored.
pub fn is_app_exe(exe: Option<&Path>, binaries: &[&str]) -> bool {
    let Some(name) = exe.and_then(Path::file_name).and_then(|n| n.to_str()) else {
        return false;
    };
    #[cfg(windows)]
    let name = name.strip_suffix(".exe").unwrap_or(name);
    binaries.contains(&name)
}

/// `/Applications` and `<pw_dir>/Applications` for `uid` on macOS; empty elsewhere.
///
/// The home folder comes from the user database, never from the environment:
/// whoever launched the daemon chose its `HOME`. Without an entry for `uid`,
/// only `/Applications`.
#[cfg(target_os = "macos")]
pub fn system_app_dirs(uid: u32) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Ok(Some(user)) = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid)) {
        dirs.push(user.dir.join("Applications"));
    }
    dirs
}

/// `/Applications` and `<pw_dir>/Applications` for `uid` on macOS; empty elsewhere.
#[cfg(not(target_os = "macos"))]
pub fn system_app_dirs(_uid: u32) -> Vec<PathBuf> {
    Vec::new()
}

/// Path identity of the alphas (ADR-GRP-017 § 3). Order: canonicalize →
/// translocation and `/Volumes` → exact location and name → bundle modes,
/// owner and device → apps folder.
///
/// `Unverifiable` only when the executable or a folder of its bundle cannot
/// be read; an applications folder that is missing or unreadable is skipped.
pub fn path_identity(
    exe: Option<&Path>,
    apps: &[PathBuf],
    fs: &dyn AppFs,
    uid: u32,
) -> PathIdentity {
    let Some(exe) = exe else {
        return PathIdentity::Unverifiable;
    };
    // No applications folder (every OS but macOS): no path identity to
    // prove, so `none` (row 4), not `unverifiable` (D4).
    if apps.is_empty() {
        return PathIdentity::Absent;
    }
    let Ok(exe) = fs.canonicalize(exe) else {
        return PathIdentity::Unverifiable;
    };
    // The executable and the three folders of its would-be bundle
    // (`MacOS`, `Contents`, `GitRaptor.app`), read before any judgement.
    let chain: Vec<&Path> = exe.ancestors().take(4).collect();
    let mut metas = Vec::with_capacity(chain.len());
    for path in &chain {
        match fs.metadata(path) {
            Ok(meta) => metas.push(meta),
            Err(_) => return PathIdentity::Unverifiable,
        }
    }

    if exe
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
        || exe.starts_with("/Volumes")
    {
        return PathIdentity::Absent;
    }
    if chain.len() < 4 || !in_bundle(&exe) {
        return PathIdentity::Absent;
    }
    let Some(bundle_parent) = chain[3].parent() else {
        return PathIdentity::Absent;
    };

    for folder in apps {
        let Ok(folder) = fs.canonicalize(folder) else {
            continue;
        };
        if folder.as_path() != bundle_parent {
            continue;
        }
        let Ok(folder_meta) = fs.metadata(&folder) else {
            continue;
        };
        return judge(&metas, folder_meta, uid);
    }
    PathIdentity::Absent
}

/// Whether `exe` ends in `GitRaptor.app/Contents/MacOS/<app binary>`,
/// compared by components and case-sensitively.
fn in_bundle(exe: &Path) -> bool {
    let tail: Vec<Component<'_>> = exe.components().rev().take(4).collect();
    let [
        Component::Normal(bin),
        Component::Normal(macos),
        Component::Normal(contents),
        Component::Normal(bundle),
    ] = tail.as_slice()
    else {
        return false;
    };
    *macos == "MacOS"
        && *contents == "Contents"
        && *bundle == BUNDLE
        && bin.to_str().is_some_and(|b| APP_BINARIES.contains(&b))
}

/// Modes, owner and device of the executable and its bundle (`metas`, from
/// the executable up), then the applications folder.
fn judge(metas: &[FsMeta], folder: FsMeta, uid: u32) -> PathIdentity {
    let owned = |m: &FsMeta| m.uid == uid || m.uid == 0;
    for (i, meta) in metas.iter().enumerate() {
        let kind_ok = if i == 0 { meta.is_file } else { meta.is_dir };
        if !kind_ok
            || meta.mode & GROUP_OR_OTHER_WRITE != 0
            || !owned(meta)
            || meta.dev != folder.dev
        {
            return PathIdentity::Absent;
        }
    }
    if !folder.is_dir || folder.mode & OTHER_WRITE != 0 || !owned(&folder) {
        return PathIdentity::Absent;
    }
    PathIdentity::Verified
}

/// Profile of a desktop-class connection (D3), first rule that applies.
///
/// `identity` is reported as received even when an earlier rule decides.
pub fn decide(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
    declared: ClientKind,
    exe: Option<&Path>,
    identity: PathIdentity,
) -> DesktopDecision {
    let reported = match identity {
        PathIdentity::Verified => DesktopIdentity::Path,
        PathIdentity::Absent => DesktopIdentity::None,
        PathIdentity::Unverifiable => DesktopIdentity::Unverifiable,
    };
    let with = |profile, reason| DesktopDecision {
        profile,
        reason,
        identity: reported,
    };
    let mcp = |reason| with(ConnectionProfile::Mcp, reason);

    if declared == ClientKind::Mcp || is_mcp_exe(exe) {
        return mcp(DesktopReason::DeclaredMcp);
    }
    let Ok((resolution, trace)) = resolve_traced(peer, checks, marks) else {
        return mcp(DesktopReason::RequesterUnverified);
    };
    if resolution.via == ResolvedVia::Executor || trace.daemon_descendant {
        return mcp(DesktopReason::DaemonDescendant);
    }
    if resolution.who.is_agent() {
        return mcp(DesktopReason::AgentInAncestry);
    }
    if trace.interpreter || trace.multiplexer || !trace.ended_cleanly {
        return mcp(DesktopReason::AmbiguousAncestry);
    }
    match identity {
        PathIdentity::Unverifiable => mcp(DesktopReason::IdentityUnverifiable),
        PathIdentity::Verified => with(ConnectionProfile::Full, DesktopReason::PathIdentity),
        // Row 4: the rule of a client without a terminal, controls 1 to 3
        // (not `Resolution::confirmable`, which also weighs agents around).
        PathIdentity::Absent => {
            let profile = if check_reserved(peer, checks).refused.is_none() {
                ConnectionProfile::Full
            } else {
                ConnectionProfile::Mcp
            };
            with(profile, DesktopReason::NoIdentity)
        }
    }
}

/// The MCP server's executable, the same rule the handshake applies to any
/// connection.
fn is_mcp_exe(exe: Option<&Path>) -> bool {
    exe.and_then(Path::file_name)
        .is_some_and(|n| n == "raptor-mcp" || n == "raptor-mcp.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file system that cannot read anything, as on Windows.
    struct Unreadable;

    impl AppFs for Unreadable {
        fn canonicalize(&self, _path: &Path) -> std::io::Result<PathBuf> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }

        fn metadata(&self, _path: &Path) -> std::io::Result<FsMeta> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }
    }

    /// Without applications folders (every OS but macOS) the identity is
    /// `none`, not `unverifiable`, so row 4 decides (D4).
    #[test]
    fn no_applications_folders_means_no_identity() {
        let exe = Path::new("/opt/GitRaptor.app/Contents/MacOS/GitRaptor");
        assert_eq!(
            path_identity(Some(exe), &[], &Unreadable, 501),
            PathIdentity::Absent
        );
        // Without an executable it stays unverifiable.
        assert_eq!(
            path_identity(None, &[], &Unreadable, 501),
            PathIdentity::Unverifiable
        );
    }
}
