//! An FSEvents stream with exclusion paths and resumption (ADR-GRP-010, Enmienda 2026-10-08).
//!
//! `notify` 8.2 fixes the stream's flags and latency, offers no exclusion and loses the events
//! between two streams. This one is the same stream (file events, latency 0) with what the
//! engine needs on top: up to [`MAX_EXCLUSIONS`] folders `fseventsd` filters out before the
//! daemon is called, and a `since_when` event id to start from, so a stream that replaces
//! another one starts where the old one stopped.
//!
//! The callback runs on a serial dispatch queue the stream owns. It must not drop the stream
//! it belongs to (dropping waits for that queue).

use std::path::{Path, PathBuf};

/// Most folders FSEvents lets one stream exclude.
pub const MAX_EXCLUSIONS: usize = 8;

/// `since_when` for "from now on".
pub const SINCE_NOW: u64 = crate::ffi_fsevents::SINCE_NOW;

/// Flags of an event (`FSEventStreamEventFlags`).
pub mod flag {
    pub const MUST_SCAN_SUBDIRS: u32 = 0x0000_0001;
    pub const USER_DROPPED: u32 = 0x0000_0002;
    pub const KERNEL_DROPPED: u32 = 0x0000_0004;
    pub const EVENT_IDS_WRAPPED: u32 = 0x0000_0008;
    pub const HISTORY_DONE: u32 = 0x0000_0010;
    pub const ROOT_CHANGED: u32 = 0x0000_0020;
    pub const MOUNT: u32 = 0x0000_0040;
    pub const UNMOUNT: u32 = 0x0000_0080;
    pub const ITEM_CREATED: u32 = 0x0000_0100;
    pub const ITEM_REMOVED: u32 = 0x0000_0200;
    pub const ITEM_INODE_META_MOD: u32 = 0x0000_0400;
    pub const ITEM_RENAMED: u32 = 0x0000_0800;
    pub const ITEM_MODIFIED: u32 = 0x0000_1000;
    pub const ITEM_IS_DIR: u32 = 0x0002_0000;

    /// The OS lost events or the watched tree changed identity: what happened is unknown and
    /// the whole tree must be read again.
    pub const LOSS: u32 = MUST_SCAN_SUBDIRS
        | USER_DROPPED
        | KERNEL_DROPPED
        | EVENT_IDS_WRAPPED
        | ROOT_CHANGED
        | MOUNT
        | UNMOUNT;
}

/// One event of a batch. Borrowed: it lives for the callback only.
#[derive(Debug, Clone, Copy)]
pub struct Event<'a> {
    pub path: &'a Path,
    /// [`flag`] bits.
    pub flags: u32,
    /// `0` for the marks that carry no event (`HISTORY_DONE`, `EVENT_IDS_WRAPPED`).
    pub id: u64,
}

/// Receives every batch the OS delivers, on the stream's queue.
pub type Handler = Box<dyn Fn(&[Event<'_>]) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    /// A path that is not UTF-8, does not exist or cannot be made canonical.
    InvalidPath,
    /// More than [`MAX_EXCLUSIONS`] folders.
    TooManyExclusions,
    /// The OS did not create the stream.
    Create,
    /// The OS rejected the exclusion list.
    Exclusions,
    /// The OS did not start the stream.
    Start,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "path not usable by FSEvents",
            Self::TooManyExclusions => "more exclusion paths than FSEvents allows",
            Self::Create => "FSEvents did not create the stream",
            Self::Exclusions => "FSEvents rejected the exclusion paths",
            Self::Start => "FSEvents did not start the stream",
        })
    }
}

impl std::error::Error for StreamError {}

/// A running stream over one root. Dropping it stops it, and no callback runs after the drop
/// returns.
pub struct Stream {
    raw: crate::ffi_fsevents::Raw,
    root: PathBuf,
    exclusions: Vec<PathBuf>,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("root", &self.root)
            .field("exclusions", &self.exclusions)
            .finish_non_exhaustive()
    }
}

fn canonical(path: &Path) -> Result<PathBuf, StreamError> {
    path.canonicalize().map_err(|_| StreamError::InvalidPath)
}

impl Stream {
    /// Starts a stream over `root` (recursive), skipping `exclusions`, from `since_when`
    /// ([`SINCE_NOW`] or the id of an earlier event). Paths are made canonical (`/var` is
    /// `/private/var`), which is how the OS reports them, and the exclusions must exist and be
    /// under `root`: the ones that are not are skipped.
    pub fn start(
        root: &Path,
        exclusions: &[PathBuf],
        since_when: u64,
        handler: Handler,
    ) -> Result<Self, StreamError> {
        if exclusions.len() > MAX_EXCLUSIONS {
            return Err(StreamError::TooManyExclusions);
        }
        let root = canonical(root)?;
        let mut kept = Vec::with_capacity(exclusions.len());
        for e in exclusions {
            if let Ok(e) = canonical(e)
                && e != root
                && e.starts_with(&root)
                && !kept.contains(&e)
            {
                kept.push(e);
            }
        }
        let root_str = root.to_str().ok_or(StreamError::InvalidPath)?;
        let excluded: Vec<&str> = kept
            .iter()
            .map(|e| e.to_str().ok_or(StreamError::InvalidPath))
            .collect::<Result<_, _>>()?;
        let raw = crate::ffi_fsevents::create(root_str, &excluded, since_when, handler)?;
        Ok(Self {
            raw,
            root,
            exclusions: kept,
        })
    }

    /// The canonical root the stream watches.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The canonical folders it excludes.
    pub fn exclusions(&self) -> &[PathBuf] {
        &self.exclusions
    }

    /// The highest event id delivered so far, or the one the stream started from. A new stream
    /// started from it misses nothing the old one had not delivered; call
    /// [`flush_sync`](Self::flush_sync) first to take what the OS already has.
    pub fn last_event_id(&self) -> u64 {
        self.raw.last_event_id()
    }

    /// Delivers every event the OS already has and returns once the callback ran for them.
    /// Not from the callback itself.
    pub fn flush_sync(&self) {
        self.raw.flush_sync();
    }
}
