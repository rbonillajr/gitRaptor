//! The observed repos: adding and retiring them, their observation and
//! what it publishes (US-GRP-001, US-GRP-002).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gitraptor_api::event::{GIT_EVENT, REPO_OBSERVATION, WORKTREE_STATE};
use gitraptor_api::messages::{
    EventsHistoryParams, GitEventKind, GitEventView, MAX_HISTORY_PAGE, RepoAddOutcome,
    RepoAddResult, RepoObservationData, RepoRetireResult, RepoStateView, RepoView,
    WorktreeStateData, WorktreeStatus, WorktreeView,
};
use gitraptor_api::{Timings, Untrusted, clock};

use super::state::{EngineState, Trigger};
use super::{
    CHANGE_LIST_BUDGET, Daemon, Field, TM_RECOVERY_WAIT, git_event_view, now_ms, outcome_field,
    persist_read, profile_error_kind, recover_repo, repo_base, run_id, seed_activity, sessions,
};
use super::{RepoAddRequest, RepoCommandError};

use crate::observe::{self, RepoRead};
use crate::profile::{AddOutcome, GapCause, KnownState, NewEvent, RepoState, Timestamp, WriteOp};
use crate::watch::{ObservedBatch, Observer, WatchConfig};

impl Daemon {
    /// Adds a repo the channel located and read (US-GRP-001): registers it
    /// in the profile, opens its store, persists the reconciliation and
    /// publishes it. The loop re-checks the profile, so two adds of the same
    /// repo end with one entry.
    pub(super) fn add_repo(
        &mut self,
        mut request: RepoAddRequest,
    ) -> Result<RepoAddResult, RepoCommandError> {
        let now = now_ms();
        let (entry, outcome) = self
            .profile
            .add_repo(&request.common_dir, None, now)
            .map_err(|err| self.repo_command_failed("repo_add_failed", &err))?;
        let outcome = match outcome {
            AddOutcome::New => RepoAddOutcome::New,
            AddOutcome::AlreadyObserved => RepoAddOutcome::AlreadyObserved,
            AddOutcome::Reactivated { .. } => RepoAddOutcome::Reactivated,
        };
        let repo_id = entry.repo_id.clone();
        let path = Untrusted::from_os(entry.canonical_path.as_os_str());
        self.logger.info(
            "repo_added",
            &[
                ("repo", Field::id(&repo_id)),
                ("outcome", outcome_field(outcome).into()),
            ],
        );
        // Its Time Machine, recovered like at startup, so that operations on
        // it are protected from now on (US-TMC-001).
        if !self.tm.contains(&repo_id) {
            match recover_repo(&self.config.dirs, &entry, Instant::now() + TM_RECOVERY_WAIT) {
                Ok((oplog, _)) => self.tm.insert(&repo_id, &entry.canonical_path, oplog),
                Err(err) => self.logger.warn(
                    "tm_unavailable",
                    &[
                        ("repo", Field::id(&repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                ),
            }
        }
        // Without Git the repo is registered but nothing is observed
        // (BR-WF-002).
        if self.state == EngineState::WaitingForGit {
            return Ok(RepoAddResult {
                outcome,
                repo: RepoView {
                    repo_id,
                    state: RepoStateView::Observed,
                    path,
                    base: observe::base_view(&observe::base_branch(None)),
                    worktrees: Vec::new(),
                    fetched_utc_ms: None,
                },
            });
        }
        if !self.stores.iter().any(|(id, _)| *id == repo_id) {
            match self.profile.open_store(&repo_id) {
                Ok((store, _)) => self.stores.push((repo_id.clone(), store)),
                Err(err) => self.logger.warn(
                    "repo_unavailable",
                    &[
                        ("repo", Field::id(&repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                ),
            }
        }
        // The channel counted against the default; the store may keep a
        // confirmed base branch (US-GRP-012).
        request.read.set_base(repo_base(&self.stores, &repo_id));
        let state = match self.stores.iter_mut().find(|(id, _)| *id == repo_id) {
            Some((_, store)) => {
                persist_read(store, &request.read, &self.logger, &repo_id);
                RepoStateView::Observed
            }
            None => RepoStateView::Unavailable,
        };
        if state == RepoStateView::Observed {
            self.link_marks(&repo_id);
            self.observe(&repo_id, &request.common_dir, &request.read);
        }
        let t_persisted = clock::monotonic_ns();
        let mut worktrees = request.read.views();
        seed_activity(&self.stores, &repo_id, &mut worktrees);
        let view = RepoView {
            repo_id: repo_id.clone(),
            state,
            path: path.clone(),
            base: observe::base_view(&request.read.base),
            worktrees: worktrees.clone(),
            fetched_utc_ms: observe::fetched_utc_ms(&request.common_dir, now_ms()),
        };
        if self.state == EngineState::NoRepos {
            self.transition(Trigger::FirstRepoAdded);
        }
        if outcome != RepoAddOutcome::AlreadyObserved {
            let added = view.clone();
            self.bus.publish(
                REPO_OBSERVATION,
                RepoObservationData {
                    repo_id: repo_id.clone(),
                    observed: true,
                    state,
                    path,
                },
                None,
                move |shared| {
                    shared.repos.retain(|r| r.repo_id != added.repo_id);
                    shared.repos.push(added);
                },
            );
        }
        self.publish_worktrees(
            &repo_id,
            &request.read,
            Timings {
                batch_id: self.bus.next_batch(),
                t_recv: request.t_recv,
                t_flush: request.t_recv,
                t_computed: request.t_computed,
                t_persisted,
                t_published: 0,
            },
        );
        Ok(RepoAddResult {
            outcome,
            repo: view,
        })
    }

    /// Publishes the reconciled worktrees of one repo and updates the view
    /// snapshots read, in the same critical section.
    pub(super) fn publish_worktrees(&self, repo_id: &str, read: &RepoRead, timings: Timings) {
        self.publish_views(
            repo_id,
            read.views(),
            read.divergence_inputs(),
            Some(observe::base_view(&read.base)),
            Vec::new(),
            timings,
        );
    }

    /// Publishes the worktree views of one repo with the inputs of their
    /// ahead/behind recount, in the same critical section as the snapshot.
    /// `in_gap` are the worktrees read by the reconciliation of a gap.
    pub(super) fn publish_views(
        &self,
        repo_id: &str,
        worktrees: Vec<WorktreeView>,
        inputs: observe::DivergenceInputs,
        base: Option<gitraptor_api::messages::BaseBranchView>,
        in_gap: Vec<String>,
        timings: Timings,
    ) {
        let id = repo_id.to_owned();
        let fetched = observe::fetched_utc_ms(&inputs.common_dir, now_ms());
        self.bus
            .publish_with(WORKTREE_STATE, Some(timings), move |shared| {
                let mut worktrees = worktrees;
                let old_heads = shared
                    .divergence
                    .get(&id)
                    .map(|i| i.heads.clone())
                    .unwrap_or_default();
                if let Some(repo) = shared.repos.iter_mut().find(|r| r.repo_id == id) {
                    observe::stamp_activity(
                        &repo.worktrees,
                        &old_heads,
                        &mut worktrees,
                        &inputs.heads,
                        now_ms(),
                        &in_gap,
                    );
                    repo.worktrees = worktrees.clone();
                    repo.fetched_utc_ms = fetched;
                    if let Some(base) = base {
                        repo.base = base;
                    }
                    shared.divergence.insert(id.clone(), inputs);
                }
                let mut data = WorktreeStateData {
                    repo_id: id,
                    worktrees,
                    fetched_utc_ms: fetched,
                };
                if serde_json::to_vec(&data).map_or(0, |v| v.len()) > CHANGE_LIST_BUDGET {
                    observe::without_change_lists(&mut data.worktrees);
                }
                data
            });
    }

    /// Stops observing a repo (US-GRP-001). Its store and data are kept
    /// (Q25), with "observed until" set to the retirement time, so US-GRP-005 and
    /// US-GRP-006 can open the gap of the retired interval. Who asked is in
    /// the reserved-command audit.
    pub(super) fn retire_repo(
        &mut self,
        repo_id: &str,
    ) -> Result<RepoRetireResult, RepoCommandError> {
        let entry = self
            .profile
            .repo(repo_id)
            .map_err(|err| self.repo_command_failed("repo_retire_failed", &err))?
            .ok_or(RepoCommandError::UnknownRepo)?;
        if entry.state == RepoState::Retired {
            return Ok(RepoRetireResult { retired: false });
        }
        let now = now_ms();
        self.profile
            .retire_repo(repo_id, now)
            .map_err(|err| self.repo_command_failed("repo_retire_failed", &err))?;
        if let Some(observer) = &self.observer {
            observer.forget_repo(repo_id);
        }
        if let Some(detector) = &self.detector {
            detector.forget_repo(repo_id);
        }
        self.tm.remove(repo_id);
        self.modules.repo_retired(repo_id);
        self.marks.remove(repo_id);
        if let Some(pos) = self.stores.iter().position(|(id, _)| id == repo_id) {
            let (_, mut store) = self.stores.remove(pos);
            if let Err(err) = store.write_batch(&[WriteOp::SetObservedUntil { ms: now }]) {
                self.logger.error(
                    "observed_until_failed",
                    &[
                        ("repo", Field::id(repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                );
            }
        }
        self.logger
            .info("repo_retired", &[("repo", Field::id(repo_id))]);
        let id = repo_id.to_owned();
        self.bus.publish(
            REPO_OBSERVATION,
            RepoObservationData {
                repo_id: id.clone(),
                observed: false,
                state: RepoStateView::Observed,
                path: Untrusted::from_os(entry.canonical_path.as_os_str()),
            },
            None,
            move |shared| {
                shared.repos.retain(|r| r.repo_id != id);
                shared.divergence.remove(&id);
            },
        );
        let observed_left = self
            .profile
            .repos()
            .map(|all| all.iter().any(|r| r.state == RepoState::Observed))
            .unwrap_or(true);
        if !observed_left && self.state == EngineState::Observing {
            self.transition(Trigger::LastRepoRetired);
        }
        Ok(RepoRetireResult { retired: true })
    }

    /// Starts observing a repo, creating the observer on first use. Its
    /// batches come back to this loop as [`Control::Observed`].
    pub(super) fn observe(&mut self, repo_id: &str, common_dir: &std::path::Path, read: &RepoRead) {
        let mut hooks = vec![self.detector_hooks()];
        hooks.extend(self.modules.observer_hooks());
        let hooks: Arc<dyn crate::watch::ObserverHooks> =
            Arc::new(crate::watch::FanoutHooks(hooks));
        let roots = self.resources.roots_counter();
        let observer = self.observer.get_or_insert_with(|| {
            let handle = self.handle.clone();
            Observer::start_counted(
                WatchConfig::default(),
                Arc::new(move |batch| {
                    handle.observed(batch);
                }),
                Some(hooks),
                roots,
            )
        });
        let start = observer.watch_repo(repo_id, common_dir, read);
        self.marks.set_head_logs(repo_id, &start.head_logs);
        self.marks.set_heads(repo_id, &start.heads);
        self.detect_repo(repo_id, common_dir, read);
    }

    /// Persists what the observer saw, then publishes it (ADR-GRP-013:
    /// persisted before published). A batch of a repo no longer observed
    /// is discarded.
    pub(super) fn observed(&mut self, batch: ObservedBatch) {
        if !self.stores.iter().any(|(id, _)| *id == batch.repo_id) {
            return;
        }
        // S3 (US-GRP-007): the session each event points to, if any.
        let attributed = self.attribute(&batch);
        let Some((_, store)) = self.stores.iter_mut().find(|(id, _)| *id == batch.repo_id) else {
            return;
        };
        let now = now_ms();
        let known: Vec<PathBuf> = store
            .worktrees()
            .map(|all| all.into_iter().map(|w| w.path).collect())
            .unwrap_or_default();
        let mut ops = Vec::new();
        for read in &batch.worktrees {
            let path = PathBuf::from(read.view.path.raw());
            if !matches!(read.view.status, WorktreeStatus::Ready { .. }) {
                continue;
            }
            let refs = match &batch.refs {
                Some(refs) => refs.clone(),
                None => store
                    .last_known_state(&path)
                    .ok()
                    .flatten()
                    .map(|k| k.refs)
                    .unwrap_or_default(),
            };
            ops.push(WriteOp::UpsertWorktree {
                path: path.clone(),
                admin_name: read.view.admin_name.as_ref().map(|n| n.raw().to_owned()),
                seen_ms: now,
            });
            ops.push(WriteOp::SetLastKnownState {
                worktree: path,
                state: KnownState {
                    head: read.head_commit.clone(),
                    refs,
                    operation: None,
                    dirty_fingerprint: read.fingerprint.clone(),
                    updated_ms: now,
                },
            });
        }
        // An event's worktree must exist in the store.
        for event in &batch.events {
            let listed = batch
                .worktrees
                .iter()
                .any(|r| r.view.path.raw() == event.worktree.to_string_lossy());
            if !listed
                && !known.contains(&event.worktree)
                && !ops.iter().any(|op| {
                    matches!(op, WriteOp::UpsertWorktree { path, .. } if *path == event.worktree)
                })
            {
                ops.push(WriteOp::UpsertWorktree {
                    path: event.worktree.clone(),
                    admin_name: None,
                    seen_ms: now,
                });
            }
        }
        let gap_id = batch.gap.map(|gap| {
            let id = format!("{}-{}", gap.cause.as_str(), run_id());
            ops.push(WriteOp::OpenGap {
                gap_id: id.clone(),
                started_ms: gap.started_ms,
                cause: gap.cause,
                requested_by: None,
            });
            ops.push(WriteOp::CloseGap {
                gap_id: id.clone(),
                ended_ms: gap.ended_ms,
            });
            id
        });
        // A session S3 found before the detector's start reached this loop
        // is created here; its later start is then a no-op.
        let mut created: Vec<&str> = Vec::new();
        for a in attributed.iter().flatten().filter(|a| !a.inferred) {
            if !created.contains(&a.session.session_id.as_str()) {
                created.push(&a.session.session_id);
                ops.extend(sessions::start_ops(store, &a.session));
            }
        }
        for (event, session) in batch.events.iter().zip(&attributed) {
            ops.push(WriteOp::AppendEvent(NewEvent {
                worktree: event.worktree.clone(),
                kind: event.kind.as_str().to_owned(),
                metadata: serde_json::to_string(&event.details).unwrap_or_default(),
                observed: Timestamp {
                    utc_ms: event.observed_ms,
                    offset_s: event.offset_s,
                },
                // No session without positive evidence (ADR-GRP-013 § 3).
                session_id: session
                    .as_ref()
                    .and_then(|a| a.session_id().map(str::to_owned)),
                evidence: session.as_ref().map(sessions::Attribution::evidence),
                gap_id: if event.kind == GitEventKind::Reconciled {
                    gap_id.clone()
                } else {
                    None
                },
            }));
        }
        for gone in &batch.gone {
            if known.contains(gone) {
                ops.push(WriteOp::MarkWorktreeGone {
                    path: gone.clone(),
                    gone_ms: now,
                });
            }
        }
        ops.push(WriteOp::SetObservedUntil { ms: now });
        let seqs = match store.write_batch(&ops) {
            Ok(result) => {
                // The mark first, then what the engine read: once a worktree's
                // `HEAD` reflog matches, its mark already covers it (US-TMC-004).
                self.marks.set_seq(&batch.repo_id, store.last_seq());
                self.marks.set_head_logs(&batch.repo_id, &batch.head_logs);
                self.marks.set_heads(&batch.repo_id, &batch.heads);
                result.seqs
            }
            Err(err) => {
                self.logger.error(
                    "observed_persist_failed",
                    &[
                        ("repo", Field::id(&batch.repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                );
                Vec::new()
            }
        };
        let t_persisted = clock::monotonic_ns();
        if let Some(gap) = batch.gap {
            if gap.cause == GapCause::PeriodicReconciliation {
                self.periodic_diffs += 1;
            }
            self.logger.warn(
                "observer_gap",
                &[
                    ("repo", Field::id(&batch.repo_id)),
                    ("cause", gap.cause.as_str().into()),
                    (
                        "periodic_diffs",
                        i64::try_from(self.periodic_diffs)
                            .unwrap_or(i64::MAX)
                            .into(),
                    ),
                ],
            );
        }
        let timings = Timings {
            batch_id: self.bus.next_batch(),
            t_recv: batch.marks.t_recv,
            t_flush: batch.marks.t_flush,
            t_computed: batch.marks.t_computed,
            t_persisted,
            t_published: 0,
        };
        let views_changed = !batch.worktrees.is_empty() || !batch.gone.is_empty();
        if views_changed {
            self.publish_observed_views(&batch, timings);
        }
        // Only persisted events are published: they have their sequence.
        for ((event, seq), session) in batch.events.iter().zip(seqs).zip(&attributed) {
            let actor = session
                .as_ref()
                .and_then(|a| {
                    let store = &self.stores.iter().find(|(id, _)| *id == batch.repo_id)?.1;
                    let s = store.session(a.session_id()?).ok().flatten()?;
                    Some(sessions::session_actor(store, &s))
                })
                .unwrap_or(gitraptor_api::Actor::Unattributed);
            let view = GitEventView {
                repo_id: batch.repo_id.clone(),
                seq,
                worktree: Untrusted::from_os(event.worktree.as_os_str()),
                kind: event.kind,
                actor,
                observed_utc_ms: event.observed_ms,
                utc_offset_s: event.offset_s,
                details: event.details.clone(),
                gap_id: (event.kind == GitEventKind::Reconciled)
                    .then(|| gap_id.clone())
                    .flatten(),
                inferred: session.as_ref().filter(|a| a.inferred).map(|a| {
                    gitraptor_api::messages::InferredAgent {
                        kind: gitraptor_api::AgentKind::ClaudeCode,
                        session_id: a.session.session_id.clone(),
                    }
                }),
            };
            self.bus.publish(GIT_EVENT, view, Some(timings), |_| {});
            // After publishing, outside the engine's budget (ADR-TMC-004 § 2).
            self.modules.git_event(&batch.repo_id, &event.worktree, seq);
        }
        // Second phase (ADR-GRP-010 § 4, ADR-GRP-011 § 2): the ahead/behind
        // against the base branch, outside the first event's budget.
        if views_changed || batch.refs.is_some() {
            self.publish_divergence(&batch.repo_id, timings);
        }
    }

    /// First phase of an observed change: the repo's worktree views with
    /// the batch merged in, each changed worktree keeping its previous
    /// ahead/behind until the second phase counts it again. The inputs of
    /// the snapshot's recount follow the new `HEAD`s (US-GRP-012).
    pub(super) fn publish_observed_views(&self, batch: &ObservedBatch, timings: Timings) {
        let shared = self.bus.snapshot().1;
        let old = shared
            .repos
            .iter()
            .find(|r| r.repo_id == batch.repo_id)
            .map(|r| r.worktrees.clone())
            .unwrap_or_default();
        let old_inputs = shared.divergence.get(&batch.repo_id).cloned();
        let mut worktrees: Vec<(WorktreeView, observe::HeadRef)> = old
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                !batch
                    .gone
                    .iter()
                    .any(|g| g.to_string_lossy() == w.path.raw())
            })
            .map(|(i, w)| {
                let head = old_inputs
                    .as_ref()
                    .and_then(|inp| inp.heads.get(i).cloned())
                    .unwrap_or(observe::HeadRef::None);
                (w.clone(), head)
            })
            .collect();
        for read in &batch.worktrees {
            let mut view = read.view.clone();
            let previous = worktrees.iter().position(|(w, _)| w.path == view.path);
            if let Some(WorktreeStatus::Ready { divergence, .. }) =
                previous.map(|i| &worktrees[i].0.status)
            {
                observe::set_divergence(&mut view, divergence.clone());
            }
            match previous {
                Some(i) => worktrees[i] = (view, read.head_ref()),
                None => worktrees.push((view, read.head_ref())),
            }
        }
        worktrees.sort_by(|(a, _), (b, _)| (!a.main, a.path.raw()).cmp(&(!b.main, b.path.raw())));
        let base = old_inputs
            .as_ref()
            .map(|inp| inp.base.clone())
            .unwrap_or_else(|| repo_base(&self.stores, &batch.repo_id));
        let common_dir = old_inputs
            .map(|inp| inp.common_dir)
            .or_else(|| {
                self.profile
                    .repo(&batch.repo_id)
                    .ok()
                    .flatten()
                    .map(|e| e.canonical_path)
            })
            .unwrap_or_default();
        let (views, heads): (Vec<_>, Vec<_>) = worktrees.into_iter().unzip();
        let inputs = observe::DivergenceInputs {
            common_dir,
            base,
            heads,
        };
        let in_gap = batch.gap_worktrees();
        self.publish_views(&batch.repo_id, views, inputs, None, in_gap, timings);
    }

    /// Second phase: counts the ahead/behind of the repo as it is now and
    /// publishes it if it changed.
    pub(super) fn publish_divergence(&self, repo_id: &str, timings: Timings) {
        let shared = self.bus.snapshot().1;
        let (Some(repo), Some(inputs)) = (
            shared.repos.iter().find(|r| r.repo_id == repo_id),
            shared.divergence.get(repo_id),
        ) else {
            return;
        };
        let mut counted = [repo.clone()];
        let map = BTreeMap::from([(repo_id.to_owned(), inputs.clone())]);
        observe::refresh_divergence(&mut counted, &map, &self.divergence_cache);
        let [counted] = counted;
        if counted.worktrees == repo.worktrees {
            return;
        }
        let t_computed = clock::monotonic_ns();
        let timings = Timings {
            t_computed,
            t_persisted: t_computed,
            ..timings
        };
        self.publish_views(
            repo_id,
            counted.worktrees,
            inputs.clone(),
            None,
            Vec::new(),
            timings,
        );
    }

    /// One page of a repo's Git events (US-GRP-002, ADR-GRP-013 § 6).
    pub(super) fn event_history(
        &self,
        params: &EventsHistoryParams,
    ) -> Result<Vec<GitEventView>, RepoCommandError> {
        let (_, store) = self
            .stores
            .iter()
            .find(|(id, _)| *id == params.repo_id)
            .ok_or(RepoCommandError::UnknownRepo)?;
        let limit = params
            .limit
            .unwrap_or(MAX_HISTORY_PAGE)
            .min(MAX_HISTORY_PAGE);
        let events = store
            .events_page(
                params.worktree.as_deref().map(std::path::Path::new),
                params.after_seq,
                limit,
            )
            .map_err(|err| self.repo_command_failed("events_history_failed", &err))?;
        Ok(events
            .into_iter()
            .filter_map(|e| git_event_view(&params.repo_id, store, e))
            .collect())
    }
}
