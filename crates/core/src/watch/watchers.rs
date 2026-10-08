//! The OS watchers (ADR-GRP-010 § 1).
//!
//! macOS: one FSEvents stream per watched root, our own (`gitraptor-macsys`, ADR-GRP-010,
//! Enmienda 2026-10-08) instead of `notify`'s, which fixes its flags and latency, takes no
//! exclusion paths and, on every `watch()` or `unwatch()`, recreates the stream "since now" and
//! loses the events of that interval without a rescan mark. With one stream per root, adding or
//! removing a worktree only touches its own stream, and a stream that replaces another starts
//! from the last event id the old one delivered, so changing its exclusions loses nothing.
//!
//! Linux and Windows: `notify`, one shared watcher (a single inotify instance respects
//! `max_user_instances`), with additions and removals grouped through
//! `paths_mut()`. Pendiente: etapa de validación multiplataforma.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(target_os = "macos"))]
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Receives every file event of every watcher.
pub(crate) type Handler = Arc<dyn Fn(notify::Result<notify::Event>) + Send + Sync>;

pub(crate) struct Watchers {
    #[cfg(target_os = "macos")]
    handler: Handler,
    #[cfg(target_os = "macos")]
    per_root: std::collections::HashMap<PathBuf, mac::Entry>,
    #[cfg(target_os = "macos")]
    generation: u64,
    #[cfg(not(target_os = "macos"))]
    shared: Option<RecommendedWatcher>,
    #[cfg(not(target_os = "macos"))]
    watched: std::collections::HashSet<PathBuf>,
    /// Roots watched now, read by `engine.resources` (US-GRP-017).
    roots: Arc<AtomicU64>,
}

impl Watchers {
    pub(crate) fn new(handler: Handler, roots: Arc<AtomicU64>) -> Self {
        Self {
            #[cfg(not(target_os = "macos"))]
            shared: {
                let h = Arc::clone(&handler);
                notify::recommended_watcher(move |r| h(r)).ok()
            },
            #[cfg(not(target_os = "macos"))]
            watched: std::collections::HashSet::new(),
            #[cfg(target_os = "macos")]
            handler,
            #[cfg(target_os = "macos")]
            per_root: std::collections::HashMap::new(),
            #[cfg(target_os = "macos")]
            generation: 0,
            roots,
        }
    }

    fn publish(&self) {
        #[cfg(target_os = "macos")]
        let n = self.per_root.len();
        #[cfg(not(target_os = "macos"))]
        let n = self.watched.len();
        self.roots.store(n as u64, Ordering::Relaxed);
    }

    /// Watches every root recursively. `false` if any could not be watched.
    #[cfg(target_os = "macos")]
    pub(crate) fn add(&mut self, roots: &[PathBuf]) -> bool {
        use gitraptor_macsys::fsevents::{SINCE_NOW, Stream};

        let mut ok = true;
        for root in roots {
            if self.per_root.contains_key(root) {
                continue;
            }
            let handler = mac::adapter(Arc::clone(&self.handler), None);
            match Stream::start(root, &[], SINCE_NOW, handler) {
                Ok(stream) => {
                    self.generation += 1;
                    self.per_root.insert(
                        root.clone(),
                        mac::Entry {
                            stream: Arc::new(stream),
                            requested: Vec::new(),
                            generation: self.generation,
                        },
                    );
                }
                Err(_) => ok = false,
            }
        }
        self.publish();
        ok
    }

    /// Stops the streams of `roots`. They are returned, not dropped: dropping one waits for its
    /// callback, which may need the lock this runs under, so the caller drops them outside it.
    #[cfg(target_os = "macos")]
    #[must_use]
    pub(crate) fn remove(
        &mut self,
        roots: &[PathBuf],
    ) -> Vec<Arc<gitraptor_macsys::fsevents::Stream>> {
        let removed = roots
            .iter()
            .filter_map(|root| self.per_root.remove(root))
            .map(|e| e.stream)
            .collect();
        self.publish();
        removed
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn add(&mut self, roots: &[PathBuf]) -> bool {
        let Some(watcher) = self.shared.as_mut() else {
            return false;
        };
        let mut paths = watcher.paths_mut();
        let mut ok = true;
        let mut added = Vec::new();
        for root in roots {
            // A woken repo's roots were never unwatched (N1).
            if self.watched.contains(root) {
                continue;
            }
            if paths.add(root, RecursiveMode::Recursive).is_ok() {
                added.push(root.clone());
            } else {
                ok = false;
            }
        }
        let committed = paths.commit().is_ok();
        if committed {
            self.watched.extend(added);
            self.publish();
        }
        ok && committed
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn remove(&mut self, roots: &[PathBuf]) {
        let Some(watcher) = self.shared.as_mut() else {
            return;
        };
        let mut paths = watcher.paths_mut();
        for root in roots {
            let _ = paths.remove(root);
        }
        let _ = paths.commit();
        for root in roots {
            self.watched.remove(root);
        }
        self.publish();
    }

    /// The folders `root`'s stream was asked to leave out.
    #[cfg(target_os = "macos")]
    pub(crate) fn requested(&self, root: &std::path::Path) -> Option<Vec<PathBuf>> {
        self.per_root.get(root).map(|e| e.requested.clone())
    }

    /// What a replacement of `root`'s stream starts from.
    #[cfg(target_os = "macos")]
    pub(crate) fn ticket(&self, root: &std::path::Path) -> Option<mac::Ticket> {
        let entry = self.per_root.get(root)?;
        Some(mac::Ticket {
            root: root.to_path_buf(),
            old: Arc::clone(&entry.stream),
            generation: entry.generation,
            handler: Arc::clone(&self.handler),
        })
    }

    /// Puts `new` in place of the stream `ticket` was taken from. If the root was removed or
    /// replaced meanwhile, `new` is handed back instead. Either way the stream that leaves is
    /// returned, for the caller to drop outside the lock.
    #[cfg(target_os = "macos")]
    pub(crate) fn commit(
        &mut self,
        ticket: &mac::Ticket,
        new: gitraptor_macsys::fsevents::Stream,
        requested: Vec<PathBuf>,
    ) -> Arc<gitraptor_macsys::fsevents::Stream> {
        let new = Arc::new(new);
        match self.per_root.get_mut(&ticket.root) {
            Some(entry) if entry.generation == ticket.generation => {
                self.generation += 1;
                entry.generation = self.generation;
                entry.requested = requested;
                std::mem::replace(&mut entry.stream, new)
            }
            _ => new,
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod mac {
    //! The FSEvents backend: streams per root, and how one replaces another.

    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::mpsc::{Sender, channel};
    use std::time::Duration;

    use gitraptor_macsys::fsevents::{self, SINCE_NOW, Stream, StreamError, flag};
    use notify::EventKind;
    use notify::event::{Flag, ModifyKind};

    use super::Handler;

    /// How long a replacement stream may take to replay what happened since the old one's last
    /// event (ADR-GRP-010, Enmienda 2026-10-08, E4). ⚠️ **ASSUMPTION**.
    const HISTORY_WAIT: Duration = Duration::from_secs(2);

    pub(super) struct Entry {
        pub stream: Arc<Stream>,
        /// What the exclusion manager asked for (non-canonical, as it knows the folders).
        pub requested: Vec<PathBuf>,
        pub generation: u64,
    }

    /// The old stream of a root and what a replacement needs.
    pub(crate) struct Ticket {
        pub root: PathBuf,
        pub old: Arc<Stream>,
        pub generation: u64,
        pub handler: Handler,
    }

    /// Turns the batches of a stream into the events the router takes: one `Modify` with every
    /// path of the batch, and a rescan mark when the OS says it lost events. `history_done` is
    /// signalled when a replayed history ends.
    pub(crate) fn adapter(handler: Handler, history_done: Option<Sender<()>>) -> fsevents::Handler {
        Box::new(move |events| {
            let mut paths = Vec::with_capacity(events.len());
            let mut loss = false;
            for e in events {
                if e.flags & flag::LOSS != 0 {
                    loss = true;
                } else if e.flags & flag::HISTORY_DONE != 0 {
                    if let Some(tx) = &history_done {
                        let _ = tx.send(());
                    }
                } else {
                    paths.push(e.path.to_path_buf());
                }
            }
            if loss {
                handler(Ok(
                    notify::Event::new(EventKind::Other).set_flag(Flag::Rescan)
                ));
            }
            if !paths.is_empty() {
                let mut event = notify::Event::new(EventKind::Modify(ModifyKind::Any));
                event.paths = paths;
                handler(Ok(event));
            }
        })
    }

    pub(crate) struct Rebuilt {
        pub stream: Stream,
        /// Its history was replayed in time.
        pub complete: bool,
        /// The OS rejected the exclusions: the stream runs without them.
        pub rejected: bool,
        /// The folders of `wanted` the stream really leaves out: one that no longer exists, or
        /// is a symlink, is not.
        pub applied: Vec<PathBuf>,
    }

    /// Starts the stream that takes over from `ticket.old`, leaving out `wanted`, and waits for
    /// it to catch up. The old stream keeps running meanwhile: the events of the overlap come
    /// twice, which is harmless. Returns the new stream, and whether its history was replayed
    /// in time (if not, the caller reconciles the whole worktree). If the OS rejects the
    /// exclusions, the stream starts without them: the router still drops what is ignored.
    /// Takes no lock: it waits for the old stream's callbacks.
    pub(crate) fn rebuild(ticket: &Ticket, wanted: &[PathBuf]) -> Option<Rebuilt> {
        // Taken before the flush: whatever the OS records from here on, the new stream replays
        // and the old one may or may not have delivered (duplicates are harmless).
        let from = match fsevents::current_event_id() {
            0 => SINCE_NOW,
            id => id,
        };
        ticket.old.flush_sync();
        let (tx, rx) = channel();
        let start = |excluded: &[PathBuf]| {
            Stream::start(
                &ticket.root,
                excluded,
                from,
                adapter(Arc::clone(&ticket.handler), Some(tx.clone())),
            )
        };
        let (stream, rejected) = match start(wanted) {
            Ok(s) => (s, false),
            Err(StreamError::Exclusions | StreamError::TooManyExclusions) => {
                (start(&[]).ok()?, true)
            }
            Err(_) => return None,
        };
        let complete = from == SINCE_NOW || rx.recv_timeout(HISTORY_WAIT).is_ok();
        let kept = stream.exclusions();
        let applied = wanted
            .iter()
            .filter(|w| w.canonicalize().is_ok_and(|c| kept.contains(&c)))
            .cloned()
            .collect();
        Some(Rebuilt {
            stream,
            complete,
            rejected,
            applied,
        })
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use gitraptor_macsys::fsevents::{Event, flag};

    use super::*;

    fn run(events: &[Event<'_>]) -> Vec<notify::Event> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let adapt = mac::adapter(
            Arc::new(move |r| sink.lock().unwrap().push(r.unwrap())),
            None,
        );
        adapt(events);
        let out = seen.lock().unwrap();
        out.clone()
    }

    fn ev(path: &str, flags: u32) -> Event<'_> {
        Event {
            path: Path::new(path),
            flags,
            id: 1,
        }
    }

    #[test]
    fn a_batch_is_one_event_with_every_path() {
        let out = run(&[
            ev("/w/a", flag::ITEM_CREATED),
            ev("/w/b", flag::ITEM_MODIFIED),
        ]);
        assert_eq!(out.len(), 1);
        assert!(!out[0].need_rescan());
        assert_eq!(out[0].paths, [PathBuf::from("/w/a"), PathBuf::from("/w/b")]);
    }

    #[test]
    fn every_loss_mark_is_a_rescan() {
        for mark in [
            flag::MUST_SCAN_SUBDIRS,
            flag::USER_DROPPED,
            flag::KERNEL_DROPPED,
            flag::EVENT_IDS_WRAPPED,
            flag::ROOT_CHANGED,
            flag::MOUNT,
            flag::UNMOUNT,
        ] {
            let out = run(&[ev("/w", mark)]);
            assert!(out.iter().any(notify::Event::need_rescan), "{mark:#x}");
        }
    }

    #[test]
    fn the_end_of_a_replayed_history_is_signalled_and_is_no_change() {
        let (tx, rx) = std::sync::mpsc::channel();
        let seen = Arc::new(Mutex::new(0));
        let sink = Arc::clone(&seen);
        let adapt = mac::adapter(Arc::new(move |_| *sink.lock().unwrap() += 1), Some(tx));
        adapt(&[ev("", flag::HISTORY_DONE)]);
        assert!(rx.try_recv().is_ok());
        assert_eq!(*seen.lock().unwrap(), 0);
    }
}
