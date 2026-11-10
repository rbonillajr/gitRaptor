//! The desktop app's connection class and its read profile (TS-GRP-009,
//! ADR-GRP-017 § 3, DS-TS-GRP-009).
//!
//! The daemon deduces the class from the kernel's executable of the peer,
//! never from what the client declares, and fixes its profile once, at
//! `hello`. In doubt, the most restrictive profile.

use std::path::{Path, PathBuf};

use gitraptor_api::desktop::{DesktopIdentity, DesktopReason};
use gitraptor_api::messages::{ClientKind, ConnectionProfile};

use super::authz::{AcceptedPeer, Checks};
use super::marks::ExecutorMarks;

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

/// The real file system.
pub struct SystemFs;

impl AppFs for SystemFs {
    fn canonicalize(&self, _path: &Path) -> std::io::Result<PathBuf> {
        // Stub (TS-GRP-009 T002): filled by the build.
        Err(std::io::Error::other("not implemented"))
    }

    fn metadata(&self, _path: &Path) -> std::io::Result<FsMeta> {
        // Stub (TS-GRP-009 T002): filled by the build.
        Err(std::io::Error::other("not implemented"))
    }
}

/// Whether the kernel's executable of the peer is the app's (class `desktop`).
pub fn is_app_exe(_exe: Option<&Path>, _binaries: &[&str]) -> bool {
    // Stub (TS-GRP-009 T002): fails closed.
    false
}

/// `/Applications` and `<pw_dir>/Applications` for `uid` on macOS; empty elsewhere.
pub fn system_app_dirs(_uid: u32) -> Vec<PathBuf> {
    // Stub (TS-GRP-009 T002): fails closed.
    Vec::new()
}

/// Path identity of the alphas (ADR-GRP-017 § 3).
pub fn path_identity(
    _exe: Option<&Path>,
    _apps: &[PathBuf],
    _fs: &dyn AppFs,
    _uid: u32,
) -> PathIdentity {
    // Stub (TS-GRP-009 T002): fails closed.
    PathIdentity::Unverifiable
}

/// Profile of a desktop-class connection (D3), first rule that applies.
pub fn decide(
    _peer: AcceptedPeer,
    _checks: &Checks<'_>,
    _marks: Option<&ExecutorMarks>,
    _declared: ClientKind,
    _exe: Option<&Path>,
    _identity: PathIdentity,
) -> DesktopDecision {
    // Stub (TS-GRP-009 T002): fails closed.
    DesktopDecision {
        profile: ConnectionProfile::Mcp,
        reason: DesktopReason::RequesterUnverified,
        identity: DesktopIdentity::Unverifiable,
    }
}
