//! The conflict predictor's publication (DS-TS-CKP-001 § Contrato de API, D16): the summary of
//! every pair in the snapshot, the detail of `prediction.list` and the data of
//! `prediction.changed`. Paths travel as untrusted text (SEC-12, NFR-03), the declared limits as
//! codes, and no file content ever does.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::untrusted::Untrusted;

/// Most pair ids one `prediction.list` takes.
pub const MAX_LIST_PAIR_IDS: usize = 50;
/// Most pairs in one page of `prediction.list`.
pub const MAX_PAGE_PAIRS: usize = 50;
/// Most serialized bytes of one page of `prediction.list`.
pub const MAX_PAGE_BYTES: usize = 512 * 1024;
/// Length of a pair id: the hex of a 64-bit hash.
pub const PAIR_ID_LEN: usize = 16;

/// `RepoView.prediction`: the pairs of the repo, cut to fit the frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PredictionSummary {
    /// Always the eight of [`LimitCode::ALL`].
    pub limits: Vec<LimitCode>,
    pub pairs: Vec<PairSummary>,
    /// Some pairs did not fit in the frame: the rest come from `prediction.list`.
    pub pairs_truncated: bool,
}

/// One pair, as the snapshot and `prediction.changed` carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PairSummary {
    /// [`pair_id`] of the pair: stable across daemon restarts.
    pub pair_id: String,
    /// `scope_seq` of the event that last changed the pair.
    pub rev: u64,
    pub a: PairSideView,
    pub b: PairSideView,
    pub state: PairStateView,
    /// Only in `pending` and `not-computable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<PairReason>,
    /// Only in `current` and `recalculating` (there, the last result's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<PairLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed_at_utc_ms: Option<i64>,
    pub overlap_files: u32,
    pub overlap_partial: bool,
    pub conflict_files: u32,
    pub conflict_truncated: bool,
}

/// A side of a pair. A worktree goes by its root; the client derives names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum PairSideView {
    Worktree { root: Untrusted },
    Base {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PairStateView {
    Calculating,
    Current,
    Recalculating,
    Pending,
    NotComputable,
}

/// Why a pair is `pending` (the first four) or `not-computable` (the rest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PairReason {
    BaseUnconfirmed,
    OperationInProgress,
    PowerSaving,
    Dormant,
    BaseMissing,
    MissingObject,
    Shallow,
    LimitExceeded,
    Error,
}

/// What the pair's last result found. `none` on the wire (D17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PairLevel {
    #[serde(rename = "none")]
    Clean,
    Overlap,
    Conflict,
}

/// The declared limits of the prediction (ADR-CKP-001 § 7). The text is the clients'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LimitCode {
    OverlapIncludesUncommitted,
    ConflictCommittedOnly,
    NoMergeDriversOrAttributes,
    RenameLimit,
    InsertionsMayMerge,
    JoinedHunks,
    PairIgnoresBaseMoves,
    LocalOnly,
}

impl LimitCode {
    pub const ALL: [Self; 8] = [
        Self::OverlapIncludesUncommitted,
        Self::ConflictCommittedOnly,
        Self::NoMergeDriversOrAttributes,
        Self::RenameLimit,
        Self::InsertionsMayMerge,
        Self::JoinedHunks,
        Self::PairIgnoresBaseMoves,
        Self::LocalOnly,
    ];
}

/// One pair of `prediction.list`: its summary, with the same `rev`, and its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PairDetail {
    #[serde(flatten)]
    pub summary: PairSummary,
    /// At most `max_files` (200).
    pub overlap: Vec<Untrusted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConflictView {
    pub merge_base: MergeBaseKind,
    pub files: Vec<ConflictFileView>,
    pub truncated: bool,
    pub rewrites_escalated: bool,
}

/// `none` on the wire (D17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MergeBaseKind {
    Single,
    Virtual,
    #[serde(rename = "none")]
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConflictFileView {
    pub path: Untrusted,
    pub kind: ConflictKindView,
    pub ours: Vec<LineRange>,
    pub theirs: Vec<LineRange>,
    pub hunks_truncated: bool,
    /// The file has a `merge=` rule of its own in a tip's tree (limit 1, D14).
    pub has_merge_rule: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKindView {
    Content,
    AddAdd,
    ModifyDelete,
    Rename,
    DirectoryFile,
    Binary,
    Submodule,
}

/// Lines `start..start + len` of one side, 1-based. Never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineRange {
    pub start: u32,
    pub len: u32,
}

/// `prediction.list` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPredictionParams {
    pub repo_id: String,
    /// A worktree root: only the pairs that include it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// At most [`MAX_LIST_PAIR_IDS`], each of the form of [`pair_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// `prediction.list` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PredictionPage {
    pub limits: Vec<LimitCode>,
    pub pairs: Vec<PairDetail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `data` of `prediction.changed` (version 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PredictionChanged {
    /// What `scope_of` routes a repo event by.
    pub repo_id: String,
    pub pairs: Vec<PairSummary>,
    /// Pair ids of pairs that are gone.
    pub removed: Vec<String>,
}

/// Why `prediction.list` refused its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListParamsError {
    TooManyPairIds,
    MalformedPairId,
    MalformedCursor,
}

/// The id of a pair: the hex of a stable hash (no per-run salt) of the repo id, the root of `a`
/// and the side `b` (`None`: the base). Always [`PAIR_ID_LEN`] lowercase hex digits.
pub fn pair_id(repo_id: &str, a: &std::path::Path, b: Option<&std::path::Path>) -> String {
    let _ = (repo_id, a, b);
    unimplemented!("T009")
}

/// Whether `id` has the form of a [`pair_id`].
pub fn is_pair_id(id: &str) -> bool {
    let _ = id;
    unimplemented!("T009")
}

/// Checks `prediction.list` parameters before any lookup.
///
/// # Errors
///
/// More than [`MAX_LIST_PAIR_IDS`] ids, an id not of the form of [`pair_id`], or a cursor not
/// issued by [`page`].
pub fn validate_list(params: &ListPredictionParams) -> Result<(), ListParamsError> {
    let _ = params;
    unimplemented!("T009")
}

/// The summary of `pairs` that fits in `budget` serialized bytes: whole pairs in order, the
/// rest cut with `pairs_truncated: true`, and always the eight limits.
pub fn fit_summary(pairs: Vec<PairSummary>, budget: usize) -> PredictionSummary {
    let _ = (pairs, budget);
    unimplemented!("T009")
}

/// The page of `details` (already in page order) that starts at `cursor`: at most
/// [`MAX_PAGE_PAIRS`] pairs and [`MAX_PAGE_BYTES`] serialized, with `next_cursor` when more
/// remain. A pair that alone does not fit is cut in `overlap` (`overlap_partial: true`) and in
/// `conflict.files` (`truncated: true`).
pub fn page(details: Vec<PairDetail>, cursor: Option<&str>) -> PredictionPage {
    let _ = (details, cursor);
    unimplemented!("T009")
}

#[cfg(test)]
mod tests;
