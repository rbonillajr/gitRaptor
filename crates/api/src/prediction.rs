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
///
/// The hash is 64-bit FNV-1a over a length-prefixed encoding of the three fields, so no two
/// distinct triples share an encoding and the id survives a daemon restart.
pub fn pair_id(repo_id: &str, a: &std::path::Path, b: Option<&std::path::Path>) -> String {
    let mut hash = Fnv1a::new();
    hash.write(b"gitraptor.pair.v1");
    hash.field(repo_id.as_bytes());
    hash.field(a.as_os_str().as_encoded_bytes());
    match b {
        None => hash.write(&[0]),
        Some(b) => {
            hash.write(&[1]);
            hash.field(b.as_os_str().as_encoded_bytes());
        }
    }
    format!("{:016x}", hash.0)
}

/// 64-bit FNV-1a: stable across runs and platforms, unlike `std`'s randomly keyed hasher.
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    /// A field with its length first, so `("ab", "c")` and `("a", "bc")` differ.
    fn field(&mut self, bytes: &[u8]) {
        self.write(&(bytes.len() as u64).to_le_bytes());
        self.write(bytes);
    }
}

/// Whether `id` has the form of a [`pair_id`].
pub fn is_pair_id(id: &str) -> bool {
    id.len() == PAIR_ID_LEN && id.bytes().all(is_lower_hex)
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

/// Prefix of the cursors [`page`] issues: an opaque token for the client.
const CURSOR_PREFIX: &str = "pc1.";

fn cursor_for(offset: usize) -> String {
    format!("{CURSOR_PREFIX}{offset:x}")
}

/// The offset a cursor of [`page`] carries; `None` for any other text. A page never issues
/// offset 0 (the first page goes without a cursor).
fn cursor_offset(cursor: &str) -> Option<usize> {
    let digits = cursor.strip_prefix(CURSOR_PREFIX)?;
    if digits.is_empty() || digits.len() > 16 || !digits.bytes().all(is_lower_hex) {
        return None;
    }
    usize::from_str_radix(digits, 16)
        .ok()
        .filter(|offset| *offset > 0)
}

/// Checks `prediction.list` parameters before any lookup.
///
/// # Errors
///
/// More than [`MAX_LIST_PAIR_IDS`] ids, an id not of the form of [`pair_id`], or a cursor not
/// issued by [`page`].
pub fn validate_list(params: &ListPredictionParams) -> Result<(), ListParamsError> {
    if let Some(ids) = &params.pair_ids {
        if ids.len() > MAX_LIST_PAIR_IDS {
            return Err(ListParamsError::TooManyPairIds);
        }
        if !ids.iter().all(|id| is_pair_id(id)) {
            return Err(ListParamsError::MalformedPairId);
        }
    }
    if let Some(cursor) = &params.cursor
        && cursor_offset(cursor).is_none()
    {
        return Err(ListParamsError::MalformedCursor);
    }
    Ok(())
}

/// Serialized size of `value`; a value that does not serialize never fits.
fn json_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

/// Bytes one more element adds to a JSON array: itself and, at worst, a comma.
fn element_len<T: Serialize>(value: &T) -> usize {
    json_len(value).saturating_add(1)
}

/// The summary of `pairs` that fits in `budget` serialized bytes: whole pairs in order, the
/// rest cut with `pairs_truncated: true`, and always the eight limits.
pub fn fit_summary(pairs: Vec<PairSummary>, budget: usize) -> PredictionSummary {
    let mut used = json_len(&PredictionSummary {
        limits: LimitCode::ALL.to_vec(),
        pairs: Vec::new(),
        // The longer of the two values, so the count is an upper bound.
        pairs_truncated: false,
    });
    let total = pairs.len();
    let mut kept = Vec::with_capacity(total);
    for pair in pairs {
        let size = element_len(&pair);
        if used.saturating_add(size) > budget {
            break;
        }
        used += size;
        kept.push(pair);
    }
    PredictionSummary {
        limits: LimitCode::ALL.to_vec(),
        pairs_truncated: kept.len() < total,
        pairs: kept,
    }
}

/// The page of `details` (already in page order) that starts at `cursor`: at most
/// [`MAX_PAGE_PAIRS`] pairs and [`MAX_PAGE_BYTES`] serialized, with `next_cursor` when more
/// remain. A pair that alone does not fit is cut in `overlap` (`overlap_partial: true`) and in
/// `conflict.files` (`truncated: true`).
///
/// A cursor not issued by `page` starts at the first pair: [`validate_list`] refuses it first.
pub fn page(details: Vec<PairDetail>, cursor: Option<&str>) -> PredictionPage {
    let start = cursor.and_then(cursor_offset).unwrap_or(0);
    let total = details.len();
    let mut used = json_len(&PredictionPage {
        limits: LimitCode::ALL.to_vec(),
        pairs: Vec::new(),
        next_cursor: Some(cursor_for(usize::MAX)),
    });
    let mut pairs = Vec::new();
    for detail in details.into_iter().skip(start) {
        if pairs.len() == MAX_PAGE_PAIRS {
            break;
        }
        let size = element_len(&detail);
        if used.saturating_add(size) <= MAX_PAGE_BYTES {
            used += size;
            pairs.push(detail);
        } else if pairs.is_empty() {
            // Alone it does not fit: cut it rather than drop it.
            let cut = cut_detail(detail, MAX_PAGE_BYTES.saturating_sub(used));
            pairs.push(cut);
            break;
        } else {
            break;
        }
    }
    let next = start.saturating_add(pairs.len());
    PredictionPage {
        limits: LimitCode::ALL.to_vec(),
        pairs,
        next_cursor: (next < total).then(|| cursor_for(next)),
    }
}

/// `detail` with as many of its overlap paths and conflict files, in order and taken in turns,
/// as fit `budget` serialized bytes as an array element. A list that loses entries is flagged.
fn cut_detail(detail: PairDetail, budget: usize) -> PairDetail {
    let PairDetail {
        mut summary,
        overlap,
        conflict,
    } = detail;
    let (mut conflict, files) = match conflict {
        Some(mut c) => {
            let files = std::mem::take(&mut c.files);
            (Some(c), files)
        }
        None => (None, Vec::new()),
    };
    let (overlap_total, files_total) = (overlap.len(), files.len());
    let partial_before = summary.overlap_partial;
    let truncated_before = (
        summary.conflict_truncated,
        conflict.as_ref().is_some_and(|c| c.truncated),
    );
    // Measured with the flags a cut sets, so clearing one later never grows the size.
    summary.overlap_partial |= overlap_total > 0;
    summary.conflict_truncated |= files_total > 0;
    if let Some(c) = conflict.as_mut() {
        c.truncated |= files_total > 0;
    }
    let mut cut = PairDetail {
        summary,
        overlap: Vec::with_capacity(overlap_total),
        conflict,
    };
    let mut used = element_len(&cut);
    let mut overlap = overlap.into_iter();
    let mut files = files.into_iter();
    let mut overlap_open = overlap_total > 0;
    let mut files_open = cut.conflict.is_some() && files_total > 0;
    while overlap_open || files_open {
        if overlap_open {
            overlap_open = match overlap.next() {
                Some(path) if used.saturating_add(element_len(&path)) <= budget => {
                    used += element_len(&path);
                    cut.overlap.push(path);
                    true
                }
                _ => false,
            };
        }
        if files_open && let Some(c) = cut.conflict.as_mut() {
            files_open = match files.next() {
                Some(file) if used.saturating_add(element_len(&file)) <= budget => {
                    used += element_len(&file);
                    c.files.push(file);
                    true
                }
                _ => false,
            };
        }
    }
    // A list that kept every entry keeps the flags it came with.
    if cut.overlap.len() == overlap_total {
        cut.summary.overlap_partial = partial_before;
    }
    if let Some(c) = cut.conflict.as_mut()
        && c.files.len() == files_total
    {
        cut.summary.conflict_truncated = truncated_before.0;
        c.truncated = truncated_before.1;
    }
    cut
}

/// `changed` split into batches whose `scope.event` notification fits in `budget` bytes, in
/// order; each carries the `repo_id`, and the `removed` ids go in the first.
///
/// A pair that alone does not fit a batch travels alone in its own.
pub fn split_changed(changed: PredictionChanged, budget: usize) -> Vec<PredictionChanged> {
    if json_len(&changed) <= budget {
        return vec![changed];
    }
    let PredictionChanged {
        repo_id,
        pairs,
        removed,
    } = changed;
    let empty = |removed: Vec<String>| PredictionChanged {
        repo_id: repo_id.clone(),
        pairs: Vec::new(),
        removed,
    };
    let mut batches = Vec::new();
    let mut batch = empty(removed);
    let mut used = json_len(&batch);
    for pair in pairs {
        let size = element_len(&pair);
        if !batch.pairs.is_empty() && used.saturating_add(size) > budget {
            batches.push(std::mem::replace(&mut batch, empty(Vec::new())));
            used = json_len(&batch);
        }
        used = used.saturating_add(size);
        batch.pairs.push(pair);
    }
    batches.push(batch);
    batches
}

#[cfg(test)]
mod tests;
