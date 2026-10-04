//! The `meta` blob of a snapshot (ADR-TMC-001 § 1): what a tree cannot hold. It is untrusted
//! input when read back (SEC-TMC-09): parsed strictly, unknown fields rejected.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::timemachine::oplog::Exclusion;

/// Format of [`Meta`].
pub const META_FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub format: u32,
    /// Keys of the worktrees in the snapshot, as in `wt/<key>/`.
    pub scope: Vec<String>,
    pub worktrees: Vec<MetaWorktree>,
    /// Every worktree of the repo, in or out of the scope.
    pub registered: Vec<RegisteredWorktree>,
    /// `refs/heads/*` → commit.
    pub branches: BTreeMap<String, String>,
    pub stash: Option<String>,
    /// Paths left out, with the reason.
    pub exclusions: Vec<Exclusion>,
    /// History the store could not copy (`shallow`, `partial-clone`).
    pub gaps: Vec<String>,
}

/// A worktree in the scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetaWorktree {
    pub key: String,
    pub path: String,
    /// Branch `HEAD` names, if symbolic.
    pub head_branch: Option<String>,
    /// Commit `HEAD` resolves to, if any.
    pub head_commit: Option<String>,
    pub detached: bool,
    /// Index marks a tree does not keep.
    pub intent_to_add: Vec<String>,
    pub skip_worktree: Vec<String>,
    pub conflicts: Vec<ConflictEntry>,
}

/// One stage of a conflicted index entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictEntry {
    pub path: String,
    pub stage: u8,
    pub kind: String,
    pub id: String,
}

/// A worktree registered in the repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredWorktree {
    /// `None` for the main worktree.
    pub id: Option<String>,
    pub path: String,
    pub branch: Option<String>,
    pub locked: bool,
}

impl Meta {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("meta serializes")
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let meta: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if meta.format != META_FORMAT {
            return Err(format!("unknown meta format {}", meta.format));
        }
        Ok(meta)
    }
}
