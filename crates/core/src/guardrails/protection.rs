//! What the daemon follows of the hook layer of each protected repo (US-GRD-004, ADR-GRD-005
//! § 5): the confirmed install to check, the last state seen, and when each cause last alerted.
//! A change of state is a transition; only one nobody on our side made, and that the debounce
//! lets through, is an alert.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use gitraptor_api::guard::{HooksLayer, HooksStatus, LossCause};

use super::journal::Journal;

/// One alert at most per repo and cause every 10 minutes (⚠️ ASSUMPTION of ADR-GRD-005 § 5, L-01).
pub const ALERT_DEBOUNCE_MS: i64 = 10 * 60 * 1000;

/// How often the daemon checks the hook layer of every protected repo (⚠️ ASSUMPTION of
/// ADR-GRD-005 § 4: 60 s, with a jitter the checker adds).
pub const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(60);
/// The check hashes the files again at least this often even when their stat did not change,
/// since a modification time can be forged.
pub const FULL_EVERY: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// A repo's Git events trigger a check at most this often.
pub const EVENT_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// Debug builds only: another interval, in milliseconds, for the end-to-end suites.
pub const HEALTH_ENV: &str = "GITRAPTOR_TEST_GUARD_HEALTH_MS";

/// The interval of this daemon.
pub fn check_every() -> std::time::Duration {
    if cfg!(debug_assertions)
        && let Some(ms) = std::env::var(HEALTH_ENV)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
    {
        return std::time::Duration::from_millis(ms);
    }
    CHECK_EVERY
}

/// A confirmed install to check: published after an install and at startup.
#[derive(Debug, Clone)]
pub struct Watched {
    pub common: PathBuf,
    pub journal: Journal,
}

/// A change of state of the hook layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub from: HooksStatus,
    pub to: HooksLayer,
    /// Guardrails made it (install, uninstall, repair): logged, never alerted.
    pub expected: bool,
    /// Tell the developer now: nobody on our side did it and the debounce lets it through.
    pub alert: bool,
}

/// What the check thread remembers of one repo between ticks.
#[derive(Debug, Clone, Copy)]
pub struct Seen {
    pub fingerprint: u64,
    pub full_at: Instant,
}

#[derive(Debug, Default)]
pub struct Protection {
    watched: HashMap<String, Watched>,
    tracked: HashMap<String, HooksLayer>,
    alerted: HashMap<(String, Option<LossCause>), i64>,
    seen: HashMap<String, Seen>,
}

impl Protection {
    pub fn watch(&mut self, repo_id: &str, watched: Watched) {
        self.watched.insert(repo_id.to_owned(), watched);
        self.seen.remove(repo_id);
    }

    /// The install is gone: what was followed of it goes too, so a later install starts clean.
    pub fn unwatch(&mut self, repo_id: &str) {
        self.watched.remove(repo_id);
        self.seen.remove(repo_id);
        self.tracked.remove(repo_id);
        self.alerted.retain(|(id, _), _| id != repo_id);
    }

    pub fn watched(&self) -> Vec<(String, Watched)> {
        self.watched
            .iter()
            .map(|(id, w)| (id.clone(), w.clone()))
            .collect()
    }

    pub fn get(&self, repo_id: &str) -> Option<Watched> {
        self.watched.get(repo_id).cloned()
    }

    /// The last state seen; while nothing was seen, what the install says: active for a
    /// confirmed install, not installed otherwise.
    pub fn current(&self, repo_id: &str) -> HooksLayer {
        self.tracked.get(repo_id).cloned().unwrap_or_else(|| {
            HooksLayer::of(if self.watched.contains_key(repo_id) {
                HooksStatus::Active
            } else {
                HooksStatus::NotInstalled
            })
        })
    }

    pub fn seen(&self, repo_id: &str) -> Option<Seen> {
        self.seen.get(repo_id).copied()
    }

    pub fn set_seen(&mut self, repo_id: &str, seen: Seen) {
        self.seen.insert(repo_id.to_owned(), seen);
    }

    /// Records the state a check found. `None` when nothing changed. `expected` is true when
    /// Guardrails did it itself.
    pub fn observe(
        &mut self,
        repo_id: &str,
        layer: HooksLayer,
        now_ms: i64,
        expected: bool,
    ) -> Option<Transition> {
        let from = self.current(repo_id);
        self.observe_from(repo_id, from, layer, now_ms, expected)
    }

    /// [`Protection::observe`] against the state seen before a change of Guardrails' own, which
    /// the install and the uninstall already published (the watch changes with them).
    pub fn observe_from(
        &mut self,
        repo_id: &str,
        from: HooksLayer,
        layer: HooksLayer,
        now_ms: i64,
        expected: bool,
    ) -> Option<Transition> {
        if from == layer {
            return None;
        }
        self.tracked.insert(repo_id.to_owned(), layer.clone());
        let lost = matches!(layer.status, HooksStatus::Inactive | HooksStatus::Orphaned);
        let alert = lost && !expected && {
            let key = (repo_id.to_owned(), layer.cause);
            let due = self
                .alerted
                .get(&key)
                .is_none_or(|at| now_ms - at >= ALERT_DEBOUNCE_MS);
            if due {
                self.alerted.insert(key, now_ms);
            }
            due
        };
        Some(Transition {
            from: from.status,
            to: layer,
            expected,
            alert,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lost(cause: LossCause) -> HooksLayer {
        HooksLayer::lost(cause)
    }

    #[test]
    fn the_first_loss_alerts_and_the_same_cause_is_debounced() {
        let mut p = Protection::default();
        let active = HooksLayer::of(HooksStatus::Active);
        let t = p
            .observe("r", lost(LossCause::FolderMissing), 0, false)
            .unwrap();
        assert!(t.alert && !t.expected);
        // Back and lost again within ten minutes: logged, not alerted.
        assert!(!p.observe("r", active.clone(), 1_000, false).unwrap().alert);
        let again = p
            .observe("r", lost(LossCause::FolderMissing), 2_000, false)
            .unwrap();
        assert!(!again.alert);
        // Another cause is another alert; the same cause after the window alerts again.
        assert!(
            p.observe("r", lost(LossCause::DispatcherMissing), 3_000, false)
                .unwrap()
                .alert
        );
        let _ = p.observe("r", active, 4_000, false);
        assert!(
            p.observe(
                "r",
                lost(LossCause::FolderMissing),
                ALERT_DEBOUNCE_MS + 5_000,
                false
            )
            .unwrap()
            .alert
        );
    }

    #[test]
    fn an_expected_change_never_alerts_and_nothing_changed_is_nothing() {
        let mut p = Protection::default();
        let t = p
            .observe("r", HooksLayer::of(HooksStatus::Active), 0, true)
            .unwrap();
        assert!(t.expected && !t.alert);
        assert_eq!(t.from, HooksStatus::NotInstalled);
        assert!(
            p.observe("r", HooksLayer::of(HooksStatus::Active), 1, false)
                .is_none()
        );
        let t = p
            .observe("r", HooksLayer::of(HooksStatus::NotInstalled), 2, true)
            .unwrap();
        assert!(!t.alert);
    }
}
