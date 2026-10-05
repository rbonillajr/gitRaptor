//! Git layer of the engine (ADR-GRP-009): the only crate that touches observed repositories.
//!
//! Reads go through gitoxide ([`RepoReader`]) with every user-configured program neutralized.
//! The system Git CLI is reachable only through the typed, closed allowlist in [`cli`], with a
//! fixed argv, an environment built from an allowlist and a timeout per invocation. The binary
//! itself is located and checked by [`resolve`].
//!
//! Boundary: nothing in this crate writes to an observed repository, takes a lock or runs a
//! filter, hook, pager, signing program or credential helper. The only writer is
//! [`tm_write::store`], the Time Machine store writer, which can only open a validated store in
//! the profile (ADR-TMC-002 § 1).

mod capture;
pub mod cli;
mod committed;
mod invoke;
pub mod paths;
mod reader;
pub mod redact;
pub mod refname;
pub mod resolve;
pub mod tm_write;

pub use capture::{
    Conversions, EntryKind, FileKind, FileStat, HistoryGap, IgnoreCheck, IndexEntry, IndexView,
    Untracked, UntrackedKind,
};
pub use committed::{BlobRead, CommittedFile, NotRegular};
pub use invoke::{ArgvSink, Invoker, MemoryArgvLog};
pub use reader::{
    Branch, Change, ChangeKind, Count, Head, InProgress, LinkedWorktree, ReaderOptions, RepoReader,
    Status,
};
pub use refname::RefName;
pub use resolve::{GitVersion, SystemGit};

/// Byte strings for paths, which need not be UTF-8.
pub use gix::bstr;

/// A Git object id (SHA-1). Opaque outside this crate, so no gix type leaks to the engine.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Oid(pub(crate) gix::ObjectId);

impl Oid {
    /// Parses 40 hex digits.
    pub fn from_hex(hex: &str) -> Option<Self> {
        gix::ObjectId::from_hex(hex.as_bytes()).ok().map(Self)
    }

    pub fn to_hex(&self) -> String {
        self.0.to_string()
    }

    /// The id of the empty tree.
    pub fn empty_tree() -> Self {
        Self(gix::ObjectId::empty_tree(gix::hash::Kind::Sha1))
    }
}

impl std::fmt::Display for Oid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Debug for Oid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Oid({})", self.0)
    }
}

/// Why a read could not produce data. A read never makes data up: it fails with one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The input was rejected before touching the file system or spawning anything.
    InvalidInput(String),
    /// The path is neither a worktree root nor a Git directory.
    NotARepository(String),
    /// The repository is not trusted by the `safe.directory` ownership rules (SEC-11).
    /// Reported as "not available" without touching any Git configuration.
    Untrusted(String),
    /// The read did not finish in time; retry later (ADR-GRP-009 § 3).
    TemporarilyUnavailable(String),
    /// The repository or object could not be read.
    Unavailable(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(m) => write!(f, "invalid input: {m}"),
            Self::NotARepository(m) => write!(f, "not a Git repository: {m}"),
            Self::Untrusted(m) => write!(f, "repository not trusted (safe.directory): {m}"),
            Self::TemporarilyUnavailable(m) => write!(f, "temporarily unavailable: {m}"),
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
        }
    }
}

impl std::error::Error for ReadError {}
