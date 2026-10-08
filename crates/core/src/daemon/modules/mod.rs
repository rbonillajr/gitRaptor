//! Feature modules of the daemon (ADR-GRP-016 § 5): what a feature runs next
//! to the engine plugs into the daemon's life here, in its own file, instead
//! of adding a field and a call at every point of `daemon/mod.rs`.
//!
//! A module starts once the repos are open, right before the observation
//! starts; it hears of each point below in the order of [`MODULES`], and
//! stops after the observer and the session detector and before the stores
//! close. A new module is a file here and one line in [`MODULES`]. The
//! engine's own parts (observer, session detector, stores) are not modules.

mod discovery;
mod guard_health;
mod tm_capture;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::Daemon;
use crate::profile::DiscoveryRoot;
use crate::watch::ObserverHooks;

/// One feature the daemon runs. Every point has a default: a module
/// implements the ones it needs.
pub(super) trait DaemonModule: Send {
    /// A repo is no longer observed: forget what was kept for it.
    fn repo_retired(&self, _repo_id: &str) {}

    /// What the change observer calls, after the session detector.
    fn observer_hooks(&self) -> Option<Arc<dyn ObserverHooks>> {
        None
    }

    /// The engine persisted and published a Git event of a worktree.
    fn git_event(&self, _repo_id: &str, _worktree: &Path, _seq: i64) {}

    /// The declared discovery roots changed (US-GRP-020), with the
    /// canonical home folder.
    fn discovery_roots_changed(&self, _roots: &[DiscoveryRoot], _home: Option<PathBuf>) {}

    /// The daemon stops: end before the stores close.
    fn stop(self: Box<Self>) {}
}

/// Starts a module over the daemon; `None` when it cannot run (it says why
/// in the log) and the daemon goes on without it.
type Start = fn(&Daemon) -> Option<Box<dyn DaemonModule>>;

/// Every module, in the order the daemon starts them and calls them.
const MODULES: &[(&str, Start)] = &[
    ("tm-capture", tm_capture::start),
    ("discovery", discovery::start),
    ("guard-health", guard_health::start),
];

/// The running modules, in the order of [`MODULES`].
#[derive(Default)]
pub(super) struct Modules(Vec<Box<dyn DaemonModule>>);

impl Modules {
    pub(super) fn start(daemon: &Daemon) -> Self {
        Self(
            MODULES
                .iter()
                .filter_map(|(_, start)| start(daemon))
                .collect(),
        )
    }

    pub(super) fn repo_retired(&self, repo_id: &str) {
        for module in &self.0 {
            module.repo_retired(repo_id);
        }
    }

    pub(super) fn observer_hooks(&self) -> impl Iterator<Item = Arc<dyn ObserverHooks>> + '_ {
        self.0.iter().filter_map(|m| m.observer_hooks())
    }

    pub(super) fn git_event(&self, repo_id: &str, worktree: &Path, seq: i64) {
        for module in &self.0 {
            module.git_event(repo_id, worktree, seq);
        }
    }

    pub(super) fn discovery_roots_changed(&self, roots: &[DiscoveryRoot], home: Option<PathBuf>) {
        for module in &self.0 {
            module.discovery_roots_changed(roots, home.clone());
        }
    }

    pub(super) fn stop(self) {
        for module in self.0 {
            module.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Recorder {
        name: &'static str,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Recorder {
        fn note(&self, point: &str) {
            self.log
                .lock()
                .unwrap()
                .push(format!("{point}:{}", self.name));
        }
    }

    struct NoHooks;

    impl ObserverHooks for NoHooks {
        fn worktree_touched(&self, _: &str, _: &Path) {}
        fn git_dir_touched(&self, _: &str, _: u64) {}
    }

    impl DaemonModule for Recorder {
        fn repo_retired(&self, _: &str) {
            self.note("retired");
        }
        fn observer_hooks(&self) -> Option<Arc<dyn ObserverHooks>> {
            self.note("hooks");
            Some(Arc::new(NoHooks))
        }
        fn git_event(&self, _: &str, _: &Path, _: i64) {
            self.note("event");
        }
        fn stop(self: Box<Self>) {
            self.note("stop");
        }
    }

    /// The order of the modules is the order of every point, the stop
    /// included (Validation of ADR-GRP-016 § 5).
    #[test]
    fn every_point_follows_the_registration_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let module = |name| -> Box<dyn DaemonModule> {
            Box::new(Recorder {
                name,
                log: Arc::clone(&log),
            })
        };
        let modules = Modules(vec![module("a"), module("b")]);
        modules.repo_retired("r");
        assert_eq!(modules.observer_hooks().count(), 2);
        modules.git_event("r", Path::new("/w"), 1);
        modules.stop();
        assert_eq!(
            *log.lock().unwrap(),
            [
                "retired:a",
                "retired:b",
                "hooks:a",
                "hooks:b",
                "event:a",
                "event:b",
                "stop:a",
                "stop:b"
            ]
        );
    }

    /// The registered modules and their order: changing it changes when
    /// each one hears of the daemon's life.
    #[test]
    fn the_registered_modules() {
        let names: Vec<_> = MODULES.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["tm-capture", "discovery", "guard-health"]);
    }
}
