//! The Time Machine's continuous capture (US-TMC-004, ADR-TMC-004 § 2) as a
//! module of the daemon.

use std::path::Path;
use std::sync::Arc;

use super::{Daemon, DaemonModule};
use crate::timemachine::continuous::ContinuousCapture;
use crate::timemachine::engine::RepoMarks;
use crate::watch::ObserverHooks;

struct TmCapture {
    capture: ContinuousCapture,
    /// The engine marks the observer feeds for the capture.
    marks: Arc<RepoMarks>,
}

pub(super) fn start(daemon: &Daemon) -> Option<Box<dyn DaemonModule>> {
    match ContinuousCapture::start(daemon.config.tm_capture.config, daemon.capture_deps()) {
        Ok(capture) => Some(Box::new(TmCapture {
            capture,
            marks: Arc::clone(&daemon.marks),
        })),
        Err(_) => {
            daemon.logger.warn("tm_capture_unavailable", &[]);
            None
        }
    }
}

impl DaemonModule for TmCapture {
    fn repo_retired(&self, repo_id: &str) {
        self.capture.forget(repo_id);
    }

    fn observer_hooks(&self) -> Option<Arc<dyn ObserverHooks>> {
        Some(self.capture.observer_hooks(Arc::clone(&self.marks)))
    }

    /// After publishing, outside the engine's budget (ADR-TMC-004 § 2).
    fn git_event(&self, repo_id: &str, worktree: &Path, seq: i64) {
        self.capture.git_event(repo_id, worktree, seq);
    }

    /// A capture in progress ends before the stores close.
    fn stop(self: Box<Self>) {
        self.capture.stop();
    }
}
