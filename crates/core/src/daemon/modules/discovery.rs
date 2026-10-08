//! Watching and listing the discovery roots (US-GRP-020; ADR-GRP-010,
//! Enmienda 2026-10-07, N6) as a module of the daemon.
//!
//! One thread for every root. A normal root is watched without recursion
//! where the OS allows it (inotify, ReadDirectoryChangesW) and listed
//! `settle` after a change; on macOS it is listed every `poll`, because the
//! `notify` 8.2 backend there (FSEvents) watches the whole subtree. A broad
//! root is never watched: it is listed every `broad_poll` (RES-03). Every
//! root is listed again every `safety`. Each listing goes to the loop, which
//! filters and persists it: this thread never touches the profile.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{Daemon, DaemonModule};
use crate::daemon::discovery::DiscoveryConfig;
use crate::daemon::{Logger, ShutdownHandle};
use crate::discovery::{BROAD_ENTRIES, list_first_level};
use crate::profile::DiscoveryRoot;

enum Msg {
    Roots {
        roots: Vec<DiscoveryRoot>,
        home: Option<PathBuf>,
    },
    /// Something changed in the first level of this root.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Changed(PathBuf),
    Stop,
}

struct Discovery {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

pub(super) fn start(daemon: &Daemon) -> Option<Box<dyn DaemonModule>> {
    let (tx, rx) = channel();
    let worker = Worker {
        config: daemon.config.discovery.clone(),
        handle: daemon.handle.clone(),
        logger: daemon.logger.clone(),
        roots: Vec::new(),
        home: None,
        watcher: Watcher::new(tx.clone()),
    };
    let join = std::thread::Builder::new()
        .name("raptor-discovery".into())
        .spawn(move || worker.run(&rx));
    let Ok(join) = join else {
        daemon.logger.warn("discovery_unavailable", &[]);
        return None;
    };
    let module = Discovery {
        tx,
        join: Some(join),
    };
    if let Ok(roots) = daemon.profile.discovery_roots() {
        module.discovery_roots_changed(&roots, daemon.home_key());
    }
    Some(Box::new(module))
}

impl DaemonModule for Discovery {
    fn discovery_roots_changed(&self, roots: &[DiscoveryRoot], home: Option<PathBuf>) {
        let _ = self.tx.send(Msg::Roots {
            roots: roots.to_vec(),
            home,
        });
    }

    fn stop(mut self: Box<Self>) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct Root {
    path: PathBuf,
    broad: bool,
    home: bool,
    watched: bool,
    due: Instant,
}

struct Worker {
    config: DiscoveryConfig,
    handle: ShutdownHandle,
    logger: Logger,
    roots: Vec<Root>,
    home: Option<PathBuf>,
    watcher: Watcher,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        loop {
            let now = Instant::now();
            let wait = self
                .roots
                .iter()
                .map(|r| r.due.saturating_duration_since(now))
                .min()
                .unwrap_or(Duration::from_secs(3600));
            match rx.recv_timeout(wait) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Msg::Roots { roots, home }) => self.set_roots(roots, home),
                Ok(Msg::Changed(path)) => {
                    let settle = Instant::now() + self.config.settle;
                    if let Some(root) = self.roots.iter_mut().find(|r| r.path == path) {
                        root.due = root.due.min(settle);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            for i in 0..self.roots.len() {
                if self.roots[i].due <= now {
                    self.list(i);
                }
            }
        }
    }

    fn interval(&self, root: &Root) -> Duration {
        if root.broad || !root.watched {
            if root.broad {
                self.config.broad_poll
            } else {
                self.config.poll
            }
        } else {
            self.config.safety
        }
    }

    fn set_roots(&mut self, roots: Vec<DiscoveryRoot>, home: Option<PathBuf>) {
        self.home = home;
        let now = Instant::now();
        let old = std::mem::take(&mut self.roots);
        for root in &old {
            if root.watched && !roots.iter().any(|r| root.path == Path::new(&r.path)) {
                self.watcher.unwatch(&root.path);
            }
        }
        for declared in roots {
            let path = PathBuf::from(&declared.path);
            let home = self.home.as_ref() == Some(&path);
            if let Some(kept) = old.iter().find(|r| r.path == path) {
                self.roots.push(Root {
                    path,
                    home,
                    ..*kept
                });
                continue;
            }
            let watched = !declared.broad && self.watcher.watch(&path);
            let mut root = Root {
                path,
                broad: declared.broad,
                home,
                watched,
                due: now,
            };
            // Just listed by `discovery.root.add`.
            root.due = now + self.interval(&root);
            self.roots.push(root);
        }
        // At start nothing listed them yet: what appeared while the daemon
        // was down is found now.
        if old.is_empty() {
            for root in &mut self.roots {
                root.due = now;
            }
        }
    }

    fn list(&mut self, i: usize) {
        let root = &self.roots[i];
        let (path, home) = (root.path.clone(), root.home);
        match list_first_level(&path, home) {
            Ok(listing) => {
                // A normal root that grew past the broad threshold is no
                // longer watched (N6), with a diagnostic, without asking.
                if !self.roots[i].broad && listing.entries > BROAD_ENTRIES {
                    self.logger.warn("discovery_root_now_broad", &[]);
                    if self.roots[i].watched {
                        self.watcher.unwatch(&path);
                    }
                    self.roots[i].broad = true;
                    self.roots[i].watched = false;
                }
                self.handle.discovery_listed(&path, listing);
            }
            Err(_) => self.logger.warn("discovery_root_unreadable", &[]),
        }
        let interval = self.interval(&self.roots[i]);
        self.roots[i].due = Instant::now() + interval;
    }
}

/// The first-level watch of the normal roots: none on macOS (see the module
/// doc), one non-recursive `notify` watch per root elsewhere.
struct Watcher {
    #[cfg(not(target_os = "macos"))]
    inner: Option<notify::RecommendedWatcher>,
}

#[cfg(target_os = "macos")]
impl Watcher {
    fn new(_: Sender<Msg>) -> Self {
        Self {}
    }

    fn watch(&mut self, _: &std::path::Path) -> bool {
        false
    }

    fn unwatch(&mut self, _: &std::path::Path) {}
}

#[cfg(not(target_os = "macos"))]
impl Watcher {
    fn new(tx: Sender<Msg>) -> Self {
        use notify::Watcher as _;
        let inner = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            // Only the root is watched: an entry's parent is its root.
            for path in event.paths {
                let name = path.file_name().and_then(|n| n.to_str());
                if name.is_some_and(|n| n.starts_with('.')) {
                    continue;
                }
                if let Some(root) = path.parent() {
                    let _ = tx.send(Msg::Changed(root.to_path_buf()));
                }
            }
        })
        .ok();
        Self { inner }
    }

    fn watch(&mut self, path: &std::path::Path) -> bool {
        use notify::Watcher as _;
        self.inner
            .as_mut()
            .is_some_and(|w| w.watch(path, notify::RecursiveMode::NonRecursive).is_ok())
    }

    fn unwatch(&mut self, path: &std::path::Path) {
        use notify::Watcher as _;
        if let Some(w) = self.inner.as_mut() {
            let _ = w.unwatch(path);
        }
    }
}
