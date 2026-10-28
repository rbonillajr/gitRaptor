//! The conflict predictor's model: the pairs of a repo, each worktree's own work, the overlap of
//! every pair, its state, the cache keyed by triple and the queue of dry merges.
//!
//! Pure over what the caller reads: [`pairs::plan`] takes a [`gitraptor_git::RepoReader`] for
//! merge-bases and changed paths and nothing else touches a repo. Nothing here is persisted or
//! published, and nothing runs a dry merge: [`state::RepoPairs::apply`] says which pairs to
//! queue and [`state::RepoPairs::complete`] takes the result back.

pub mod merger;
pub mod overlap;
pub mod pairs;
pub mod pool;
pub mod queue;
pub mod state;
pub mod worker;

use std::path::PathBuf;

use gitraptor_api::messages::BaseStatusView;
use gitraptor_git::dry_merge::DryMergeOutcome;

use crate::observe::UncommittedPaths;
use crate::predict::worker::WorkerConfig;

/// How the daemon runs the predictor's dry merges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PredictMerger {
    /// No dry merge runs and nothing is published: the daemon behaves as before the predictor
    /// (tests that do not exercise it).
    #[default]
    Off,
    /// In the daemon's own process (tests of the publication).
    InProcess,
    /// The worker process (production: the daemon's own binary, resolved once at start).
    Worker(WorkerConfig),
}

/// The predictor's part of the daemon's configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PredictConfig {
    pub merger: PredictMerger,
}

/// The other side of a pair: the base branch or another worktree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PairSide {
    Base,
    Worktree(PathBuf),
}

/// A pair of one repo. Between worktrees, `a` is the root lowest in byte order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PairKey {
    pub repo_id: String,
    pub a: PathBuf,
    pub b: PairSide,
}

/// What a dry merge of a pair depends on: both tips and their first merge-base in byte order.
/// The key of the cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Triple {
    pub tip_a: String,
    pub tip_b: String,
    pub merge_base: Option<String>,
}

/// Why a pair waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingReason {
    BaseUnconfirmed,
    OperationInProgress,
    PowerSaving,
    Dormant,
}

/// Why a pair has no prediction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotComputableReason {
    BaseMissing,
    MissingObject,
    Shallow,
    LimitExceeded,
    Error,
}

/// The life of a pair's prediction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairState {
    /// Queued, with no earlier result.
    Calculating,
    /// The result is the one of the pair's current triple.
    Current,
    /// Queued again; the last result is still kept.
    Recalculating,
    Pending(PendingReason),
    NotComputable(NotComputableReason),
}

/// The paths both sides of a pair touch, sorted. `partial`: an input was cut, so there may be
/// more; never an empty overlap that hides a cut.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overlap {
    pub files: Vec<String>,
    pub partial: bool,
}

/// One pair as the predictor knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairPrediction {
    pub key: PairKey,
    /// `None` when a tip or the base is missing.
    pub triple: Option<Triple>,
    pub state: PairState,
    /// When the kept result was computed, UTC.
    pub computed_at_utc_ms: Option<i64>,
    pub overlap: Overlap,
    pub conflict: Option<DryMergeOutcome>,
}

/// The base branch of a repo as the plan needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseInput {
    pub status: BaseStatusView,
    /// Its tip; `None` when the branch has no ref.
    pub tip: Option<String>,
    /// The worktree that has it checked out, if any: it is not paired against the base.
    pub checked_out_at: Option<PathBuf>,
}

/// One worktree as the plan needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInput {
    pub root: PathBuf,
    /// Its `HEAD` commit; `None` on a branch without commits.
    pub tip: Option<String>,
    pub uncommitted: UncommittedPaths,
    /// A rebase or merge is half done.
    pub in_progress: bool,
    /// An agent session is present: its pairs go first in the initial calculation.
    pub session_present: bool,
}
