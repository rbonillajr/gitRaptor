//! Manual snapshots: the point a Time Machine takes when an agent asks for one, with its own
//! quota. Compile stubs only: the signatures of the contract, no behaviour.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::methods::QuotaWindow;

use super::continuous::{CaptureDeps, FreeSpaceFloor};
use super::oplog::{Channel, Oplog, Requester};
use super::store::{CaptureError, SnapshotStore};

pub const PER_MINUTE: usize = 5;
pub const MINUTE_MS: i64 = 60_000;
pub const PER_DAY: usize = 20;
pub const DAY_MS: i64 = 86_400_000;
pub const PER_WORKTREE_DAY: usize = 60;
pub const PER_REPO_DAY: usize = 200;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MANUAL_BUDGET: Duration = Duration::from_secs(25);

/// What a manual snapshot is asked for.
#[derive(Debug, Clone)]
pub struct ManualAsk {
    pub repo_id: String,
    pub worktree: PathBuf,
    pub label: String,
    pub requester: Requester,
    pub channel: Channel,
}

/// A manual snapshot that was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualCaptured {
    pub snapshot_id: String,
    pub worktree: PathBuf,
}

/// The window that refused a request and when a slot frees up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaHit {
    pub window: QuotaWindow,
    pub release_at_ms: i64,
}

/// What the quota counts, read once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuotaInput {
    pub requester_ms: Vec<i64>,
    pub worktree_ms: Vec<i64>,
    pub repo_ms: Vec<i64>,
}

#[derive(Debug)]
pub enum ManualError {
    Quota(QuotaHit),
    NoSpace,
    InProgress,
    InFlight,
    Busy,
    Discarded,
    TimeLimit,
    Unavailable,
    Capture(CaptureError),
}

/// Pure: requester minute, requester day, worktree day, repo day.
pub fn quota(input: &QuotaInput, now_ms: i64) -> Result<(), QuotaHit> {
    let _ = (input, now_ms);
    todo!("US-MCP-008")
}

/// Indexed read of the oplog, without the store's recording lock (used by prepare).
pub fn precheck(
    oplog: &Mutex<Oplog>,
    store: &SnapshotStore,
    session_id: &str,
    worktree_key: &str,
    now_ms: i64,
) -> Result<(), QuotaHit> {
    let _ = (oplog, store, session_id, worktree_key, now_ms);
    todo!("US-MCP-008")
}

/// Free space seen by the manual capture: injectable so tests cross the floor.
pub trait FreeSpaceProbe: Send + Sync {
    fn available_bytes(&self, path: &Path) -> io::Result<u64>;
}

/// The manual floor: the continuous floor plus a reserve for the guaranteed prior.
pub struct ManualFloor<'a> {
    pub floor: FreeSpaceFloor,
    pub reserve_bytes: u64,
    pub probe: &'a dyn FreeSpaceProbe,
}

/// In flight, recording lock within `deadline`; count, check, capture and record under it.
#[allow(clippy::too_many_arguments)]
pub fn capture_in_store(
    store: &SnapshotStore,
    oplog: &Mutex<Oplog>,
    ask: &ManualAsk,
    engine_mark: Option<i64>,
    include_credentials: bool,
    floor: Option<&ManualFloor<'_>>,
    now_ms: i64,
    deadline: Instant,
) -> Result<ManualCaptured, ManualError> {
    let _ = (
        store,
        oplog,
        ask,
        engine_mark,
        include_credentials,
        floor,
        now_ms,
        deadline,
    );
    todo!("US-MCP-008")
}

/// The daemon's path: settle, busy index, free-space floor, then `capture_in_store`.
pub fn capture(
    deps: &CaptureDeps,
    ask: &ManualAsk,
    now_ms: i64,
) -> Result<ManualCaptured, ManualError> {
    let _ = (deps, ask, now_ms);
    todo!("US-MCP-008")
}

pub fn wall_now_ms() -> i64 {
    todo!("US-MCP-008")
}
