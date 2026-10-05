//! The task of one worktree (ADR-GRP-010 § 3 to § 6).
//!
//! Fixed debounce window: the first event opens it, and when it closes the
//! worktree is read again with every accumulated path; events that arrive
//! during the recompute open the next one, so a continuous burst publishes
//! once per window instead of waiting for the burst to end. The periodic
//! reconciliation runs here too, after the window is flushed, so events in
//! flight never look like gaps.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::clock;
use gitraptor_api::messages::{GitEventDetails, GitEventKind, HeadView, WorktreeStatus};
use gitraptor_git::{ReaderOptions, RepoReader};

use super::{GapMark, Marks, ObservedBatch, RawEvent, Shared, WtMsg, wall_now};
use crate::observe::{self, WorktreeRead};
use crate::profile::GapCause;

struct Window {
    deadline: Instant,
    t_recv: u64,
    /// The watcher lost events: the flush is a reconciliation with a gap.
    overflow: bool,
}

struct Task {
    shared: Arc<Shared>,
    repo_id: String,
    root: PathBuf,
    main: bool,
    admin: Option<String>,
    last: WorktreeRead,
    /// Branch of `HEAD` outside any operation in progress.
    stable_branch: Option<String>,
    window: Option<Window>,
    /// Wall time of the last event received: where an overflow gap starts.
    last_event_ms: i64,
    overflow_from_ms: Option<i64>,
    ignore: IgnoreCache,
    next_periodic: Instant,
    last_periodic_ms: i64,
    degraded: bool,
    next_degraded: Instant,
}

pub(super) fn run(
    shared: Arc<Shared>,
    repo_id: String,
    initial: WorktreeRead,
    degraded: bool,
    rx: Receiver<WtMsg>,
) {
    let config = shared.config;
    // The task compares reads without the ahead/behind, which the daemon
    // counts for the whole repo (US-GRP-012).
    let mut initial = initial;
    observe::set_divergence(
        &mut initial.view,
        gitraptor_api::messages::DivergenceView::Unreadable,
    );
    let root = PathBuf::from(initial.view.path.raw());
    let now = Instant::now();
    // Worktrees are staggered over the period so they do not all
    // reconcile at once.
    let stagger = stagger(&root, config.periodic);
    let mut task = Task {
        repo_id,
        main: initial.view.main,
        admin: initial.view.admin_name.as_ref().map(|n| n.raw().to_owned()),
        stable_branch: if initial.in_progress {
            None
        } else {
            branch_of(&initial)
        },
        last: initial,
        window: None,
        last_event_ms: wall_now().0,
        overflow_from_ms: None,
        ignore: IgnoreCache::default(),
        next_periodic: now + config.periodic.min(stagger + config.periodic / 2),
        last_periodic_ms: wall_now().0,
        degraded,
        next_degraded: now + config.degraded_poll,
        shared,
        root,
    };
    // The watch is running: read once more, so a change written between
    // the reconciliation that started the task and the watch is not lost.
    task.open_window(clock::monotonic_ns(), Duration::ZERO);
    loop {
        let deadline = task.next_deadline();
        let wait = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(WtMsg::Paths(t_recv, paths)) => {
                if !task.ignore.keep_any(&task.root, paths) {
                    continue;
                }
                task.last_event_ms = wall_now().0;
                task.open_window(t_recv, task.window_len());
            }
            Ok(WtMsg::Rescan(t_recv)) => {
                task.overflow_from_ms.get_or_insert(task.last_event_ms);
                task.open_window(t_recv, task.window_len());
                if let Some(w) = task.window.as_mut() {
                    w.overflow = true;
                }
            }
            Ok(WtMsg::Reconcile) => task.open_window(clock::monotonic_ns(), Duration::ZERO),
            Ok(WtMsg::IgnoreRules) => task.ignore.clear(),
            Ok(WtMsg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if task.window.as_ref().is_some_and(|w| w.deadline <= now) {
            let window = task.window.take();
            if let Some(window) = window {
                task.flush(window);
            }
        } else if task.window.is_none() && task.next_periodic <= now {
            task.periodic();
        }
        if task.degraded && task.next_degraded <= Instant::now() {
            task.next_degraded = Instant::now() + task.shared.config.degraded_poll;
            if task.window.is_none() {
                task.open_window(clock::monotonic_ns(), Duration::ZERO);
            }
        }
    }
}

impl Task {
    fn window_len(&self) -> Duration {
        let c = self.shared.config;
        c.window.saturating_sub(c.timer_slack)
    }

    fn open_window(&mut self, t_recv: u64, len: Duration) {
        if self.window.is_none() {
            self.window = Some(Window {
                deadline: Instant::now() + len,
                t_recv,
                overflow: false,
            });
        }
    }

    fn next_deadline(&self) -> Instant {
        let mut next = match &self.window {
            Some(w) => w.deadline,
            None => self.next_periodic,
        };
        if self.degraded {
            next = next.min(self.next_degraded);
        }
        next
    }

    fn read(&self) -> WorktreeRead {
        self.shared
            .recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        observe::read_worktree(&self.root, self.main, self.admin.as_deref())
    }

    fn flush(&mut self, window: Window) {
        let t_flush = clock::monotonic_ns();
        let read = self.read();
        let t_computed = clock::monotonic_ns();
        let (now_ms, offset_s) = wall_now();
        let mut events = Vec::new();
        if let Some(switch) = self.branch_switch(&read) {
            events.push(RawEvent {
                worktree: self.root.clone(),
                kind: GitEventKind::BranchSwitch,
                details: switch,
                observed_ms: now_ms,
                offset_s,
            });
        }
        let changed = read != self.last;
        // An overflow always opens a gap, from the last event received
        // until this reconciliation ends (ADR-GRP-013 § 5).
        let gap = if window.overflow {
            let started_ms = self.overflow_from_ms.take().unwrap_or(now_ms);
            if changed {
                events.push(self.reconciled(now_ms, offset_s));
            }
            Some(GapMark {
                cause: GapCause::WatcherOverflow,
                started_ms,
                ended_ms: now_ms,
            })
        } else {
            None
        };
        if changed || !events.is_empty() || gap.is_some() {
            self.send(
                read.clone(),
                changed,
                events,
                gap,
                Marks {
                    t_recv: window.t_recv,
                    t_flush,
                    t_computed,
                },
            );
        }
        self.last = read;
    }

    /// Full reconciliation every period (ADR-GRP-010 § 5): what it finds
    /// was missed without a mark, so it is published in a gap of its own.
    /// Not in degraded mode, which already reads the full state.
    fn periodic(&mut self) {
        let period = self.shared.config.periodic;
        self.next_periodic = Instant::now() + period;
        let (now_ms, offset_s) = wall_now();
        let from_ms = std::mem::replace(&mut self.last_periodic_ms, now_ms);
        if self.degraded {
            return;
        }
        let t_flush = clock::monotonic_ns();
        let read = self.read();
        let t_computed = clock::monotonic_ns();
        if read == self.last {
            return;
        }
        if !read.in_progress {
            self.stable_branch = branch_of(&read).or(self.stable_branch.take());
        }
        let event = self.reconciled(now_ms, offset_s);
        self.send(
            read.clone(),
            true,
            vec![event],
            Some(GapMark {
                cause: GapCause::PeriodicReconciliation,
                started_ms: from_ms,
                ended_ms: now_ms,
            }),
            Marks {
                t_recv: t_flush,
                t_flush,
                t_computed,
            },
        );
        self.last = read;
    }

    fn reconciled(&self, observed_ms: i64, offset_s: i32) -> RawEvent {
        RawEvent {
            worktree: self.root.clone(),
            kind: GitEventKind::Reconciled,
            details: GitEventDetails::default(),
            observed_ms,
            offset_s,
        }
    }

    /// `HEAD` moved to another branch, outside any operation in progress: a
    /// rebase detaches `HEAD` without the developer switching branch.
    fn branch_switch(&mut self, read: &WorktreeRead) -> Option<GitEventDetails> {
        if read.in_progress || !matches!(read.view.status, WorktreeStatus::Ready { .. }) {
            return None;
        }
        let current = branch_of(read);
        if current == self.stable_branch {
            return None;
        }
        let from = std::mem::replace(&mut self.stable_branch, current.clone());
        let to = current?;
        Some(GitEventDetails {
            branch: Some(Untrusted::new(to)),
            from: from.map(Untrusted::new),
            old_commit: self.last.head_commit.clone(),
            new_commit: read.head_commit.clone(),
            worktree_inferred: false,
        })
    }

    fn send(
        &self,
        read: WorktreeRead,
        changed: bool,
        events: Vec<RawEvent>,
        gap: Option<GapMark>,
        marks: Marks,
    ) {
        self.shared.send(ObservedBatch {
            repo_id: self.repo_id.clone(),
            worktrees: if changed { vec![read] } else { Vec::new() },
            gone: Vec::new(),
            events,
            gap,
            refs: None,
            marks,
        });
    }
}

/// The branch `HEAD` names, with or without commits.
pub(super) fn branch_of(read: &WorktreeRead) -> Option<String> {
    match &read.view.status {
        WorktreeStatus::Ready {
            head: HeadView::Branch { name } | HeadView::Unborn { name },
            ..
        } => Some(name.raw().to_owned()),
        _ => None,
    }
}

fn stagger(root: &Path, period: Duration) -> Duration {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut h);
    let millis = u64::try_from(period.as_millis()).unwrap_or(u64::MAX).max(1);
    Duration::from_millis(h.finish() % millis)
}

/// Directories ignored by Git, asked once each (ADR-GRP-010 § 2, Enmienda
/// 2026-10-05): events under them are dropped before the debounce, so a
/// build writing to an ignored `target/` costs no recompute.
#[derive(Default)]
struct IgnoreCache {
    ignored: HashSet<String>,
    kept: HashSet<String>,
}

impl IgnoreCache {
    fn clear(&mut self) {
        self.ignored.clear();
        self.kept.clear();
    }

    /// Whether any path is outside every ignored directory.
    fn keep_any(&mut self, root: &Path, paths: Vec<PathBuf>) -> bool {
        if paths
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == ".gitignore"))
        {
            self.clear();
            return true;
        }
        let mut reader: Option<Option<RepoReader>> = None;
        let mut keep = false;
        for path in paths {
            let Ok(rel) = path.strip_prefix(root) else {
                keep = true;
                continue;
            };
            if self.is_ignored(root, rel, &mut reader) {
                continue;
            }
            keep = true;
            break;
        }
        keep
    }

    fn is_ignored(
        &mut self,
        root: &Path,
        rel: &Path,
        reader: &mut Option<Option<RepoReader>>,
    ) -> bool {
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts.first().is_some_and(|c| c == ".git") {
            return false;
        }
        // Every ancestor directory, top-down; the path itself may be a file.
        let mut dir = String::new();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if !dir.is_empty() {
                dir.push('/');
            }
            dir.push_str(part);
            if self.ignored.contains(&dir) {
                return true;
            }
            if self.kept.contains(&dir) {
                continue;
            }
            let reader = reader
                .get_or_insert_with(|| RepoReader::open(root, &ReaderOptions::default()).ok());
            let Some(reader) = reader.as_ref() else {
                return false;
            };
            if reader.is_ignored(&dir, true).unwrap_or(false) {
                self.ignored.insert(dir);
                return true;
            }
            self.kept.insert(dir.clone());
        }
        false
    }
}
