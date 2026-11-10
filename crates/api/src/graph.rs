//! Types of `graph.lanes`: the commits of each worktree's branch over the confirmed base.
//!
//! Branch names, paths and commit subjects are untrusted text (SEC-12). Nothing here names an
//! actor, a session, an author or an email (the actor per commit waits for the engine), and the
//! response is never persisted.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::messages::CommitCountView;
use crate::untrusted::{Untrusted, UntrustedName};

/// Lanes per response; the rest are counted in `lanes_total` and `cut` says so.
pub const MAX_GRAPH_LANES: u32 = 32;
/// Commits listed per lane, newest first.
pub const MAX_LANE_COMMITS: usize = 50;
/// Bytes of a commit subject.
pub const MAX_LANE_SUMMARY_BYTES: usize = 256;
/// Deadline of one `graph.lanes` query, in milliseconds (the value of `DIFF_DEADLINE_MS`).
pub const GRAPH_DEADLINE_MS: u64 = 2_000;
/// Serialized response cap, under `MAX_MESSAGE_BYTES`.
pub const MAX_GRAPH_RESPONSE_BYTES: usize = 768 * 1024;

/// `graph.lanes` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphParams {
    pub repo_id: String,
}

/// `graph.lanes` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphLanes {
    pub computed_at_utc_ms: i64,
    pub base: GraphBase,
    /// Empty when the base is not ready.
    pub lanes: Vec<Lane>,
    /// Every lane of the repo, also the ones that did not fit.
    pub lanes_total: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<GraphCut>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GraphBase {
    Ready { name: UntrustedName },
    NotComputable { reason: BaseGap },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BaseGap {
    BaseUnconfirmed { name: UntrustedName },
    NoBase,
    BaseMissing { name: UntrustedName },
    RepoUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GraphCut {
    Lanes,
    Bytes,
}

/// One worktree's branch drawn from its merge-base with the base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    /// Root of the worktree, to match the fleet row.
    pub worktree: Untrusted,
    pub head: LaneHead,
    pub state: LaneState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LaneHead {
    Branch { name: UntrustedName },
    Detached,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LaneState {
    Ready {
        merge_base: String,
        commits: Vec<LaneCommit>,
        total: CommitCountView,
    },
    NotComputable {
        reason: LaneGap,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LaneGap {
    WorktreeUnavailable,
    NoCommits,
    NoMergeBase,
    TooSlow,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaneCommit {
    /// Full hex object id.
    pub oid: String,
    /// First line of the message, at most [`MAX_LANE_SUMMARY_BYTES`].
    pub summary: Untrusted,
    pub committed_utc_ms: i64,
    /// Two or more parents.
    pub merge: bool,
}
