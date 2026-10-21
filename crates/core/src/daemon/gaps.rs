//! The gap of a start: the interval the engine did not observe, recorded
//! with what changed in it before the repo is observed again (ADR-GRP-013
//! § 5), and the query that serves the gaps of a repo.
//!
//! What the reconciliation finds is the net state, not the full history: a
//! file edited and reverted, a branch created and deleted, or a commit
//! dropped by a reset without a reflog trace leave nothing. Everything found
//! is unattributed: no session, evidence or authorship (BR-EDGE-005).

use std::path::{Path, PathBuf};

use super::{
    Daemon, Field, Logger, PendingGap, RepoCommandError, now_ms, profile_error_kind, run_id,
};
use crate::observe::RepoRead;
use crate::profile::{
    Gap, GapCause, KnownState, NewEvent, RepoStore, SessionWithState, StoreOpen, Timestamp, WriteOp,
};
use crate::watch::{RawEvent, RefsView, WorktreeRefs, classify_gap};
use gitraptor_api::messages::{GitEventDetails, GitEventKind, WorktreeStatus};
use gitraptor_api::methods::{EventsGapsParams, GapCauseView, GapView, MAX_GAPS_PAGE};

/// What a start records for one repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GapPlan {
    pub cause: GapCause,
    pub started_ms: i64,
    pub requested_by: Option<String>,
}

/// The non-gone worktrees of the store with their last known state.
pub(super) fn known_states(store: &RepoStore) -> Vec<(PathBuf, KnownState)> {
    store
        .worktrees()
        .map(|all| {
            all.into_iter()
                .filter(|w| w.gone_ms.is_none())
                .filter_map(|w| {
                    let state = store.last_known_state(&w.path).ok().flatten()?;
                    Some((w.path, state))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The gap this start must record, if any. A recovered store opens a
/// `store-corrupt` gap from the date the repo was added (its history is
/// gone); a previous run that did not end cleanly opens the gap its cause
/// says, from "observed until", else the latest known state, else the date
/// the repo was added.
pub(super) fn startup_plan(
    store: &RepoStore,
    pending: Option<&PendingGap>,
    status: &StoreOpen,
    added_ms: i64,
) -> Option<GapPlan> {
    if matches!(status, StoreOpen::Recovered { .. }) {
        return Some(GapPlan {
            cause: GapCause::StoreCorrupt,
            started_ms: added_ms,
            requested_by: None,
        });
    }
    let pending = pending?;
    let started_ms = pending
        .from_ms
        .or_else(|| {
            known_states(store)
                .iter()
                .map(|(_, state)| state.updated_ms)
                .max()
        })
        .unwrap_or(added_ms);
    Some(GapPlan {
        cause: pending.cause,
        started_ms,
        requested_by: pending.requested_by.clone(),
    })
}

/// The refs side of the repo as the last known state left it. Branches come
/// from the most recently updated state, each worktree's commit from its
/// own; `remotes` and the `HEAD` log sizes are copied from `current`, so no
/// `push` or `reset` is invented. `None` without any known state.
pub(super) fn prior_view(known: &[(PathBuf, KnownState)], current: &RefsView) -> Option<RefsView> {
    let latest = known.iter().max_by_key(|(_, state)| state.updated_ms)?;
    let mut view = RefsView {
        remotes: current.remotes.clone(),
        ..RefsView::default()
    };
    for line in latest.1.refs.lines() {
        if let Some((name, commit)) = line.rsplit_once(' ') {
            view.branches.insert(name.to_owned(), commit.to_owned());
        }
    }
    for (path, state) in known {
        let now = current.worktrees.get(path);
        view.worktrees.insert(
            path.clone(),
            WorktreeRefs {
                main: now.is_some_and(|w| w.main),
                admin: now.and_then(|w| w.admin.clone()),
                branch: None,
                commit: state.head.clone(),
                head_reflog: None,
                head_log_len: now.map_or(0, |w| w.head_log_len),
                operating_on: None,
                fingerprint: state.dirty_fingerprint.clone().unwrap_or_default(),
            },
        );
    }
    Some(view)
}

/// A `reconciled` event for each worktree whose changes differ from the last
/// known state, or whose `HEAD` moved with no classified event in it
/// (US-GRP-002 D10).
fn reconciled_events(
    known: &[(PathBuf, KnownState)],
    read: &RepoRead,
    events: &[RawEvent],
    now: (i64, i32),
) -> Vec<RawEvent> {
    let mut out = Vec::new();
    for worktree in &read.worktrees {
        if !matches!(worktree.view.status, WorktreeStatus::Ready { .. }) {
            continue;
        }
        let path = PathBuf::from(worktree.view.path.raw());
        let Some((_, state)) = known.iter().find(|(p, _)| *p == path) else {
            continue;
        };
        let changed = state.dirty_fingerprint != worktree.fingerprint;
        let head_moved = state.head != worktree.head_commit;
        let explained = events.iter().any(|e| e.worktree == path);
        if changed || (head_moved && !explained) {
            out.push(RawEvent {
                worktree: path,
                kind: GitEventKind::Reconciled,
                details: GitEventDetails::default(),
                observed_ms: now.0,
                offset_s: now.1,
            });
        }
    }
    out
}

/// Writes the gap, what was found in it and the new last known state in one
/// batch, in the order the foreign keys need: the gap, its close, the
/// worktrees, the events, "observed until". Returns `false` when the batch
/// could not be written: the caller leaves the repo unobserved (D8).
pub(super) fn record_gap(
    store: &mut RepoStore,
    read: &RepoRead,
    plan: &GapPlan,
    logger: &Logger,
    repo_id: &str,
) -> bool {
    let (now, offset_s) = (now_ms(), gitraptor_git::local_utc_offset_s());
    let known = known_states(store);
    let mut collapsed = 0u32;
    let mut events = Vec::new();
    if !known.is_empty() {
        let Ok(current) = RefsView::read(&read.common_dir) else {
            logger.error(
                "startup_gap_failed",
                &[("repo", Field::id(repo_id)), ("kind", "refs".into())],
            );
            return false;
        };
        if let Some(before) = prior_view(&known, &current) {
            let found = classify_gap(&read.common_dir, &before, &current, (now, offset_s));
            collapsed = found.collapsed;
            events = found.events;
        }
    }
    let extra = reconciled_events(&known, read, &events, (now, offset_s));
    events.extend(extra);

    let gap_id = format!("{}-{}", plan.cause.as_str(), run_id());
    let known_paths: Vec<PathBuf> = known.iter().map(|(path, _)| path.clone()).collect();
    let mut ops = vec![
        WriteOp::OpenGap {
            gap_id: gap_id.clone(),
            started_ms: plan.started_ms.min(now),
            cause: plan.cause,
            requested_by: plan.requested_by.clone(),
        },
        WriteOp::CloseGap {
            gap_id: gap_id.clone(),
            ended_ms: now,
        },
    ];
    ops.extend(read.store_ops(&known_paths, now));
    // An event whose worktree the store does not know yet breaks the foreign key.
    for event in &events {
        if !known_paths.contains(&event.worktree) && !has_upsert(&ops, &event.worktree) {
            ops.push(WriteOp::UpsertWorktree {
                path: event.worktree.clone(),
                admin_name: None,
                seen_ms: now,
            });
        }
    }
    let found = events.len();
    for event in events {
        ops.push(WriteOp::AppendEvent(NewEvent {
            worktree: event.worktree,
            kind: event.kind.as_str().to_owned(),
            metadata: serde_json::to_string(&event.details).unwrap_or_default(),
            observed: Timestamp {
                utc_ms: event.observed_ms,
                offset_s: event.offset_s,
            },
            session_id: None,
            evidence: None,
            gap_id: Some(gap_id.clone()),
            authorship: None,
        }));
    }
    ops.push(WriteOp::SetObservedUntil { ms: now });

    match store.write_batch(&ops) {
        Ok(_) => {
            logger.info(
                "startup_gap",
                &[
                    ("repo", Field::id(repo_id)),
                    ("cause", plan.cause.as_str().into()),
                    ("events", found.into()),
                    ("collapsed", collapsed.into()),
                ],
            );
            true
        }
        Err(err) => {
            logger.error(
                "startup_gap_failed",
                &[
                    ("repo", Field::id(repo_id)),
                    ("kind", profile_error_kind(&err).into()),
                ],
            );
            false
        }
    }
}

fn has_upsert(ops: &[WriteOp], path: &Path) -> bool {
    ops.iter()
        .any(|op| matches!(op, WriteOp::UpsertWorktree { path: p, .. } if p == path))
}

/// The view of a cause. A `match` without a catch-all: a new cause must be
/// named here.
pub(super) fn cause_view(cause: GapCause) -> GapCauseView {
    match cause {
        GapCause::MachineOff => GapCauseView::MachineOff,
        GapCause::DaemonDown => GapCauseView::DaemonDown,
        GapCause::DaemonDownDuringSession => GapCauseView::DaemonDownDuringSession,
        GapCause::DaemonStopped => GapCauseView::DaemonStopped,
        GapCause::RepoRetired => GapCauseView::RepoRetired,
        GapCause::GitUnavailable => GapCauseView::GitUnavailable,
        GapCause::ProfileLost => GapCauseView::ProfileLost,
        GapCause::StoreCorrupt => GapCauseView::StoreCorrupt,
        GapCause::WatcherOverflow => GapCauseView::WatcherOverflow,
        GapCause::StreamRecreated => GapCauseView::StreamRecreated,
        GapCause::PeriodicReconciliation => GapCauseView::PeriodicReconciliation,
        GapCause::Dormant => GapCauseView::Dormant,
    }
}

/// Whether an agent session was present when the engine stopped (SEC-13).
/// Only the causes that stop the engine can say so; the observer's own gaps
/// are never "during a session". A session counts when it started before the
/// gap and did not end before it: its end, or without one the time of its
/// last `session-*` event (a session with neither is still open), is not earlier than the
/// start of the gap.
pub(super) fn during_session(gap: &Gap, sessions: &[SessionWithState]) -> bool {
    match gap.cause {
        GapCause::DaemonDownDuringSession => true,
        GapCause::DaemonDown | GapCause::DaemonStopped => sessions.iter().any(|(session, last)| {
            // No end and no `session-*` event after its start: it is still open.
            let last_seen = session
                .ended_ms
                .or_else(|| last.as_ref().map(|(_, ms)| *ms))
                .unwrap_or(i64::MAX);
            session.started_ms < gap.started_ms && last_seen >= gap.started_ms
        }),
        _ => false,
    }
}

pub(super) fn gap_view(gap: &Gap, sessions: &[SessionWithState]) -> GapView {
    GapView {
        gap_id: gap.gap_id.clone(),
        started_utc_ms: gap.started_ms,
        ended_utc_ms: gap.ended_ms,
        cause: cause_view(gap.cause),
        during_session: during_session(gap, sessions),
    }
}

impl Daemon {
    /// One page of a repo's observation gaps (US-GRP-005).
    pub(super) fn gap_history(
        &self,
        params: &EventsGapsParams,
    ) -> Result<Vec<GapView>, RepoCommandError> {
        let (_, store) = self
            .stores
            .iter()
            .find(|(id, _)| *id == params.repo_id)
            .ok_or(RepoCommandError::UnknownRepo)?;
        let limit = params
            .limit
            .unwrap_or(MAX_GAPS_PAGE)
            .clamp(1, MAX_GAPS_PAGE);
        let gaps = store
            .gaps_page(params.since_utc_ms, limit)
            .map_err(|err| self.repo_command_failed("events_gaps_failed", &err))?;
        let sessions = store
            .sessions_with_state()
            .map_err(|err| self.repo_command_failed("events_gaps_failed", &err))?;
        Ok(gaps.iter().map(|gap| gap_view(gap, &sessions)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::LogLimits;
    use crate::observe;
    use crate::profile::{Agent, AgentKind, Origin, Profile, ProfileDirs, Session};
    use gitraptor_testkit::Fixture;

    struct Env {
        fx: Fixture,
        _root: tempfile::TempDir,
        store: RepoStore,
        common: PathBuf,
    }

    fn env() -> Env {
        let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
        let root = tempfile::tempdir().unwrap();
        let (mut profile, _) =
            Profile::open(ProfileDirs::under_root(root.path().join("profile"))).unwrap();
        let (entry, _) = profile.add_repo(&fx.repo.join(".git"), None, 1).unwrap();
        let (store, _) = profile.open_store(&entry.repo_id).unwrap();
        Env {
            fx,
            _root: root,
            store,
            common: entry.canonical_path,
        }
    }

    fn read(env: &Env) -> RepoRead {
        observe::reconcile(&env.common, &observe::base_branch(None)).unwrap()
    }

    fn logger() -> (tempfile::TempDir, Logger) {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::open(dir.path(), LogLimits::default()).unwrap();
        (dir, logger)
    }

    fn state(head: &str, refs: &str, updated_ms: i64) -> KnownState {
        KnownState {
            head: Some(head.into()),
            refs: refs.into(),
            operation: None,
            dirty_fingerprint: None,
            updated_ms,
        }
    }

    fn pending(from_ms: Option<i64>) -> PendingGap {
        PendingGap {
            from_ms,
            cause: GapCause::DaemonDown,
            requested_by: None,
        }
    }

    #[test]
    fn the_prior_view_takes_refs_from_the_latest_known_state_and_remotes_from_now() {
        let mut current = RefsView::default();
        current.remotes.insert("origin/main".into(), "r1".into());
        let known = vec![
            (PathBuf::from("/a"), state("h1", "main a1\nold b1", 5)),
            (PathBuf::from("/b"), state("h2", "main a2", 9)),
        ];
        let view = prior_view(&known, &current).unwrap();
        assert_eq!(view.branches.len(), 1);
        assert_eq!(view.branches["main"], "a2");
        assert_eq!(view.remotes, current.remotes);
        assert_eq!(
            view.worktrees[&PathBuf::from("/a")].commit.as_deref(),
            Some("h1")
        );
        assert_eq!(
            view.worktrees[&PathBuf::from("/b")].commit.as_deref(),
            Some("h2")
        );
    }

    #[test]
    fn without_known_state_there_is_no_prior_view() {
        assert!(prior_view(&[], &RefsView::default()).is_none());
    }

    #[test]
    fn gone_worktrees_are_not_in_the_prior_view() {
        let mut env = env();
        let dir = env._root.path();
        let (kept, gone) = (dir.join("kept"), dir.join("gone"));
        let mut ops = Vec::new();
        for (path, refs) in [(&kept, "main a1"), (&gone, "main a2")] {
            ops.push(WriteOp::UpsertWorktree {
                path: path.clone(),
                admin_name: None,
                seen_ms: 1,
            });
            ops.push(WriteOp::SetLastKnownState {
                worktree: path.clone(),
                state: state("h", refs, 3),
            });
        }
        ops.push(WriteOp::MarkWorktreeGone {
            path: gone.clone(),
            gone_ms: 4,
        });
        env.store.write_batch(&ops).unwrap();
        let known = known_states(&env.store);
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].0, kept);
        let view = prior_view(&known, &RefsView::default()).unwrap();
        assert!(!view.worktrees.contains_key(&gone));
    }

    #[test]
    fn a_recovered_store_plans_a_store_corrupt_gap_from_the_add_date() {
        let env = env();
        let plan = startup_plan(
            &env.store,
            Some(&pending(Some(900))),
            &StoreOpen::Recovered {
                quarantined: PathBuf::from("q"),
            },
            42,
        )
        .unwrap();
        assert_eq!(plan.cause, GapCause::StoreCorrupt);
        assert_eq!(plan.started_ms, 42);
        assert!(startup_plan(&env.store, None, &StoreOpen::Existing, 42).is_none());
    }

    #[test]
    fn the_gap_starts_at_observed_until_then_known_state_then_add_date() {
        let mut env = env();
        let existing = StoreOpen::Existing;
        let from = |env: &Env, p: Option<i64>| {
            startup_plan(&env.store, Some(&pending(p)), &existing, 42)
                .unwrap()
                .started_ms
        };
        assert_eq!(from(&env, Some(900)), 900);
        assert_eq!(from(&env, None), 42);
        let wt = env._root.path().join("w");
        env.store
            .write_batch(&[
                WriteOp::UpsertWorktree {
                    path: wt.clone(),
                    admin_name: None,
                    seen_ms: 1,
                },
                WriteOp::SetLastKnownState {
                    worktree: wt,
                    state: state("h", "main a", 77),
                },
            ])
            .unwrap();
        assert_eq!(from(&env, None), 77);
        assert_eq!(from(&env, Some(900)), 900);
    }

    #[test]
    fn a_reconciled_event_only_where_no_git_event_explains_the_change() {
        let env = env();
        let read = read(&env);
        let w = &read.worktrees[0];
        let path = PathBuf::from(w.view.path.raw());
        let known = |head: Option<String>, fp: Option<String>| {
            vec![(
                path.clone(),
                KnownState {
                    head,
                    refs: String::new(),
                    operation: None,
                    dirty_fingerprint: fp,
                    updated_ms: 1,
                },
            )]
        };
        let found = |known: &[(PathBuf, KnownState)], events: &[RawEvent]| {
            reconciled_events(known, &read, events, (10, 0)).len()
        };
        let same = known(w.head_commit.clone(), w.fingerprint.clone());
        assert_eq!(found(&same, &[]), 0);
        let other_fp = known(w.head_commit.clone(), Some("different".into()));
        assert_eq!(found(&other_fp, &[]), 1);
        let moved = known(Some("0".repeat(40)), w.fingerprint.clone());
        assert_eq!(found(&moved, &[]), 1);
        let explained = [RawEvent {
            worktree: path,
            kind: GitEventKind::Commit,
            details: GitEventDetails::default(),
            observed_ms: 10,
            offset_s: 0,
        }];
        assert_eq!(found(&moved, &explained), 0);
    }

    #[test]
    fn every_gap_cause_has_a_view() {
        for cause in [
            GapCause::MachineOff,
            GapCause::DaemonDown,
            GapCause::DaemonDownDuringSession,
            GapCause::DaemonStopped,
            GapCause::RepoRetired,
            GapCause::GitUnavailable,
            GapCause::ProfileLost,
            GapCause::StoreCorrupt,
            GapCause::WatcherOverflow,
            GapCause::StreamRecreated,
            GapCause::PeriodicReconciliation,
            GapCause::Dormant,
        ] {
            let text = serde_json::to_value(cause_view(cause)).unwrap();
            assert_eq!(text, cause.as_str());
        }
    }

    #[test]
    fn a_failed_gap_write_leaves_the_repo_unobserved() {
        let mut env = env();
        let read = read(&env);
        let (_dir, logger) = logger();
        env.store.make_read_only();
        let plan = GapPlan {
            cause: GapCause::DaemonDown,
            started_ms: 1,
            requested_by: None,
        };
        assert!(!record_gap(&mut env.store, &read, &plan, &logger, "abcd"));
        assert!(env.store.gaps().unwrap().is_empty());
        assert_eq!(env.store.observed_until().unwrap(), None);
        let _ = &env.fx;
    }

    fn session(started_ms: i64, ended_ms: Option<i64>) -> Session {
        Session {
            session_id: "s".into(),
            worktree: PathBuf::from("/w"),
            agent: Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
            },
            initial_origin: Origin::Registered,
            detection_key: None,
            started_ms,
            ended_ms,
            end_cause: None,
        }
    }

    fn gap(cause: GapCause, started_ms: i64) -> Gap {
        Gap {
            gap_id: "g".into(),
            started_ms,
            ended_ms: Some(started_ms + 10),
            cause,
            requested_by: None,
        }
    }

    #[test]
    fn during_session_covers_a_session_present_at_the_gap_start() {
        let open = [(session(5, None), None)];
        assert!(during_session(&gap(GapCause::DaemonStopped, 100), &open));
        assert!(during_session(&gap(GapCause::DaemonDown, 100), &open));
        let ended_after = [(session(5, Some(150)), None)];
        assert!(during_session(
            &gap(GapCause::DaemonStopped, 100),
            &ended_after
        ));
        assert!(during_session(
            &gap(GapCause::DaemonDownDuringSession, 100),
            &[]
        ));
        let started_later = [(session(120, None), None)];
        assert!(!during_session(
            &gap(GapCause::DaemonStopped, 100),
            &started_later
        ));
    }

    #[test]
    fn a_session_ended_in_an_earlier_gap_does_not_mark_a_later_one() {
        let ended = [(session(5, Some(50)), None)];
        assert!(!during_session(&gap(GapCause::DaemonStopped, 100), &ended));
        let unknown_end = [(session(5, None), Some(("session-end".to_owned(), 60)))];
        assert!(!during_session(
            &gap(GapCause::DaemonDown, 100),
            &unknown_end
        ));
    }

    #[test]
    fn an_observer_gap_is_never_during_session() {
        let open = [(session(5, None), None)];
        for cause in [
            GapCause::WatcherOverflow,
            GapCause::StreamRecreated,
            GapCause::PeriodicReconciliation,
            GapCause::Dormant,
            GapCause::StoreCorrupt,
            GapCause::MachineOff,
        ] {
            assert!(!during_session(&gap(cause, 100), &open), "{cause:?}");
        }
    }
}
