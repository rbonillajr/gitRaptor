//! The Guardrails requests the daemon serves and the recovery of their
//! installs at start (US-GRD-001).

use gitraptor_api::guard::{HooksLayer, HooksStatus, ProtectionLostData, UnloggedPeriod};
use gitraptor_git::SystemGit;

use super::{Daemon, DaemonConfig, Field, Logger, now_ms, raptor_path};
use super::{GuardLogReply, GuardReply, GuardRequest, HealthReport};

use gitraptor_api::guard::{GuardUninstallResult, PendingKind, UninstallRefusal};

use crate::channel::audit_view::audit_entry;
use crate::guardrails::install::{GuardCtx, InstallError};
use crate::guardrails::log::LogEntry;
use crate::guardrails::pending::{self, Entry, Refused, Requester};
use crate::guardrails::uninstall::{self as guard_uninstall, UninstallError};
use crate::guardrails::{GuardRegistry, install as guard_install};
use crate::profile::{AuditRow, Profile, RepoState, RepoStore};

/// What every applied, cancelled or expired uninstall records with it (ADR-GRD-007 § 2,
/// aceptación del riesgo por acción): the version of the vector table and what the controls of
/// the reserved commands do not cover today.
const RISK_ACCEPTED: &str = "risk-accepted: ADR-GRD-007 § 2 (2026-10-04); uncovered: detached-tree (setsid, launchctl, systemd-run), pty-injection (tmux send-keys), ui-automation (osascript, SendKeys), planted-code";

fn requester_text(r: Requester) -> String {
    serde_json::json!({ "pid": r.pid, "start_us": r.start_us }).to_string()
}

impl Daemon {
    /// A Guardrails request for an observed repo (US-GRD-001). The loop is the only writer
    /// of the repo stores; the install itself writes the repo through the Guardrails write
    /// layer (ADR-GRD-001 § 7).
    pub(super) fn guard(
        &mut self,
        common_dir: &std::path::Path,
        request: GuardRequest,
    ) -> GuardReply {
        let entry = match self.profile.repo_by_common_dir(common_dir) {
            Ok(Some(entry)) if entry.state == RepoState::Observed => entry,
            Ok(_) => return GuardReply::NotObserved,
            Err(_) => return GuardReply::Failed,
        };
        let (Some(git), Some(raptor)) = (self.report.git.clone(), self.raptor_path()) else {
            return GuardReply::Failed;
        };
        let now = now_ms();
        // Expired windows go to the audit before anything reads them (D6).
        let expired = self.guard.pending().sweep_expired(now);
        for (repo_id, e) in &expired {
            self.audit_pending(repo_id, e, "expired", e.requester, now);
        }
        let invoker = self.config.env.invoker();
        let instance = self.profile.instance_id().to_owned();
        let Some((_, store)) = self.stores.iter_mut().find(|(id, _)| *id == entry.repo_id) else {
            return GuardReply::Failed;
        };
        let ctx = GuardCtx {
            git: &git,
            invoker: &invoker,
            dirs: &self.config.dirs,
            instance: &instance,
            raptor: &raptor,
        };
        let common = entry.canonical_path.as_path();
        let repo_id = entry.repo_id.as_str();
        let with_pending = |mut status: gitraptor_api::guard::GuardStatus| {
            status.pending = self.guard.pending().get(repo_id, now);
            status
        };
        // An audit row to write once the store is released: (action, outcome, requester).
        let mut audit: Option<(Entry, &'static str, Requester)> = None;
        // What Guardrails itself changes is an expected transition (US-GRD-004).
        let own = matches!(
            request,
            GuardRequest::Install { .. } | GuardRequest::UninstallApply { .. }
        );
        // The state before a change of ours: the install and the uninstall move the watch.
        let before = self.guard.protection().current(&entry.repo_id);
        let reply = match request {
            GuardRequest::Plan => {
                let mut plan = guard_install::plan(&ctx, repo_id, common, store);
                plan.status = with_pending(plan.status);
                GuardReply::Plan(Box::new(plan))
            }
            GuardRequest::Status => GuardReply::Status(Box::new(with_pending(
                guard_install::status(repo_id, common, store),
            ))),
            GuardRequest::Decline => {
                self.logger
                    .info("guard_declined", &[("repo", Field::id(repo_id))]);
                GuardReply::Status(Box::new(guard_install::decline(repo_id, common, store)))
            }
            GuardRequest::Install { repair } => {
                match guard_install::install(&ctx, repo_id, common, store, &self.guard, now, repair)
                {
                    Ok(status) => {
                        self.logger
                            .info("guard_installed", &[("repo", Field::id(repo_id))]);
                        GuardReply::Status(Box::new(status))
                    }
                    Err(InstallError::Rejected(blockers)) => {
                        self.logger.info(
                            "guard_install_refused",
                            &[
                                ("repo", Field::id(repo_id)),
                                ("blockers", blockers.len().into()),
                            ],
                        );
                        GuardReply::Rejected(blockers)
                    }
                    Err(InstallError::Failed(why)) => {
                        // Only which step failed, from a closed list: the text carries paths.
                        let step = ["folder acl", "folder", "key", "journal", "stub", "config"]
                            .into_iter()
                            .find(|s| why.starts_with(s))
                            .unwrap_or("other");
                        self.logger.error(
                            "guard_install_failed",
                            &[("repo", Field::id(repo_id)), ("step", step.into())],
                        );
                        GuardReply::Failed
                    }
                }
            }
            GuardRequest::UninstallRequest(requester) => {
                let status = guard_install::status(repo_id, common, store);
                // An install of ours that stopped being active can be removed too: otherwise
                // the developer would be stuck with a folder and a journal they cannot clear.
                let removable = status.state == gitraptor_api::guard::ProtectionState::HooksOnly
                    || guard_install::stale_install(store, &status).is_some();
                if !removable {
                    GuardReply::UninstallRefused(UninstallRefusal::NotInstalled.into())
                } else {
                    let announced = self.guard.pending().announce(
                        repo_id,
                        PendingKind::Uninstall,
                        requester,
                        now,
                        pending::window(),
                        pending::new_action_id(),
                    );
                    match announced {
                        Ok(action) => {
                            self.logger
                                .info("guard_uninstall_announced", &[("repo", Field::id(repo_id))]);
                            GuardReply::Uninstall(Box::new(GuardUninstallResult {
                                status: with_pending(status),
                                pending: Some(action),
                            }))
                        }
                        Err(reason) => GuardReply::UninstallRefused(reason.into()),
                    }
                }
            }
            GuardRequest::UninstallApply {
                action_id,
                requester,
            } => {
                let due = self
                    .guard
                    .pending()
                    .take_due(repo_id, &action_id, requester, now);
                match due {
                    Err(refused) => GuardReply::UninstallRefused(refused),
                    Ok(action) => {
                        match guard_uninstall::uninstall(&ctx, repo_id, common, store, &self.guard)
                        {
                            Ok(status) => {
                                self.logger
                                    .info("guard_uninstalled", &[("repo", Field::id(repo_id))]);
                                audit = Some((action, "applied", requester));
                                GuardReply::Uninstall(Box::new(GuardUninstallResult {
                                    status,
                                    pending: None,
                                }))
                            }
                            Err(UninstallError::NotInstalled) => {
                                GuardReply::UninstallRefused(UninstallRefusal::NotInstalled.into())
                            }
                            Err(UninstallError::Failed(_)) => {
                                self.logger.error(
                                    "guard_uninstall_failed",
                                    &[("repo", Field::id(repo_id))],
                                );
                                audit = Some((action, "failed", requester));
                                GuardReply::Failed
                            }
                        }
                    }
                }
            }
            GuardRequest::Cancel(requester) => {
                let cancelled = self.guard.pending().cancel(repo_id, now);
                match cancelled {
                    Some(action) => {
                        self.logger
                            .info("guard_uninstall_cancelled", &[("repo", Field::id(repo_id))]);
                        audit = Some((action, "cancelled", requester));
                        GuardReply::Status(Box::new(with_pending(guard_install::status(
                            repo_id, common, store,
                        ))))
                    }
                    None => {
                        GuardReply::UninstallRefused(Refused::from(UninstallRefusal::NoPending))
                    }
                }
            }
        };
        if let Some((action, outcome, requester)) = audit {
            self.audit_pending(&entry.repo_id, &action, outcome, requester, now);
        }
        self.protection_after(&entry.repo_id, &reply, own.then_some(before));
        reply
    }

    /// Follows the hook layer after a request (US-GRD-004): what Guardrails itself did (install,
    /// uninstall) is an expected change, logged and never alerted; what a status finds is a
    /// change nobody on our side made.
    fn protection_after(&mut self, repo_id: &str, reply: &GuardReply, own: Option<HooksLayer>) {
        let status = match reply {
            GuardReply::Status(status) => status.as_ref(),
            GuardReply::Plan(plan) => &plan.status,
            GuardReply::Uninstall(result) => &result.status,
            _ => return,
        };
        let Some(layer) = status.hooks.clone() else {
            return;
        };
        match own {
            Some(before) => self.protection_changed(repo_id, Some(before), layer, true),
            None => self.protection_changed(repo_id, None, layer, false),
        }
    }

    /// A check found the hook layer in this state (US-GRD-004).
    ///
    /// The thread that checked did so while the loop may have been installing or removing the
    /// protection: the loop is the one place where nothing else changes it, so what it found is
    /// read again here, against the install published now (none, and the report is stale).
    pub(super) fn guard_health(&mut self, report: HealthReport) {
        let watched = self.guard.protection().get(&report.repo_id);
        let Some(watched) = watched else {
            return;
        };
        // An install or an uninstall of ours that did not finish (it fails half way, or waits
        // for the next start to be completed) is not somebody else's doing.
        // A dormant repo has no open store (ADR-GRP-015): it wakes for a change of state, which
        // is rare, and never for a check that finds nothing.
        self.wake_for_request(&report.repo_id);
        let confirmed = self
            .stores
            .iter()
            .find(|(id, _)| *id == report.repo_id)
            .and_then(|(_, store)| store.guard_keys().ok())
            .and_then(|keys| keys.journal)
            .and_then(|j| crate::guardrails::journal::Journal::from_json(&j))
            .is_some_and(|j| j.stage == crate::guardrails::journal::Stage::Confirmed);
        if !confirmed {
            return;
        }
        let layer = crate::guardrails::health::check(&watched.common, Some(&watched.journal)).hooks;
        self.protection_changed(&report.repo_id, None, layer, false);
    }

    /// Records the state of the hook layer: a change is a transition in the decision log and,
    /// when nobody on our side made it and the debounce lets it through, an alert on the event
    /// stream (ADR-GRD-005 § 5). Nothing is repaired here: the developer is told, and
    /// `raptor guard install` is theirs to run.
    fn protection_changed(
        &mut self,
        repo_id: &str,
        before: Option<HooksLayer>,
        layer: HooksLayer,
        expected: bool,
    ) {
        let (now, offset) = crate::watch::wall_now();
        let seen = {
            let mut protection = self.guard.protection();
            match before {
                Some(from) => protection.observe_from(repo_id, from, layer, now, expected),
                None => protection.observe(repo_id, layer, now, expected),
            }
        };
        let Some(transition) = seen else {
            return;
        };
        let Ok(Some(repo)) = self.profile.repo(repo_id) else {
            return;
        };
        self.wake_for_request(repo_id);
        let entry = guard_protection_entry(
            &repo.canonical_path.to_string_lossy(),
            now,
            offset,
            &transition,
        );
        let written = self
            .stores
            .iter_mut()
            .find(|(id, _)| id == repo_id)
            .is_some_and(|(_, store)| store.record_guard_decision(&entry).is_ok());
        if !written {
            self.logger.error(
                "guard_protection_log_failed",
                &[("repo", Field::id(repo_id))],
            );
        }
        if matches!(
            transition.from,
            HooksStatus::Inactive | HooksStatus::Orphaned
        ) && transition.to.status == HooksStatus::Active
        {
            self.bus.publish(
                gitraptor_api::event::GUARD_PROTECTION_RESTORED,
                ProtectionLostData {
                    repo_id: repo_id.to_owned(),
                    hooks: transition.to.clone(),
                },
                None,
                |_| {},
            );
        }
        if transition.alert {
            self.logger
                .warn("guard_protection_lost", &[("repo", Field::id(repo_id))]);
            self.bus.publish(
                gitraptor_api::event::GUARD_PROTECTION_LOST,
                ProtectionLostData {
                    repo_id: repo_id.to_owned(),
                    hooks: transition.to,
                },
                None,
                |_| {},
            );
        }
    }

    /// The permanent audit of an announced action that was applied, cancelled, failed or
    /// expired, with the risk it accepted (ADR-GRD-007 § 2). The announcement itself is the
    /// accepted `guard.uninstall` audited by the channel, with the full ancestry.
    fn audit_pending(
        &mut self,
        repo_id: &str,
        action: &Entry,
        outcome: &'static str,
        who: Requester,
        now: i64,
    ) {
        let row = AuditRow {
            at_ms: now,
            operation: gitraptor_api::methods::GUARD_UNINSTALL.to_owned(),
            repo_id: Some(repo_id.to_owned()),
            outcome: outcome.to_owned(),
            reason: Some(format!("{RISK_ACCEPTED}; action {}", action.action_id)),
            client: requester_text(who),
            chain: requester_text(action.requester),
        };
        match self.profile.append_audit(&row) {
            // Whoever holds `audit.outcomes` reads it as it happens (the bus drops it for
            // the rest).
            Ok(id) => {
                if let Some(entry) = audit_entry(id, &row) {
                    self.bus
                        .publish(gitraptor_api::event::RESERVED_AUDIT, entry, None, |_| {});
                }
            }
            Err(_) => self.logger.error(
                "audit_write_failed",
                &[("op", Field::Text("guard.uninstall"))],
            ),
        }
    }
}

fn guard_protection_entry(
    common_dir: &str,
    at_ms: i64,
    utc_offset_s: i32,
    t: &crate::guardrails::protection::Transition,
) -> LogEntry {
    crate::guardrails::log::protection_entry(
        common_dir,
        at_ms,
        utc_offset_s,
        t.from,
        &t.to,
        t.expected,
    )
}

/// Startup recovery of the Guardrails installs of every observed repo.
pub(super) fn recover_guardrails(
    config: &DaemonConfig,
    profile: &Profile,
    logger: &Logger,
    git: Option<&SystemGit>,
    stores: &mut [(String, RepoStore)],
    registry: &GuardRegistry,
) {
    let Some(raptor) = raptor_path(config) else {
        return;
    };
    let invoker = config.env.invoker();
    for (repo_id, store) in stores.iter_mut() {
        let Ok(Some(entry)) = profile.repo(repo_id) else {
            continue;
        };
        let Some(git) = git else {
            // Without Git only the confirmed installs are published; an unfinished one waits.
            if let Some(journal) = store
                .guard_keys()
                .ok()
                .and_then(|k| k.journal)
                .and_then(|j| crate::guardrails::journal::Journal::from_json(&j))
                .filter(|j| j.stage == crate::guardrails::journal::Stage::Confirmed)
            {
                let confirmed = store.confirmed_team_baseline().ok().flatten();
                guard_install::publish(&config.dirs, repo_id, &journal, registry, confirmed);
            }
            continue;
        };
        let ctx = GuardCtx {
            git,
            invoker: &invoker,
            dirs: &config.dirs,
            instance: profile.instance_id(),
            raptor: &raptor,
        };
        match guard_install::recover(&ctx, repo_id, &entry.canonical_path, store, registry) {
            guard_install::Recovery::Nothing => {}
            guard_install::Recovery::Confirmed => {
                logger.info("guard_install_recovered", &[("repo", Field::id(repo_id))]);
            }
            guard_install::Recovery::RolledBack => {
                logger.info("guard_install_rolled_back", &[("repo", Field::id(repo_id))]);
            }
            guard_install::Recovery::UninstallCompleted => {
                logger.info("guard_uninstall_recovered", &[("repo", Field::id(repo_id))]);
            }
            guard_install::Recovery::UninstallKept => {
                // `no completada`: the protection stays, and the developer is told on status.
                logger.warn(
                    "guard_uninstall_incomplete",
                    &[("repo", Field::id(repo_id))],
                );
            }
        }
    }
}

impl Daemon {
    /// The store of the observed repo of a dispatcher's common directory: found by the
    /// profile's own entry, never by the id the hook client sent (US-GRD-005, D3).
    fn observed_store(&mut self, common_dir: &std::path::Path) -> Option<&mut RepoStore> {
        let entry = match self.profile.repo_by_common_dir(common_dir) {
            Ok(Some(entry)) if entry.state == RepoState::Observed => entry,
            _ => return None,
        };
        self.stores
            .iter_mut()
            .find(|(id, _)| *id == entry.repo_id)
            .map(|(_, store)| store)
    }

    /// Writes one decision log entry (US-GRD-005). A repo that is not observed has no store:
    /// the entry is dropped with a diagnostic that carries none of its content.
    pub(super) fn guard_record(&mut self, entry: LogEntry) {
        self.guard.log().release();
        let common = std::path::PathBuf::from(&entry.common_dir);
        // A hook ran in the repo: a dormant one wakes and opens its store
        // before the entry is written (TS-GRP-006, N4).
        self.wake_for_common_dir(&common);
        let written = match self.observed_store(&common) {
            Some(store) => store.record_guard_decision(&entry).is_ok(),
            None => {
                self.logger.info("guard_log_dropped", &[]);
                true
            }
        };
        if !written {
            self.logger.error("guard_log_write_failed", &[]);
        }
        self.flush_guard_overflow();
    }

    /// Writes the occurrences the connections counted over the in-flight cap (D4).
    fn flush_guard_overflow(&mut self) {
        for row in self.guard.log().drain() {
            let common = std::path::PathBuf::from(&row.common_dir);
            let written = self
                .observed_store(&common)
                .is_some_and(|store| store.record_guard_overflow(&row).is_ok());
            if !written {
                self.logger
                    .info("guard_log_dropped", &[("count", (row.count as i64).into())]);
            }
        }
    }

    /// `guard.log`: the pending occurrences are written first, so the count is the real one.
    pub(super) fn guard_log(
        &mut self,
        common_dir: &std::path::Path,
        since_ms: Option<i64>,
        limit: u32,
        hide_relax_ignored: bool,
    ) -> GuardLogReply {
        self.flush_guard_overflow();
        let entry = match self.profile.repo_by_common_dir(common_dir) {
            Ok(Some(entry)) if entry.state == RepoState::Observed => entry,
            Ok(_) => return GuardLogReply::NotObserved,
            Err(_) => return GuardLogReply::Failed,
        };
        let Some((_, store)) = self.stores.iter_mut().find(|(id, _)| *id == entry.repo_id) else {
            return GuardLogReply::Failed;
        };
        let now = now_ms();
        let Ok(mut log) = store.guard_log(since_ms.unwrap_or(0), limit, now, hide_relax_ignored)
        else {
            return GuardLogReply::Failed;
        };
        // The interval before this start is known now, while its gap reaches the store only
        // with the observer's next batch: report it from the startup report.
        let pending = self
            .report
            .repos
            .iter()
            .find(|r| r.repo_id == entry.repo_id)
            .and_then(|r| r.pending_gap.as_ref())
            .and_then(|gap| gap.from_ms);
        if let Some(from_ms) = pending
            && self.started_ms >= log.since_ms
            && !log.unlogged_periods.iter().any(|p| p.from_ms == from_ms)
        {
            log.unlogged_periods.push(UnloggedPeriod {
                from_ms,
                to_ms: Some(self.started_ms),
            });
            log.unlogged_periods.sort_by_key(|p| p.from_ms);
        }
        GuardLogReply::Log(Box::new(log))
    }

    /// Purges the expired entries of every store, at the first heartbeat and then every 24 h
    /// (ADR-GRD-006 § 3); the query filters by date in between.
    pub(super) fn guard_log_maintenance(&mut self, now: i64) {
        // An announced uninstall nobody applied: its expiry goes to the audit too (D6).
        let expired = self.guard.pending().sweep_expired(now);
        for (repo_id, e) in &expired {
            self.audit_pending(repo_id, e, "expired", e.requester, now);
        }
        self.flush_guard_overflow();
        if !self.guard.log().purge_due(now) {
            return;
        }
        for (repo_id, store) in self.stores.iter_mut() {
            if let Ok(purged) = store.purge_guard_log(now)
                && purged > 0
            {
                self.logger.info(
                    "guard_log_purged",
                    &[("repo", Field::id(repo_id)), ("entries", purged.into())],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RISK_ACCEPTED;
    use crate::channel::audit_view::RISK_ACCEPTED_PREFIX;

    /// The audit view reads the risk text by its prefix: the text the daemon writes keeps it.
    #[test]
    fn the_accepted_risk_keeps_the_prefix_the_audit_view_reads() {
        assert!(RISK_ACCEPTED.starts_with(RISK_ACCEPTED_PREFIX));
    }
}
