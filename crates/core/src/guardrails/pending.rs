//! The reserved actions that relax, waiting for their window (ADR-GRD-007 § 1, D5; US-GRD-003):
//! announced by the requester's first call, applied only by that same requester once the window
//! closed and before it expires, cancellable by anyone meanwhile. One per repo, in the daemon's
//! memory only: a restart drops them (nothing was applied, the protection stays).

use std::collections::HashMap;
use std::time::Duration;

use gitraptor_api::guard::{PendingAction, PendingKind, UninstallRefusal};

/// Length of the window (⚠️ ASSUMPTION of ADR-GRD-007 § 1: 10 s).
pub const WINDOW: Duration = Duration::from_secs(10);
/// How long after the window closes the requester may still apply it.
pub const GRACE: Duration = Duration::from_secs(60);
/// Debug builds only: another window for the end-to-end suites.
pub const WINDOW_ENV: &str = "GITRAPTOR_TEST_GUARD_WINDOW_MS";

/// Debug builds only: another grace for the end-to-end suites (how soon an unapplied
/// announcement expires).
pub const GRACE_ENV: &str = "GITRAPTOR_TEST_GUARD_GRACE_MS";

/// The grace of this daemon.
pub fn grace() -> Duration {
    if cfg!(debug_assertions)
        && let Some(ms) = std::env::var(GRACE_ENV)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    GRACE
}

/// The window of this daemon.
pub fn window() -> Duration {
    if cfg!(debug_assertions)
        && let Some(ms) = std::env::var(WINDOW_ENV)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    WINDOW
}

/// Who asked: the process of the reserved call, with the start time that makes its id unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requester {
    pub pid: u32,
    pub start_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub action_id: String,
    pub kind: PendingKind,
    pub requester: Requester,
    pub applies_at_ms: i64,
    pub window_ms: u64,
    /// What the announcement showed would be written back as `core.hooksPath` (the removal of an
    /// orphan, read from its manifest): applied only if it is still what the repo says.
    pub restores: Option<String>,
    /// What the confirmation of the team baseline showed (its fingerprint): applied only if the
    /// team level still gives the same one.
    pub fingerprint: Option<String>,
    /// What a conscious exception announced it would skip (US-GRD-006).
    pub exception: Option<super::exception::ExceptionPlan>,
    /// The reasons of that announcement, as the log names them (with their level).
    pub exception_reasons: Vec<gitraptor_api::guard::LoggedReason>,
    /// The textconv drivers its git is to run with neutralized, read when it was announced.
    pub exception_drivers: Vec<String>,
}

impl Entry {
    fn expired(&self, now_ms: i64) -> bool {
        now_ms > self.applies_at_ms + grace().as_millis() as i64
    }

    pub fn view(&self) -> PendingAction {
        PendingAction {
            action_id: self.action_id.clone(),
            action: self.kind,
            applies_at_ms: self.applies_at_ms,
            window_ms: self.window_ms,
            // The channel drops it for a connection without `guard.exception`.
            exception: self
                .exception
                .as_ref()
                .map(super::exception::ExceptionPlan::view),
        }
    }
}

/// Why a call to apply did nothing, and how long is left when the window is still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    pub reason: UninstallRefusal,
    pub remaining_ms: Option<u64>,
}

impl From<UninstallRefusal> for Refused {
    fn from(reason: UninstallRefusal) -> Self {
        Self {
            reason,
            remaining_ms: None,
        }
    }
}

/// An opaque, random id (128 bits from the OS-seeded hasher keys).
pub fn new_action_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let half = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

/// The pending actions by repo id.
#[derive(Debug, Default)]
pub struct PendingActions {
    by_repo: HashMap<String, Entry>,
}

impl PendingActions {
    /// Attaches the plan of an exception to the pending action `action_id` of `repo_id`.
    pub fn note_exception(
        &mut self,
        repo_id: &str,
        action_id: &str,
        plan: super::exception::ExceptionPlan,
    ) {
        if let Some(entry) = self
            .by_repo
            .get_mut(repo_id)
            .filter(|e| e.action_id == action_id)
        {
            entry.exception = Some(plan);
        }
    }

    /// Records the reasons, with their level, the exception `action_id` announced.
    pub fn note_exception_reasons(
        &mut self,
        repo_id: &str,
        action_id: &str,
        reasons: Vec<gitraptor_api::guard::LoggedReason>,
    ) {
        if let Some(entry) = self
            .by_repo
            .get_mut(repo_id)
            .filter(|e| e.action_id == action_id)
        {
            entry.exception_reasons = reasons;
        }
    }

    /// Records the textconv drivers the exception `action_id` neutralizes in its git.
    pub fn note_exception_drivers(&mut self, repo_id: &str, action_id: &str, drivers: Vec<String>) {
        if let Some(entry) = self
            .by_repo
            .get_mut(repo_id)
            .filter(|e| e.action_id == action_id)
        {
            entry.exception_drivers = drivers;
        }
    }

    /// Opens the window of `kind` in `repo_id`. One at a time per repo.
    pub fn announce(
        &mut self,
        repo_id: &str,
        kind: PendingKind,
        requester: Requester,
        now_ms: i64,
        window: Duration,
        action_id: String,
    ) -> Result<PendingAction, UninstallRefusal> {
        if self
            .by_repo
            .get(repo_id)
            .is_some_and(|e| !e.expired(now_ms))
        {
            return Err(UninstallRefusal::AlreadyPending);
        }
        let window_ms = window.as_millis() as u64;
        let entry = Entry {
            action_id,
            kind,
            requester,
            applies_at_ms: now_ms + window_ms as i64,
            window_ms,
            restores: None,
            fingerprint: None,
            exception: None,
            exception_reasons: Vec::new(),
            exception_drivers: Vec::new(),
        };
        let view = entry.view();
        self.by_repo.insert(repo_id.to_owned(), entry);
        Ok(view)
    }

    /// Records what the announcement of `action_id` showed it restores.
    pub fn note_restores(&mut self, repo_id: &str, action_id: &str, restores: Option<String>) {
        if let Some(entry) = self
            .by_repo
            .get_mut(repo_id)
            .filter(|e| e.action_id == action_id)
        {
            entry.restores = restores;
        }
    }

    /// Records the fingerprint of the plan the announcement of `action_id` showed.
    pub fn note_fingerprint(&mut self, repo_id: &str, action_id: &str, fingerprint: String) {
        if let Some(entry) = self
            .by_repo
            .get_mut(repo_id)
            .filter(|e| e.action_id == action_id)
        {
            entry.fingerprint = Some(fingerprint);
        }
    }

    /// The action to apply now: the same announcement of the same kind, the same requester, the window closed
    /// and not expired. It leaves the list when taken.
    pub fn take_due(
        &mut self,
        repo_id: &str,
        action_id: &str,
        kind: PendingKind,
        requester: Requester,
        now_ms: i64,
    ) -> Result<Entry, Refused> {
        // An announcement of another kind is not this one: it stays where it is.
        let Some(entry) = self
            .by_repo
            .get(repo_id)
            .filter(|e| e.action_id == action_id && e.kind == kind && !e.expired(now_ms))
        else {
            return Err(UninstallRefusal::NoPending.into());
        };
        if entry.requester != requester {
            return Err(UninstallRefusal::NotTheRequester.into());
        }
        if now_ms < entry.applies_at_ms {
            return Err(Refused {
                reason: UninstallRefusal::WindowOpen,
                remaining_ms: Some((entry.applies_at_ms - now_ms) as u64),
            });
        }
        Ok(self.by_repo.remove(repo_id).expect("checked above"))
    }

    /// Cancels the action waiting in `repo_id`, if any.
    pub fn cancel(&mut self, repo_id: &str, now_ms: i64) -> Option<Entry> {
        self.by_repo.remove(repo_id).filter(|e| !e.expired(now_ms))
    }

    /// The action waiting in `repo_id`.
    pub fn get(&self, repo_id: &str, now_ms: i64) -> Option<PendingAction> {
        self.by_repo
            .get(repo_id)
            .filter(|e| !e.expired(now_ms))
            .map(Entry::view)
    }

    /// Drops the expired actions and returns them, for the audit.
    pub fn sweep_expired(&mut self, now_ms: i64) -> Vec<(String, Entry)> {
        let expired: Vec<String> = self
            .by_repo
            .iter()
            .filter(|(_, e)| e.expired(now_ms))
            .map(|(id, _)| id.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| self.by_repo.remove(&id).map(|e| (id, e)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: Requester = Requester {
        pid: 10,
        start_us: 1,
    };
    const W: Duration = Duration::from_secs(10);

    fn open(p: &mut PendingActions, now: i64) -> String {
        p.announce("r", PendingKind::Uninstall, ME, now, W, "a1".into())
            .unwrap()
            .action_id
    }

    #[test]
    fn only_the_requester_applies_it_once_the_window_closed() {
        let mut p = PendingActions::default();
        let id = open(&mut p, 1_000);
        let early = p
            .take_due("r", &id, PendingKind::Uninstall, ME, 5_000)
            .unwrap_err();
        assert_eq!(early.reason, UninstallRefusal::WindowOpen);
        assert_eq!(early.remaining_ms, Some(6_000));
        let other = Requester { pid: 11, ..ME };
        assert_eq!(
            p.take_due("r", &id, PendingKind::Uninstall, other, 11_000)
                .unwrap_err()
                .reason,
            UninstallRefusal::NotTheRequester
        );
        // A pid reused by another process is another requester.
        let reused = Requester { start_us: 2, ..ME };
        assert_eq!(
            p.take_due("r", &id, PendingKind::Uninstall, reused, 11_000)
                .unwrap_err()
                .reason,
            UninstallRefusal::NotTheRequester
        );
        assert_eq!(
            p.take_due("r", "other-id", PendingKind::Uninstall, ME, 11_000)
                .unwrap_err()
                .reason,
            UninstallRefusal::NoPending
        );
        assert!(
            p.take_due("r", &id, PendingKind::Uninstall, ME, 11_000)
                .is_ok()
        );
        // Taken: applying twice does nothing.
        assert_eq!(
            p.take_due("r", &id, PendingKind::Uninstall, ME, 11_000)
                .unwrap_err()
                .reason,
            UninstallRefusal::NoPending
        );
    }

    #[test]
    fn an_announcement_of_another_kind_is_neither_applied_nor_dropped() {
        let mut p = PendingActions::default();
        let id = open(&mut p, 0);
        assert_eq!(
            p.take_due("r", &id, PendingKind::ConfirmTeam, ME, 20_000)
                .unwrap_err()
                .reason,
            UninstallRefusal::NoPending
        );
        // Still there, and still the uninstall's.
        assert_eq!(
            p.get("r", 20_000).map(|a| a.action),
            Some(PendingKind::Uninstall)
        );
        assert!(
            p.take_due("r", &id, PendingKind::Uninstall, ME, 20_000)
                .is_ok()
        );
    }

    #[test]
    fn one_at_a_time_and_a_cancel_closes_it() {
        let mut p = PendingActions::default();
        let id = open(&mut p, 0);
        assert_eq!(
            p.announce("r", PendingKind::Uninstall, ME, 1, W, "a2".into()),
            Err(UninstallRefusal::AlreadyPending)
        );
        assert!(p.get("r", 1).is_some());
        assert!(p.cancel("r", 1).is_some());
        assert!(p.get("r", 1).is_none());
        assert_eq!(
            p.take_due("r", &id, PendingKind::Uninstall, ME, 20_000)
                .unwrap_err()
                .reason,
            UninstallRefusal::NoPending
        );
    }

    #[test]
    fn an_expired_action_cannot_be_applied_and_is_swept() {
        let mut p = PendingActions::default();
        let id = open(&mut p, 0);
        let late = 10_000 + GRACE.as_millis() as i64 + 1;
        assert!(p.get("r", late).is_none());
        assert_eq!(
            p.take_due("r", &id, PendingKind::Uninstall, ME, late)
                .unwrap_err()
                .reason,
            UninstallRefusal::NoPending
        );
        let swept = p.sweep_expired(late);
        assert_eq!(swept.len(), 1);
        // A new announcement can open after an expired one.
        assert!(
            p.announce("r", PendingKind::Uninstall, ME, late, W, "a3".into())
                .is_ok()
        );
    }

    #[test]
    fn action_ids_are_distinct() {
        assert_ne!(new_action_id(), new_action_id());
        assert_eq!(new_action_id().len(), 32);
    }
}
