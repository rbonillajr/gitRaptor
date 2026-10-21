//! In-memory dry merge of two commits: which files would conflict, of what kind, and on which
//! line ranges of each side (ADR-CKP-001 § 1 (a) and § 3).
//!
//! Nothing is written to the observed repository: the objects the merge produces live in an
//! object memory dropped at the end of the job, no merge or diff driver, filter or attribute of
//! the repository takes part, and a missing object is never fetched. The output carries paths
//! and line numbers only, never file content.

mod conflicts;
mod hunks;
mod memory;
mod merge_attr;

pub use hunks::hunk_ranges;
pub use memory::merge_in_memory;

/// The two commits of a pair, as resolved hex ids (never ref names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryMergeInput {
    pub ours: String,
    pub theirs: String,
}

/// Caps of one dry merge. GitRaptor constants: a repository's configuration never changes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryMergeLimits {
    /// A blob above this size (read from its header, before loading) makes its file binary.
    pub max_blob_bytes: u64,
    /// Bytes the merge may write to its object memory; past it, [`DryMergeError::LimitExceeded`].
    pub max_written_bytes: u64,
    /// Rename-detection limit, in place of `merge.renameLimit`.
    pub rewrites_limit: usize,
    /// Conflicted files reported; past it, [`DryMergeOutcome::truncated`].
    pub max_files: usize,
    /// Ranges per file and side, after joining; past it, [`ConflictFile::hunks_truncated`].
    pub max_hunks_per_file: usize,
    /// Length of a conflict marker. Only a marker of exactly this length starts or ends a block.
    pub marker_size: u8,
    /// Two blocks this many lines apart or fewer, on both sides, are reported as one.
    pub hunk_join_gap: u32,
}

impl Default for DryMergeLimits {
    fn default() -> Self {
        Self {
            max_blob_bytes: 8 * 1024 * 1024,
            max_written_bytes: 64 * 1024 * 1024,
            rewrites_limit: 1_000,
            max_files: 200,
            max_hunks_per_file: 50,
            marker_size: 32,
            hunk_join_gap: 3,
        }
    }
}

impl DryMergeLimits {
    /// The marker length the merge writes and the extractor reads: the same on both ends.
    fn marker_len(&self) -> std::num::NonZeroU8 {
        std::num::NonZeroU8::new(self.marker_size).unwrap_or(std::num::NonZeroU8::MIN)
    }
}

/// The merge-base the merge used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeBaseUsed {
    Single(String),
    /// A criss-cross history: the merge of these bases, in byte order, served as the base.
    Virtual {
        bases: Vec<String>,
    },
    /// No common history: the empty tree served as the base.
    None,
}

/// What kind of conflict a file has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictKind {
    Content,
    AddAdd,
    ModifyDelete,
    Rename,
    DirectoryFile,
    /// Binary content, a symbolic link, or a blob above [`DryMergeLimits::max_blob_bytes`].
    Binary,
    Submodule,
}

/// A range of lines of one side's version: 1-based start and length. Never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineRange {
    pub start: u32,
    pub len: u32,
}

/// A file the merge leaves in conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
    pub kind: ConflictKind,
    /// Conflict blocks in the coordinates of ours and of theirs, one entry per block on each.
    pub ours: Vec<LineRange>,
    pub theirs: Vec<LineRange>,
    pub hunks_truncated: bool,
    /// The tree of either tip gives the file a `merge` attribute: the user's own merge may
    /// resolve what is reported here. A mark only; the rule is never applied.
    pub has_merge_rule: bool,
}

/// What the dry merge of a pair predicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryMergeOutcome {
    pub merge_base: MergeBaseUsed,
    /// In byte order of their paths; empty for a clean merge.
    pub files: Vec<ConflictFile>,
    /// More than [`DryMergeLimits::max_files`] files conflict.
    pub truncated: bool,
    /// A second merge with a raised rename limit produced this outcome. Never set here: the
    /// caller that runs that second merge sets it.
    pub rewrites_escalated: bool,
}

impl DryMergeOutcome {
    /// A file has a kind whose detection depends on the rename limit, so a second merge with a
    /// higher limit may change the outcome.
    pub fn has_kind_needing_escalation(&self) -> bool {
        self.files
            .iter()
            .any(|f| matches!(f.kind, ConflictKind::ModifyDelete | ConflictKind::Rename))
    }
}

/// Why a dry merge produced no outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryMergeError {
    /// An object the merge needs is not in the repository (a partial clone); it is never fetched.
    MissingObject,
    /// The repository is shallow and its history does not reach the merge-base.
    Shallow,
    /// An object is too large to process.
    TooLarge,
    /// The merge wrote more than [`DryMergeLimits::max_written_bytes`].
    LimitExceeded,
    /// The repository or an object could not be read. The text is for logs, never published.
    Unavailable(String),
}

impl std::fmt::Display for DryMergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingObject => f.write_str("an object of the merge is missing"),
            Self::Shallow => f.write_str("shallow repository without the merge-base"),
            Self::TooLarge => f.write_str("an object is too large"),
            Self::LimitExceeded => f.write_str("the merge exceeded its memory limit"),
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
        }
    }
}

impl std::error::Error for DryMergeError {}
