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
    /// The last mark given to a window: marks are strictly increasing, so two windows of the
    /// same ref are ordered exactly even when the clock does not advance between them (macOS
    /// reads `CLOCK_MONOTONIC` in microseconds).
    last_mark: AtomicU64,
}

#[derive(Debug, Clone)]
struct Span {
    id: u64,
    path: PathBuf,
    start_ns: u64,
    /// `None` while the guard lives.
    end_ns: Option<u64>,
    /// Opened by the applier, which records what it wrote; the executor's window knows nothing.
    tracked: bool,
    /// `(refname, value)` the window left, in the order written. Only oids and ref names of the
    /// repo, in memory (SEC-04).
    wrote: Vec<(String, String)>,
    /// `(refname, value)` the cleared moves of its margin left after it, when it is the latest
    /// window that wrote the ref: the ref's current value as far as the margin knows.
    chain: Vec<(String, String)>,
    /// Every write of the window returned successfully ([`WriteWindow::complete`]).
    completed: bool,
}

impl Span {
    /// The value the margin expects `refname` to start from: the last cleared move after this
    /// window, or what the window left.
    fn current(&self, refname: &str) -> Option<&str> {
        self.chain
            .iter()
            .find(|(r, _)| r == refname)
            .map(|(_, v)| v.as_str())
            .or_else(|| self.left(refname))
    }

    /// A cleared move left `refname` at `value`.
    fn advance(&mut self, refname: &str, value: &str) {
        match self.chain.iter_mut().find(|(r, _)| r == refname) {
            Some((_, v)) => value.clone_into(v),
            None => self.chain.push((refname.to_owned(), value.to_owned())),
        }
    }

    /// The last value the window left in `refname`, if it wrote it.
    fn left(&self, refname: &str) -> Option<&str> {
        self.wrote
            .iter()
            .rev()
            .find(|(r, _)| r == refname)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the two windows were open at a common moment, or closed at the same mark: then
    /// the order of their writes is unknown.
    fn concurrent_with(&self, other: &Span) -> bool {
        match (self.end_ns, other.end_ns) {
            (Some(a), Some(b)) => a == b || (self.start_ns < b && other.start_ns < a),
            _ => true,
        }
    }
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
        if let Some(span) = self.writes.spans().iter_mut().find(|s| s.id == self.id) {
            span.wrote.push((refname.to_owned(), value.to_owned()));
        }
    }

    /// Marks that every write of the window returned successfully. A window dropped without it
    /// (an error, a refusal, a panic) is a failed one: its final state is unknown, so it never
    /// clears an event (ADR-GRP-012, amendment 2026-11-09).
    pub fn complete(&self) {
        if let Some(span) = self.writes.spans().iter_mut().find(|s| s.id == self.id) {
            span.completed = true;
        }
    }
}

impl Drop for WriteWindow {
    fn drop(&mut self) {
        let mut spans = self.writes.spans();
        let now = self.writes.mark();
        if let Some(span) = spans.iter_mut().find(|s| s.id == self.id) {
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

    /// The clock, never equal to or before the previous mark. Taken with the list locked, so the
    /// order of the marks is the order of the opens and closes.
    fn mark(&self) -> u64 {
        // Only ever called with the list locked: no other mark can interleave.
        let mark = monotonic_ns().max(self.last_mark.load(Ordering::Relaxed).saturating_add(1));
        self.last_mark.store(mark, Ordering::Relaxed);
        mark
    }

    /// Opens a window over the repo or worktree at `path` until the guard drops. The path is
    /// kept canonical when it can be resolved, as given otherwise; windows closed longer than
    /// the retention ago are dropped here.
    pub fn begin(self: &Arc<Self>, path: &Path) -> WriteWindow {
        self.open(path, false)
    }

    fn open(self: &Arc<Self>, path: &Path, tracked: bool) -> WriteWindow {
        let path = crate::observe::canonical(path);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut spans = self.spans();
        let start_ns = self.mark();
        prune(&mut spans, start_ns);
        spans.push(Span {
            id,
            path,
            start_ns,
            end_ns: None,
            tracked,
            wrote: Vec::new(),
            chain: Vec::new(),
            completed: false,
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
        self.open(path, true)
    }

    /// Whether the daemon's windows make an event of the repo foreign (ADR-GRP-012, amendment
    /// 2026-11-09). Among the windows over a path `in_repo` accepts:
    ///
    /// - one still open when the event is judged makes it foreign;
    /// - the closed windows that overlap `[from_ns, to_ns]` or closed less than [`WINDOW_TAIL`]
    ///   before `from_ns` make it foreign unless `moved` clears them all: every one of them is
    ///   the applier's and completed ([`WriteWindow::complete`]), the move's old value is known,
    ///   and it is the last value the latest window that wrote the ref (by `end_ns`) left there,
    ///   or the chain after it. Two windows that wrote the ref concurrently, or closed at the
    ///   same mark, leave the order unknown: foreign. A ref none of them wrote is not checked.
    ///
    /// Before all that, a move whose new value any kept applier window (closed less than the
    /// retention ago) wrote, on any ref, is a late echo: foreign, in the margin or past it.
    ///
    /// A move that clears the margin advances its chain: the next move of the ref may start from
    /// its new value. So this call changes state, and is made once per event.
    pub fn covers(
        &self,
        in_repo: &dyn Fn(&Path) -> bool,
        from_ns: u64,
        to_ns: u64,
        moved: Option<MoveSeen<'_>>,
    ) -> bool {
        let tail = ns(WINDOW_TAIL);
        // One lock for the whole judgement: the chain is read and advanced in the same step, so
        // two evaluations never advance from the same value.
        let mut spans = self.spans();
        let mut margin: Vec<usize> = Vec::new();
        for (i, s) in spans.iter().enumerate() {
            if !in_repo(&s.path) {
                continue;
            }
            // Still open while the event is judged: its final state is not known yet, whenever
            // it started (the batch's refs are read after `to_ns`, so a window that opened
            // later may still be inside the event).
            if s.end_ns.is_none() {
                return true;
            }
            if s.start_ns > to_ns {
                continue;
            }
            match s.end_ns {
                None => return true,
                // Overlapping the S3 window or in its tail: judged by what it wrote, only if it is
                // the applier's and every write of it succeeded.
                Some(end) if end.saturating_add(tail) >= from_ns => {
                    if !s.tracked || !s.completed {
                        return true;
                    }
                    margin.push(i);
                }
                Some(_) => {}
            }
        }
        // A late echo by value: a move to any value a kept applier window wrote, on any ref, also
        // past the margin and whatever its start (the refs are read after `to_ns`), may be the
        // applier's own write seen late.
        if let Some(m) = moved
            && spans.iter().any(|s| {
                s.tracked
                    && s.end_ns
                        .is_some_and(|e| e.saturating_add(ns(RETENTION)) >= from_ns)
                    && in_repo(&s.path)
                    && s.wrote.iter().any(|(_, v)| v == m.new)
            })
        {
            return true;
        }
        if margin.is_empty() {
            return false;
        }
        let Some(m) = moved else {
            return true;
        };
        let Some(old) = m.old else {
            return true;
        };
        let writers: Vec<usize> = margin
            .into_iter()
            .filter(|&i| spans[i].left(m.refname).is_some())
            .collect();
        for (k, &a) in writers.iter().enumerate() {
            if writers[k + 1..]
                .iter()
                .any(|&b| spans[a].concurrent_with(&spans[b]))
            {
                return true;
            }
        }
        // The latest window that wrote the ref decides; a newer one restarts the chain from its
        // own value, and the chain ends with its window's margin.
        let Some(latest) = writers.into_iter().max_by_key(|&i| spans[i].end_ns) else {
            return false;
        };
        if spans[latest].current(m.refname) != Some(old) {
            return true;
        }
        spans[latest].advance(m.refname, m.new);
        false
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
            tracked: false,
            wrote: Vec::new(),
            chain: Vec::new(),
            completed: false,
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
