//! The OS watchers behind `notify` (ADR-GRP-010 § 1).
//!
//! macOS: one FSEvents stream per watched root. With `notify` 8.2 every
//! `watch()` or `unwatch()` recreates the stream "since now" and loses the
//! events of that interval without a rescan mark; with one stream per root,
//! adding or removing a worktree only touches its own stream, and the
//! measurement of the US-GRP-002 Dev Spec (§ 1) lost no event in the others.
//!
//! Linux and Windows: one shared watcher (a single inotify instance respects
//! `max_user_instances`), with additions and removals grouped through
//! `paths_mut()`. Pendiente: etapa de validación multiplataforma.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Receives every file event of every watcher.
pub(crate) type Handler = Arc<dyn Fn(notify::Result<notify::Event>) + Send + Sync>;

pub(crate) struct Watchers {
    handler: Handler,
    #[cfg(target_os = "macos")]
    per_root: std::collections::HashMap<PathBuf, RecommendedWatcher>,
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
            handler,
            #[cfg(target_os = "macos")]
            per_root: std::collections::HashMap::new(),
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
        let mut ok = true;
        for root in roots {
            if self.per_root.contains_key(root) {
                continue;
            }
            let h = Arc::clone(&self.handler);
            let watcher = notify::recommended_watcher(move |r| h(r)).and_then(|mut w| {
                w.watch(root, RecursiveMode::Recursive)?;
                Ok(w)
            });
            match watcher {
                Ok(w) => {
                    self.per_root.insert(root.clone(), w);
                }
                Err(_) => ok = false,
            }
        }
        self.publish();
        ok
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn remove(&mut self, roots: &[PathBuf]) {
        for root in roots {
            // Dropping the watcher stops its stream only.
            self.per_root.remove(root);
        }
        self.publish();
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn add(&mut self, roots: &[PathBuf]) -> bool {
        let _ = &self.handler;
        let Some(watcher) = self.shared.as_mut() else {
            return false;
        };
        let mut paths = watcher.paths_mut();
        let mut ok = true;
        let mut added = Vec::new();
        for root in roots {
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
}
