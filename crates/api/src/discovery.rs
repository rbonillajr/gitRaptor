//! Discovery contract (US-GRP-020, US-GRP-022; ADR-GRP-010, Enmienda
//! 2026-10-07, N6 and N8; SEC-15): the code roots the developer declares and
//! the repos found in their first level, which nobody observes until the
//! developer accepts them with `repo.add`.
//!
//! Reasons travel as English kebab-case codes and each client translates
//! them (NFR-10). Paths are the daemon's canonical paths.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `discovery.root.add` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RootAddParams {
    pub path: String,
    /// The developer confirmed a broad root in their terminal. The daemon
    /// checks the path and the requester again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm_broad: bool,
}

/// `discovery.root.add` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RootAddResult {
    pub root: RootView,
    /// The root was already declared: nothing changed.
    pub already: bool,
    /// Repos discovered by the first listing.
    pub candidates: u32,
}

/// `discovery.root.remove` and `discovery.dismiss` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathParams {
    pub path: String,
}

/// `discovery.root.remove` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RootRemoveResult {
    pub root: String,
    /// Pending candidates removed with the root.
    pub candidates_removed: u32,
}

/// `discovery.dismiss` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DismissResult {
    pub path: String,
}

/// A declared root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RootView {
    pub path: String,
    /// A broad root is listed every 60 s and never watched (RES-03).
    pub broad: bool,
    pub added_utc_ms: i64,
}

/// `discovery.roots` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RootsResult {
    pub roots: Vec<RootView>,
}

/// A discovered repo waiting for the developer's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CandidateView {
    /// The repo's folder, in the first level of `root`.
    pub path: String,
    /// The folder's name.
    pub name: String,
    pub root: String,
    pub found_utc_ms: i64,
}

/// `discovery.candidates` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CandidatesResult {
    pub candidates: Vec<CandidateView>,
}

/// Data of the `repo.discovered` event: one per declared root, with how many
/// repos its first listing found, and one per later listing that finds new
/// repos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RepoDiscoveredData {
    pub root: String,
    /// The new candidates.
    pub candidates: Vec<CandidateView>,
    pub count: u32,
}

/// Why a path cannot be a root (SEC-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RootRejection {
    /// Not an absolute path.
    NotAbsolute,
    /// Nothing at the path.
    Missing,
    /// Not a folder.
    NotADirectory,
    /// A symbolic link: declare the real path.
    Symlink,
    /// `/` or the system drive.
    FilesystemRoot,
    /// A folder that holds other users' folders (`/Users`, `/home`).
    HomeAncestor,
    /// A repo, or a folder inside one.
    InsideRepo,
    /// Inside GitRaptor's profile.
    Profile,
    /// A network path or file system.
    Network,
    /// The limit of roots is reached.
    TooMany,
    /// The folder cannot be read.
    Unreadable,
}

/// `data` of `root-rejected`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RootRejectedData {
    pub reason: RootRejection,
    /// With `symlink`: the real path, to declare instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_path: Option<String>,
}

/// Why a root is broad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BroadReason {
    /// The developer's home folder.
    Home,
    /// The root of another volume.
    Volume,
    /// More first-level entries than a normal root.
    Entries,
}

/// `data` of `root-broad`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RootBroadData {
    pub reason: BroadReason,
    /// The canonical path the developer is asked to confirm.
    pub path: String,
    /// First-level entries counted, up to the limit.
    pub entries: u32,
}
