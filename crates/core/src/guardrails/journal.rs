//! The install journal (ADR-GRD-001 § 1 and § 4): written only by the daemon in the repo store,
//! before and after each step, and the authoritative integrity reference (H-04). The manifest
//! in the repo is not.

use serde::{Deserialize, Serialize};

/// Where an install stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// Started: the folder and the key may or may not be written. Recovered at startup.
    Installing,
    /// Verified and confirmed.
    Confirmed,
}

/// `(dev, inode)` as recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub dev: u64,
    pub ino: u64,
}

impl From<gitraptor_git::guard_write::FileId> for Identity {
    fn from(id: gitraptor_git::guard_write::FileId) -> Self {
        Self {
            dev: id.dev,
            ino: id.ino,
        }
    }
}

impl From<Identity> for gitraptor_git::guard_write::FileId {
    fn from(id: Identity) -> Self {
        Self {
            dev: id.dev,
            ino: id.ino,
        }
    }
}

/// One file of `gitraptor/` and its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHash {
    pub path: String,
    pub sha256: String,
}

/// The previous `core.hooksPath` and its level (ADR-GRD-001 § 1). US-GRD-001 only installs
/// without one, so it is always `none` here; US-GRD-002 fills it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prior {
    pub value: Option<String>,
    pub level: String,
}

/// The journal entry of one repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub version: u32,
    pub stage: Stage,
    pub at_ms: i64,
    /// Canonical common directory.
    pub common_dir: String,
    /// The value of `core.hooksPath` that is ours.
    pub hooks_dir: String,
    /// Every file the install writes; the only ones it ever removes.
    pub files: Vec<FileHash>,
    /// `gitraptor/` once renamed into place.
    pub folder: Option<Identity>,
    /// The common `config` after the key was written.
    pub config: Option<Identity>,
    /// `raptor` and the dispatcher that were installed.
    pub raptor: String,
    pub template: u32,
    pub instance: String,
    pub prior: Prior,
    /// Branch the install confirms (`main`), or `None` when the repo has a team configuration
    /// and the base stays unconfirmed (Q-GRD-23).
    pub confirms_base: Option<String>,
    /// Base branches the minimum protects.
    pub protected_bases: Vec<String>,
}

impl Journal {
    pub const VERSION: u32 = 1;

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// `None` for an entry this binary cannot read (treated as "no install known").
    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str::<Self>(text)
            .ok()
            .filter(|j| j.version == Self::VERSION)
    }

    pub fn listed(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.path.as_str()).collect()
    }
}

/// The read-only snapshot the daemon exports for degraded mode (ADR-GRD-003 § 4): the last
/// confirmed base branch, so the client never opens the SQLite store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub repo_id: String,
    pub common_dir: String,
    pub confirmed_base: Option<String>,
    pub protected_bases: Vec<String>,
}

/// Path of the snapshot of a repo in the state folder.
pub fn snapshot_path(state: &std::path::Path, repo_id: &str) -> std::path::PathBuf {
    state.join("guardrails").join(format!("{repo_id}.json"))
}
