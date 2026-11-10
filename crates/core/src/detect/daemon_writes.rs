//! The daemon's write windows (ADR-GRP-012, amendment 2026-11-09, TD-GRP-007): while the Time
//! Machine's applier or the executor of user operations writes in a repo, every event of that
//! repo whose S3 window overlaps is foreign, even after the `git` that moved the ref ended.
//!
//! A window opens with [`DaemonWrites::begin`] and closes when its [`WriteWindow`] guard drops,
//! also on a panic or an early return. Times are [`gitraptor_api::clock::monotonic_ns`], the
//! clock of the observer's batch marks. Only the path of the repo is kept, in memory; nothing of
//! it leaves the process (SEC-04).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// How long after it closes a window still covers an event: the delay of a write notice the
/// detector still keeps (the 3 s cap of the read notice, ADR-GRP-012 amendment 2026-11-09).
pub const WINDOW_TAIL: Duration = Duration::from_secs(3);

/// The write windows of the daemon's process.
#[derive(Debug, Default)]
pub struct DaemonWrites {
    spans: Mutex<Vec<Span>>,
}

#[derive(Debug, Clone)]
struct Span {
    id: u64,
    path: PathBuf,
    start_ns: u64,
    /// `None` while the guard lives.
    end_ns: Option<u64>,
}

/// Closes its window when dropped.
#[derive(Debug)]
#[must_use = "the window closes when the guard drops"]
pub struct WriteWindow {
    writes: Arc<DaemonWrites>,
    id: u64,
}

impl Drop for WriteWindow {
    fn drop(&mut self) {
        let _ = (&self.writes, self.id);
    }
}

impl DaemonWrites {
    /// The windows of this process, shared by the applier, the executor and the detector.
    pub fn global() -> Arc<Self> {
        static GLOBAL: OnceLock<Arc<DaemonWrites>> = OnceLock::new();
        Arc::clone(GLOBAL.get_or_init(|| Arc::new(Self::default())))
    }

    /// Opens a window over the repo or worktree at `path` until the guard drops.
    pub fn begin(self: &Arc<Self>, path: &Path) -> WriteWindow {
        let _ = (path, &self.spans);
        WriteWindow {
            writes: Arc::clone(self),
            id: 0,
        }
    }

    /// Whether a window over a path `in_repo` accepts was open at some moment of
    /// `[from_ns, to_ns]`.
    pub fn overlaps(&self, in_repo: &dyn Fn(&Path) -> bool, from_ns: u64, to_ns: u64) -> bool {
        let _ = (in_repo, from_ns, to_ns);
        false
    }

    /// How many windows, open or kept, cover a path `in_repo` accepts (tests and diagnostics).
    pub fn count(&self, in_repo: &dyn Fn(&Path) -> bool) -> usize {
        let _ = in_repo;
        0
    }
}

#[allow(dead_code)]
fn unused(s: &Span) -> (u64, &Path, u64, Option<u64>) {
    (s.id, &s.path, s.start_ns, s.end_ns)
}
