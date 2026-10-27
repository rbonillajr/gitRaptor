//! The heartbeat check of the lock and the profile: the daemon stops by itself when the
//! profile it serves is gone, or when another instance took its lock.
//!
//! On Unix, deleting the profile unlinks the files SQLite holds open: writes keep succeeding
//! and are lost, so the check compares file identities, not errors.

use super::shutdown::StopCause;
use super::{Daemon, InstanceLock, Logger, fsperm};
use crate::profile::ProfileProbe;
use crate::repo_lock;

/// What the heartbeat does after looking at the lock and the profile.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ProfileCheck {
    Continue,
    Stop(StopCause),
}

impl Daemon {
    /// The lock first (the profile may already belong to another instance), then the profile.
    pub(super) fn check_profile(&mut self) -> ProfileCheck {
        if let Some(cause) = self.check_instance_lock() {
            return ProfileCheck::Stop(cause);
        }
        react_to_probe(self.profile.probe(), repo_lock::busy(), &self.logger)
    }

    /// `Some(InstanceLost)` when the lock is not ours and cannot be taken back.
    fn check_instance_lock(&mut self) -> Option<StopCause> {
        match self.lock.intact() {
            Ok(true) => return None,
            Ok(false) => {}
            Err(_) => {
                self.logger
                    .warn("profile_probe_failed", &[("kind", "io".into())]);
                return None;
            }
        }
        let state = self.config.dirs.state.clone();
        for dir in self.config.dirs.owned_dirs() {
            if state.starts_with(dir) && fsperm::ensure_private_dir(dir).is_err() {
                return Some(StopCause::InstanceLost);
            }
        }
        match InstanceLock::acquire(&state) {
            Ok(lock) => {
                self.lock = lock;
                self.logger.info("instance_lock_restored", &[]);
                None
            }
            Err(_) => {
                self.logger.warn("instance_lost", &[]);
                Some(StopCause::InstanceLost)
            }
        }
    }
}

/// Decides from a probe result and the number of protected writes in flight (`busy`).
/// Kept apart from [`Daemon`] so the decision is testable with an injected result.
fn react_to_probe(probe: ProfileProbe, busy: usize, logger: &Logger) -> ProfileCheck {
    match probe {
        ProfileProbe::Intact => ProfileCheck::Continue,
        ProfileProbe::Unknown => {
            logger.warn("profile_probe_failed", &[("kind", "io".into())]);
            ProfileCheck::Continue
        }
        // A write the Time Machine or the executor has begun must end on its own profile;
        // the next heartbeat looks again.
        ProfileProbe::Lost if busy > 0 => {
            logger.info("profile_reset_deferred", &[("busy", busy.into())]);
            ProfileCheck::Continue
        }
        ProfileProbe::Lost => ProfileCheck::Stop(StopCause::ProfileLost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::{LOG_FILE, LogLimits};

    fn logger() -> (tempfile::TempDir, Logger) {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::open(dir.path(), LogLimits::default()).unwrap();
        (dir, logger)
    }

    fn log_text(dir: &tempfile::TempDir, logger: &Logger) -> String {
        logger.flush();
        std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap()
    }

    #[test]
    fn pending_protected_writes_defer_the_stop() {
        let (dir, logger) = logger();
        let guard = repo_lock::try_lock("profile-loss-test-repo").expect("the repo is free");
        assert!(repo_lock::busy() >= 1);
        assert_eq!(
            react_to_probe(ProfileProbe::Lost, repo_lock::busy(), &logger),
            ProfileCheck::Continue
        );
        assert!(log_text(&dir, &logger).contains("profile_reset_deferred"));
        drop(guard);
        assert_eq!(
            react_to_probe(ProfileProbe::Lost, 0, &logger),
            ProfileCheck::Stop(StopCause::ProfileLost)
        );
    }

    #[test]
    fn an_unknown_probe_result_logs_and_continues() {
        let (dir, logger) = logger();
        assert_eq!(
            react_to_probe(ProfileProbe::Unknown, 0, &logger),
            ProfileCheck::Continue
        );
        let text = log_text(&dir, &logger);
        assert!(text.contains("profile_probe_failed"), "{text}");
        assert!(!text.contains("profile_reset_deferred"));
    }
}
