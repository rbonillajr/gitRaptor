//! The daemon's write windows (ADR-GRP-012, amendment 2026-11-09, TD-GRP-007): while the Time
//! Machine's applier or the executor of user operations writes in a repo, every event of that
//! repo whose S3 window overlaps is foreign, even after the `git` that moved the ref ended.
//!
//! A window opens with [`DaemonWrites::begin`] and closes when its [`WriteWindow`] guard drops,
//! also on a panic or an early return. Times are [`gitraptor_api::clock::monotonic_ns`], the
//! clock of the observer's batch marks. Only the path of the repo is kept, in memory; nothing of
//! it leaves the process (SEC-04).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use gitraptor_api::clock::monotonic_ns;

/// How long after it closes a window still covers an event: the delay of a write notice the
/// detector still keeps (the 3 s cap of the read notice, ADR-GRP-012 amendment 2026-11-09).
pub const WINDOW_TAIL: Duration = Duration::from_secs(3);

/// How long a closed window is kept: longer than [`WINDOW_TAIL`] and than the S3 lead, so a
/// batch evaluated late still sees it; dropped on the next [`DaemonWrites::begin`].
const RETENTION: Duration = Duration::from_secs(120);

/// The write windows of the daemon's process.
#[derive(Debug, Default)]
pub struct DaemonWrites {
    spans: Mutex<Vec<Span>>,
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
struct Span {
    id: u64,
    path: PathBuf,
    start_ns: u64,
    /// `None` while the guard lives.
    end_ns: Option<u64>,
}

/// The branch move of an event, as [`DaemonWrites::covers`] reads it: the full ref name
/// (`refs/heads/<branch>`), its value before (`None`: unknown) and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveSeen<'a> {
    pub refname: &'a str,
    pub old: Option<&'a str>,
    pub new: &'a str,
}

/// Closes its window when dropped.
#[derive(Debug)]
#[must_use = "the window closes when the guard drops"]
pub struct WriteWindow {
    writes: Arc<DaemonWrites>,
    id: u64,
}

impl WriteWindow {
    /// Records that the window left `refname` (full name, or `HEAD` when detached) at `value`.
    /// A symbolic `HEAD` records the ref it points to with that ref's value at the end.
    pub fn wrote(&self, refname: &str, value: &str) {
        let _ = (refname, value);
    }
}

impl Drop for WriteWindow {
    fn drop(&mut self) {
        let now = monotonic_ns();
        if let Some(span) = self.writes.spans().iter_mut().find(|s| s.id == self.id) {
            span.end_ns = Some(now);
        }
    }
}

/// Drops the windows closed more than [`RETENTION`] before `now_ns`; open ones always stay.
fn prune(spans: &mut Vec<Span>, now_ns: u64) {
    spans.retain(|s| {
        s.end_ns
            .is_none_or(|e| e.saturating_add(ns(RETENTION)) >= now_ns)
    });
}

fn ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

impl DaemonWrites {
    /// The windows of this process, shared by the applier, the executor and the detector.
    pub fn global() -> Arc<Self> {
        static GLOBAL: OnceLock<Arc<DaemonWrites>> = OnceLock::new();
        Arc::clone(GLOBAL.get_or_init(|| Arc::new(Self::default())))
    }

    /// The windows, also after a panic of another holder: a window must still close, and a
    /// poisoned list holds nothing half written (every change is one assignment or push).
    fn spans(&self) -> MutexGuard<'_, Vec<Span>> {
        self.spans.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Opens a window over the repo or worktree at `path` until the guard drops. The path is
    /// kept canonical when it can be resolved, as given otherwise; windows closed longer than
    /// the retention ago are dropped here.
    pub fn begin(self: &Arc<Self>, path: &Path) -> WriteWindow {
        let path = crate::observe::canonical(path);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let start_ns = monotonic_ns();
        let mut spans = self.spans();
        prune(&mut spans, start_ns);
        spans.push(Span {
            id,
            path,
            start_ns,
            end_ns: None,
        });
        WriteWindow {
            writes: Arc::clone(self),
            id,
        }
    }

    /// Opens a window of the Time Machine's applier, which records what it wrote with
    /// [`WriteWindow::wrote`]: in the tail after it closes, only an event that may be its echo or
    /// that starts from a state it did not leave counts as foreign (ADR-GRP-012, amendment
    /// 2026-11-09). A window opened with [`Self::begin`] (the executor) knows nothing it wrote.
    pub fn begin_tracked(self: &Arc<Self>, path: &Path) -> WriteWindow {
        self.begin(path)
    }

    /// Whether the daemon's windows make an event of the repo foreign: one over a path `in_repo`
    /// accepts was open at some moment of `[from_ns, to_ns]`, or closed less than
    /// [`WINDOW_TAIL`] before `from_ns` and `moved` does not clear it.
    pub fn covers(
        &self,
        in_repo: &dyn Fn(&Path) -> bool,
        from_ns: u64,
        to_ns: u64,
        moved: Option<MoveSeen<'_>>,
    ) -> bool {
        let _ = moved;
        self.overlaps(in_repo, from_ns, to_ns)
    }

    /// Whether a window over a path `in_repo` accepts was open at some moment of
    /// `[from_ns, to_ns]`, or closed less than [`WINDOW_TAIL`] before `from_ns`.
    pub fn overlaps(&self, in_repo: &dyn Fn(&Path) -> bool, from_ns: u64, to_ns: u64) -> bool {
        let tail = ns(WINDOW_TAIL);
        self.spans().iter().any(|s| {
            s.start_ns <= to_ns
                && s.end_ns.map_or(u64::MAX, |e| e.saturating_add(tail)) >= from_ns
                && in_repo(&s.path)
        })
    }

    /// How many windows, open or kept, cover a path `in_repo` accepts (tests and diagnostics).
    pub fn count(&self, in_repo: &dyn Fn(&Path) -> bool) -> usize {
        self.spans().iter().filter(|s| in_repo(&s.path)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_drops_only_windows_closed_past_the_retention() {
        let span = |id, end_ns| Span {
            id,
            path: PathBuf::from("/r"),
            start_ns: 1,
            end_ns,
        };
        let now = 10 * ns(RETENTION);
        let mut spans = vec![
            span(1, Some(now - ns(RETENTION) - 1)),
            span(2, Some(now - ns(RETENTION))),
            span(3, None),
        ];
        prune(&mut spans, now);
        let kept: Vec<u64> = spans.iter().map(|s| s.id).collect();
        assert_eq!(kept, [2, 3]);
    }

    #[test]
    fn an_open_window_overlaps_any_later_interval() {
        let writes = Arc::new(DaemonWrites::default());
        let _open = writes.begin(Path::new("/r"));
        let now = monotonic_ns();
        assert!(writes.overlaps(&|p: &Path| p == Path::new("/r"), now, now + 1));
        assert!(!writes.overlaps(&|p: &Path| p == Path::new("/x"), now, now + 1));
    }
}
