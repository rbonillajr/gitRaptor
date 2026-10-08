//! Observation tiers in the daemon (TS-GRP-006, ADR-GRP-010, Enmienda
//! 2026-10-07): when an observed repo goes dormant, and what wakes it.
//!
//! The observer keeps the watches of a dormant repo as its sentinel (N2);
//! the daemon decides when it sleeps (N1: threshold, no session present, no
//! degraded worktree), closes its store, and wakes it on the sentinel, a
//! safety net, a session, a request about the repo or a Guardrails hook
//! (N4). A wake opens the store, reconciles and hands the read to the
//! observer, whose wake batch is persisted and published like any other.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::{Daemon, Field, ShutdownHandle, profile_error_kind, repo_base};
use crate::observe;
use crate::profile::WriteOp;
use crate::watch::{ObserverHooks, SleepRefused, Tier, WakeCause};

/// When repos go dormant. Off by default: nothing sleeps until the switch
/// is set (TS-GRP-006, delivered in parts; see its Dev Spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierConfig {
    /// Time without activity after which a repo sleeps
    /// (`engine.observation.dormantAfterHours`); `None` never sleeps one.
    pub dormant_after: Option<Duration>,
    /// How often the threshold is checked.
    pub check_every: Duration,
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            dormant_after: None,
            check_every: Duration::from_secs(120),
        }
    }
}

/// The daemon's side of the tiers.
#[derive(Debug, Default)]
pub(super) struct Tiers {
    /// Last activity per repo: a published batch, a session, a request.
    last_activity: HashMap<String, Instant>,
    /// Repos asleep with their store closed.
    dormant: HashSet<String>,
    next_check: Option<Instant>,
}

/// Tells the loop a dormant repo must wake.
pub(super) struct WakeHooks(pub ShutdownHandle);

impl ObserverHooks for WakeHooks {
    fn worktree_touched(&self, _: &str, _: &std::path::Path) {}
    fn git_dir_touched(&self, _: &str, _: u64) {}
    fn repo_wake(&self, repo_id: &str, cause: WakeCause) {
        self.0.wake(repo_id, cause);
    }
}

impl Daemon {
    /// When the next threshold check is due, if tiers are on.
    pub(super) fn next_tier_check(&mut self) -> Option<Instant> {
        let config = self.config.tiers;
        config.dormant_after?;
        Some(
            *self
                .tiers
                .next_check
                .get_or_insert_with(|| Instant::now() + config.check_every),
        )
    }

    /// Something happened in the repo: it stays active.
    pub(super) fn note_activity(&mut self, repo_id: &str) {
        self.tiers
            .last_activity
            .insert(repo_id.to_owned(), Instant::now());
    }

    /// Puts to sleep every active repo past the threshold (N1).
    pub(super) fn check_tiers(&mut self) {
        let config = self.config.tiers;
        self.tiers.next_check = Some(Instant::now() + config.check_every);
        let Some(after) = config.dormant_after else {
            return;
        };
        let ids: Vec<String> = self.stores.iter().map(|(id, _)| id.clone()).collect();
        for repo_id in ids {
            let idle = self
                .tiers
                .last_activity
                .get(&repo_id)
                .is_none_or(|t| t.elapsed() >= after);
            if !idle || self.tiers.dormant.contains(&repo_id) {
                continue;
            }
            // A repo with a session present never sleeps.
            let session = self
                .stores
                .iter()
                .find(|(id, _)| *id == repo_id)
                .is_none_or(|(_, s)| s.has_active_sessions().unwrap_or(true));
            if session {
                continue;
            }
            let Some(observer) = &self.observer else {
                return;
            };
            if observer.tier(&repo_id) != Some(Tier::Active) {
                continue;
            }
            match observer.sleep_repo(&repo_id) {
                Ok(()) => self.close_dormant(&repo_id),
                Err(refused) => self.logger.info(
                    "repo_stays_active",
                    &[
                        ("repo", Field::id(&repo_id)),
                        ("reason", refused_reason(refused).into()),
                    ],
                ),
            }
        }
    }

    /// The store of a repo that just went dormant: "observed until" is
    /// written and it is closed (N1 step 4).
    fn close_dormant(&mut self, repo_id: &str) {
        if let Some(pos) = self.stores.iter().position(|(id, _)| id == repo_id) {
            let (_, mut store) = self.stores.remove(pos);
            if let Err(err) = store.write_batch(&[WriteOp::SetObservedUntil {
                ms: super::now_ms(),
            }]) {
                self.logger.error(
                    "observed_until_failed",
                    &[
                        ("repo", Field::id(repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                );
            }
        }
        self.tiers.dormant.insert(repo_id.to_owned());
        self.logger
            .info("repo_dormant", &[("repo", Field::id(repo_id))]);
    }

    /// Wakes a repo if it is dormant or waking (N4): opens its store,
    /// reconciles it and hands the read to the observer.
    pub(super) fn wake_repo(&mut self, repo_id: &str, cause: WakeCause) {
        let asleep = self
            .observer
            .as_ref()
            .and_then(|o| o.tier(repo_id))
            .is_some_and(|t| t != Tier::Active);
        if !asleep {
            return;
        }
        let Some(entry) = self.profile.repo(repo_id).ok().flatten() else {
            return;
        };
        if !self.stores.iter().any(|(id, _)| id == repo_id) {
            match self.profile.open_store(repo_id) {
                Ok((store, _)) => self.stores.push((repo_id.to_owned(), store)),
                Err(err) => {
                    // Without its store nothing could be persisted: it stays
                    // as it is and the next trigger tries again.
                    self.logger.warn(
                        "repo_unavailable",
                        &[
                            ("repo", Field::id(repo_id)),
                            ("kind", profile_error_kind(&err).into()),
                        ],
                    );
                    return;
                }
            }
        }
        self.tiers.dormant.remove(repo_id);
        self.note_activity(repo_id);
        let base = repo_base(&self.stores, repo_id);
        let Ok(read) = observe::reconcile(&entry.canonical_path, &base) else {
            self.logger
                .warn("repo_wake_unreadable", &[("repo", Field::id(repo_id))]);
            return;
        };
        let Some(observer) = &self.observer else {
            return;
        };
        let start = observer.wake_repo(repo_id, &read, cause);
        self.marks.set_head_logs(repo_id, &start.head_logs);
        self.marks.set_heads(repo_id, &start.heads);
        let cause_field = match cause {
            WakeCause::Sentinel => "sentinel",
            WakeCause::SafetyNet { .. } => "safety-net",
        };
        self.logger.info(
            "repo_woken",
            &[("repo", Field::id(repo_id)), ("cause", cause_field.into())],
        );
    }

    /// A request about a repo wakes it first (N4): the TUI, `raptor status`
    /// inside it, an MCP read, the Time Machine or a Guardrails hook.
    pub(super) fn wake_for_request(&mut self, repo_id: &str) {
        if self.tiers.dormant.contains(repo_id)
            || self
                .observer
                .as_ref()
                .and_then(|o| o.tier(repo_id))
                .is_some_and(|t| t != Tier::Active)
        {
            self.wake_repo(repo_id, WakeCause::Sentinel);
        }
        if self.stores.iter().any(|(id, _)| id == repo_id) {
            self.note_activity(repo_id);
        }
    }

    /// The same, from the common directory a hook or a client located.
    pub(super) fn wake_for_common_dir(&mut self, common_dir: &std::path::Path) {
        if let Ok(Some(entry)) = self.profile.repo_by_common_dir(common_dir) {
            self.wake_for_request(&entry.repo_id);
        }
    }

    /// A retired repo leaves the tiers.
    pub(super) fn forget_tiers(&mut self, repo_id: &str) {
        self.tiers.dormant.remove(repo_id);
        self.tiers.last_activity.remove(repo_id);
    }
}

fn refused_reason(refused: SleepRefused) -> &'static str {
    match refused {
        SleepRefused::Unknown => "unknown",
        SleepRefused::NotActive => "not-active",
        SleepRefused::Degraded => "degraded",
        SleepRefused::Busy => "busy",
    }
}
