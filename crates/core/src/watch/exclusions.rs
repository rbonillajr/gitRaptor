//! Which ignored folders leave a root's FSEvents stream (ADR-GRP-010, Enmienda 2026-10-08).
//!
//! A build writes thousands of files per second under an ignored `target/`. The router already
//! drops them, but the stream still calls the daemon for each one. A folder that sustains churn
//! (`worktree::HOT_EVENTS` dropped events in a window) is left out of the root's stream, up to
//! the 8 FSEvents allows, by starting a replacement stream from the old one's last event id: the
//! overlap is harmless and nothing is lost. A folder that stops being ignored, or that now holds
//! a tracked file, is taken back at once, and the worktree is reconciled, since what happened in
//! it while it was left out was never delivered.
//!
//! One thread, asleep until a worktree says something changed: no polling.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use gitraptor_api::clock;
use gitraptor_macsys::fsevents::MAX_EXCLUSIONS;

use super::watchers::mac;
use super::worktree::IgnoredPrefixes;
use super::{Shared, WtMsg};

/// Fewest time between two recreations of a root's stream to leave folders out. Taking them back
/// is never delayed. ⚠️ **ASSUMPTION**: calibrated by the idle bench.
const MIN_BETWEEN_ADDITIONS: Duration = Duration::from_secs(30);

/// When to try again after the OS refused a replacement stream.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// A worktree says its ignored folders changed.
pub(super) struct Recheck {
    pub root: PathBuf,
    pub ignored: Weak<IgnoredPrefixes>,
    pub tx: Sender<WtMsg>,
}

pub(super) fn start(shared: Weak<Shared>, rx: Receiver<Recheck>) {
    let _ = std::thread::Builder::new()
        .name("raptor-watch-exclusions".into())
        .spawn(move || run(&shared, &rx));
}

struct Pending {
    ignored: Weak<IgnoredPrefixes>,
    tx: Sender<WtMsg>,
}

fn run(shared: &Weak<Shared>, rx: &Receiver<Recheck>) {
    let mut last_addition: HashMap<PathBuf, Instant> = HashMap::new();
    // Roots with folders that were due but for the pace, and when they may go.
    let mut deferred: HashMap<PathBuf, (Pending, Instant)> = HashMap::new();
    loop {
        let wait = deferred.values().map(|(_, due)| *due).min();
        let msg = match wait {
            Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let mut due: Vec<(PathBuf, Pending)> = Vec::new();
        match msg {
            Ok(m) => due.push((
                m.root,
                Pending {
                    ignored: m.ignored,
                    tx: m.tx,
                },
            )),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let ready: Vec<PathBuf> = deferred
            .iter()
            .filter(|(_, (_, at))| *at <= now)
            .map(|(root, _)| root.clone())
            .collect();
        for root in ready {
            if let Some((pending, _)) = deferred.remove(&root) {
                due.push((root, pending));
            }
        }
        let Some(shared) = shared.upgrade() else {
            return;
        };
        for (root, pending) in due {
            // A message for a root that is already deferred replaces it: it is newer.
            deferred.remove(&root);
            if let Some(again) = apply(&shared, &root, &pending, &mut last_addition) {
                deferred.insert(root, (pending, again));
            }
        }
    }
}

/// Brings `root`'s excluded folders in line with what its worktree found. `Some(when)` if there
/// are hot folders to add that the pace left for later.
fn apply(
    shared: &Shared,
    root: &Path,
    pending: &Pending,
    last_addition: &mut HashMap<PathBuf, Instant>,
) -> Option<Instant> {
    let ignored = pending.ignored.upgrade()?;
    let current = shared.exclusions_of(root)?;
    let snapshot = ignored.snapshot();
    // Leaving: a folder the worktree no longer calls ignored (E5).
    let mut wanted: Vec<PathBuf> = current
        .iter()
        .filter(|c| snapshot.iter().any(|s| &s.dir == *c))
        .cloned()
        .collect();
    let removed = wanted.len() != current.len();
    let mut candidates: Vec<_> = snapshot
        .iter()
        .filter(|s| s.hot && !wanted.contains(&s.dir))
        .collect();
    candidates.sort_by_key(|s| std::cmp::Reverse(s.hits));
    let room = MAX_EXCLUSIONS.saturating_sub(wanted.len());
    let paced = last_addition
        .get(root)
        .map(|t| t.elapsed())
        .filter(|e| *e < MIN_BETWEEN_ADDITIONS);
    // Taking folders back never waits; adding them rides on that recreation or waits its turn.
    let add = !candidates.is_empty() && room > 0 && (paced.is_none() || removed);
    if add {
        wanted.extend(candidates.iter().take(room).map(|s| s.dir.clone()));
    }
    let again = (!candidates.is_empty() && room > 0 && !add)
        .then(|| Instant::now() + MIN_BETWEEN_ADDITIONS.saturating_sub(paced.unwrap_or_default()));
    if wanted == current {
        return again;
    }
    let Some(done) = replace(shared, root, &wanted) else {
        // The OS refused the new stream. A folder taken back must not stay out: reconcile what
        // was missed and try again soon.
        if removed {
            let _ = pending.tx.send(WtMsg::Rescan(clock::monotonic_ns()));
            return Some(Instant::now() + RETRY_AFTER);
        }
        return again;
    };
    if add {
        last_addition.insert(root.to_path_buf(), Instant::now());
    }
    // What happened in a folder while it was left out was never delivered. A history that did
    // not replay in time, or an OS that kept the folders in, is as unknown.
    if removed || !done.complete {
        let _ = pending.tx.send(WtMsg::Rescan(clock::monotonic_ns()));
    }
    again
}

struct Replaced {
    complete: bool,
}

/// Replaces `root`'s stream by one that leaves out `wanted`, without holding the watchers lock
/// while it waits for either stream.
fn replace(shared: &Shared, root: &Path, wanted: &[PathBuf]) -> Option<Replaced> {
    let ticket = shared.stream_ticket(root)?;
    let rebuilt = mac::rebuild(&ticket, wanted)?;
    let requested = if rebuilt.excluded {
        wanted.to_vec()
    } else {
        Vec::new()
    };
    let complete = rebuilt.complete && (rebuilt.excluded || wanted.is_empty());
    // The stream that leaves is dropped here, outside the lock.
    let _leaving = shared.commit_stream(&ticket, rebuilt.stream, requested);
    Some(Replaced { complete })
}

/// The notifier a worktree installs on its [`IgnoredPrefixes`].
pub(super) fn notifier(
    tx: Sender<Recheck>,
    root: PathBuf,
    ignored: &Arc<IgnoredPrefixes>,
    wt_tx: Sender<WtMsg>,
) -> Box<dyn Fn() + Send + Sync> {
    let ignored = Arc::downgrade(ignored);
    Box::new(move || {
        let _ = tx.send(Recheck {
            root: root.clone(),
            ignored: ignored.clone(),
            tx: wt_tx.clone(),
        });
    })
}
