//! Agent sessions in the daemon loop (US-GRP-007, ADR-GRP-012, ADR-GRP-013).
//!
//! The detector finds Claude Code sessions and their state; this loop, the
//! single writer, persists every change as a session row plus a `session-*`
//! event of the repo's history, then publishes `session.state`. It also
//! asks the detector, for each Git event it persists, whether S3 points to
//! one session.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitraptor_api::event::SESSION_STATE;
use gitraptor_api::messages::{
    GitEventKind, MAX_SESSIONS_PAGE, SessionEndCauseView, SessionStateView, SessionView,
    SessionsListParams,
};
use gitraptor_api::{Actor, AgentKind as ApiAgentKind, AgentOrigin, Timings, Untrusted, clock};

use super::{Daemon, Field, RepoCommandError, env, now_ms, profile_error_kind};
use crate::detect::{
    Detector, OpenSession, PresentSession, S3Outcome, SessionChange, SessionConfig,
    SystemProcLister, detection_supported,
};
use crate::observe::RepoRead;
use crate::profile::{
    Agent, AgentKind, EndCause, NewEvent, Origin, RepoStore, Session, Timestamp, WriteOp,
};
use crate::watch::{ObservedBatch, ObserverHooks};

/// Evidence stored with an event that S3 attributed (ADR-GRP-013 § 1).
const S3_EVIDENCE: &str = r#"{"signals":["s3"]}"#;

impl Daemon {
    /// Starts the detector on first use; the observer gets its hooks.
    pub(super) fn detector_hooks(&mut self) -> Arc<dyn ObserverHooks> {
        let detector = self.detector.get_or_insert_with(|| {
            let handle = self.handle.clone();
            let skew = env::clock_skew_file();
            let clock =
                Arc::new(move || now_ms() + skew.as_deref().map_or(0, env::read_clock_skew));
            Detector::start(
                SessionConfig::default(),
                self.config.channel.agents.clone(),
                Arc::new(SystemProcLister),
                clock,
                Arc::new(move |changes| {
                    handle.sessions(changes);
                }),
            )
        });
        if !detection_supported() {
            self.logger.warn(
                "session_detection_unavailable",
                &[("os", std::env::consts::OS.into())],
            );
        }
        detector.observer_hooks()
    }

    /// Starts detecting in a repo and closes, as ended during a gap, the
    /// sessions whose process died while the repo was not observed.
    pub(super) fn detect_repo(&mut self, repo_id: &str, common_dir: &Path, read: &RepoRead) {
        let Some(detector) = &self.detector else {
            return;
        };
        let Some((_, store)) = self.stores.iter().find(|(id, _)| id == repo_id) else {
            return;
        };
        let open: Vec<OpenSession> = store
            .sessions_with_state()
            .unwrap_or_default()
            .into_iter()
            .filter(|(s, _)| s.end_cause.is_none() && is_detected(s))
            .map(|(s, state)| OpenSession {
                state: state_of(&s, state.as_ref()).0,
                session_id: s.session_id,
                worktree: s.worktree,
                started_ms: s.started_ms,
            })
            .collect();
        let worktrees = read
            .worktrees
            .iter()
            .map(|w| PathBuf::from(w.view.path.raw()))
            .collect();
        let common = crate::observe::canonical(common_dir);
        let dead = detector.watch_repo(repo_id, &common, worktrees, open);
        let changes = dead
            .into_iter()
            .map(|session_id| SessionChange::Ended {
                repo_id: repo_id.to_owned(),
                session_id,
                at_ms: None,
                cause: EndCause::EndedDuringGap,
            })
            .collect();
        self.sessions_changed(changes);
    }

    /// Persists what the detector saw, then publishes it.
    pub(super) fn sessions_changed(&mut self, changes: Vec<SessionChange>) {
        let t_recv = clock::monotonic_ns();
        let offset = crate::watch::wall_now().1;
        let mut published: Vec<(String, String)> = Vec::new();
        for change in changes {
            let (repo_id, session_id) = match &change {
                SessionChange::Started {
                    repo_id,
                    session_id,
                    ..
                }
                | SessionChange::State {
                    repo_id,
                    session_id,
                    ..
                }
                | SessionChange::Ended {
                    repo_id,
                    session_id,
                    ..
                } => (repo_id.clone(), session_id.clone()),
            };
            let Some((_, store)) = self.stores.iter_mut().find(|(id, _)| *id == repo_id) else {
                continue;
            };
            let ops = match change {
                SessionChange::Started {
                    worktree,
                    started_ms,
                    ..
                } => {
                    let mut ops = start_ops(
                        store,
                        &PresentSession {
                            session_id: session_id.clone(),
                            worktree: worktree.clone(),
                            started_ms,
                        },
                    );
                    ops.push(state_event(
                        worktree,
                        "session-start",
                        started_ms,
                        offset,
                        &session_id,
                    ));
                    self.logger.info(
                        "session_detected",
                        &[
                            ("repo", Field::id(&repo_id)),
                            ("agent", "claude-code".into()),
                        ],
                    );
                    ops
                }
                SessionChange::State { state, at_ms, .. } => {
                    let Some(session) = store.session(&session_id).ok().flatten() else {
                        continue;
                    };
                    let kind = match state {
                        SessionStateView::Inactive => "session-inactive",
                        _ => "session-active",
                    };
                    vec![state_event(
                        session.worktree,
                        kind,
                        at_ms,
                        offset,
                        &session_id,
                    )]
                }
                SessionChange::Ended { at_ms, cause, .. } => {
                    let Some(session) = store.session(&session_id).ok().flatten() else {
                        continue;
                    };
                    self.logger.info(
                        "session_ended",
                        &[
                            ("repo", Field::id(&repo_id)),
                            ("cause", cause.as_str().into()),
                        ],
                    );
                    vec![
                        WriteOp::EndSession {
                            session_id: session_id.clone(),
                            ended_ms: at_ms,
                            cause,
                        },
                        state_event(
                            session.worktree,
                            "session-end",
                            at_ms.unwrap_or_else(now_ms),
                            offset,
                            &session_id,
                        ),
                    ]
                }
            };
            if let Err(err) = store.write_batch(&ops) {
                self.logger.error(
                    "session_persist_failed",
                    &[
                        ("repo", Field::id(&repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                );
                continue;
            }
            published.push((repo_id, session_id));
        }
        let t_persisted = clock::monotonic_ns();
        for (repo_id, session_id) in published {
            let Some((_, store)) = self.stores.iter().find(|(id, _)| *id == repo_id) else {
                continue;
            };
            let Some(view) = store
                .sessions_with_state()
                .ok()
                .into_iter()
                .flatten()
                .find(|(s, _)| s.session_id == session_id)
                .map(|(s, state)| session_view(&repo_id, &s, state.as_ref()))
            else {
                continue;
            };
            let timings = Timings {
                batch_id: self.bus.next_batch(),
                t_recv,
                t_flush: t_recv,
                t_computed: t_recv,
                t_persisted,
                t_published: 0,
            };
            self.bus.publish(SESSION_STATE, view, Some(timings), |_| {});
        }
    }

    /// For each event of `batch`, the session S3 points to (ADR-GRP-012,
    /// rules 2 to 6), and the activity it means for its worktree. The
    /// outcome goes to the diagnostic log (SPIKE-GRP-001).
    pub(super) fn attribute(&self, batch: &ObservedBatch) -> Vec<Option<PresentSession>> {
        let Some(detector) = &self.detector else {
            return vec![None; batch.events.len()];
        };
        if !batch.worktrees.is_empty() || !batch.gone.is_empty() {
            detector.set_worktrees(
                &batch.repo_id,
                self.observer
                    .as_ref()
                    .map(|o| o.worktrees(&batch.repo_id))
                    .unwrap_or_default(),
            );
        }
        batch
            .events
            .iter()
            .map(|event| {
                detector.activity(&batch.repo_id, &event.worktree);
                // A reconciliation never has a session (BR-EDGE-005).
                if event.kind == GitEventKind::Reconciled {
                    return None;
                }
                let outcome = detector.evidence(
                    &batch.repo_id,
                    &event.worktree,
                    batch.marks.t_recv,
                    batch.marks.t_flush,
                );
                let label = match &outcome {
                    S3Outcome::NoSession => return None,
                    S3Outcome::NoSighting => "no-sighting",
                    S3Outcome::Ambiguous => "ambiguous",
                    S3Outcome::Attributed(_) => "attributed",
                };
                self.logger.info(
                    "s3_evidence",
                    &[
                        ("repo", Field::id(&batch.repo_id)),
                        ("event", event.kind.as_str().into()),
                        ("outcome", label.into()),
                        (
                            "samples",
                            i64::try_from(detector.diagnostics().samples)
                                .unwrap_or(i64::MAX)
                                .into(),
                        ),
                    ],
                );
                match outcome {
                    S3Outcome::Attributed(p) => Some(p),
                    _ => None,
                }
            })
            .collect()
    }

    /// `sessions.list`: the sessions of the observed repos.
    pub(super) fn sessions_list(
        &self,
        params: &SessionsListParams,
    ) -> Result<(bool, Vec<SessionView>), RepoCommandError> {
        if let Some(id) = &params.repo_id
            && !self.stores.iter().any(|(r, _)| r == id)
        {
            return Err(RepoCommandError::UnknownRepo);
        }
        let mut views = Vec::new();
        for (repo_id, store) in &self.stores {
            if params.repo_id.as_ref().is_some_and(|id| id != repo_id) {
                continue;
            }
            let all = store
                .sessions_with_state()
                .map_err(|err| self.repo_command_failed("sessions_list_failed", &err))?;
            let latest_ended = |s: &Session| {
                !all.iter().any(|(o, _)| {
                    o.worktree == s.worktree
                        && o.end_cause.is_some()
                        && (o.started_ms, &o.session_id) > (s.started_ms, &s.session_id)
                })
            };
            views.extend(
                all.iter()
                    .filter(|(s, _)| {
                        params.include_ended || s.end_cause.is_none() || latest_ended(s)
                    })
                    .map(|(s, state)| session_view(repo_id, s, state.as_ref())),
            );
        }
        views.sort_by_key(|v| v.started_utc_ms);
        let limit = params
            .limit
            .unwrap_or(MAX_SESSIONS_PAGE)
            .min(MAX_SESSIONS_PAGE) as usize;
        let skip = views.len().saturating_sub(limit);
        Ok((detection_supported(), views.split_off(skip)))
    }
}

/// Operations that create a detected session, if the store lacks it.
pub(super) fn start_ops(store: &RepoStore, session: &PresentSession) -> Vec<WriteOp> {
    if store.session(&session.session_id).ok().flatten().is_some() {
        return Vec::new();
    }
    let mut ops = Vec::new();
    let known = store
        .worktrees()
        .map(|all| all.iter().any(|w| w.path == session.worktree))
        .unwrap_or(false);
    if !known {
        ops.push(WriteOp::UpsertWorktree {
            path: session.worktree.clone(),
            admin_name: None,
            seen_ms: session.started_ms,
        });
    }
    ops.push(WriteOp::StartSession {
        session_id: session.session_id.clone(),
        worktree: session.worktree.clone(),
        agent: Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
        },
        origin: Origin::Detected,
        detection_key: Some(session.session_id.clone()),
        started_ms: session.started_ms,
    });
    ops
}

/// The evidence text of an event S3 attributed.
pub(super) fn s3_evidence() -> Option<String> {
    Some(S3_EVIDENCE.to_owned())
}

fn state_event(
    worktree: PathBuf,
    kind: &str,
    at_ms: i64,
    offset_s: i32,
    session_id: &str,
) -> WriteOp {
    WriteOp::AppendEvent(NewEvent {
        worktree,
        kind: kind.to_owned(),
        metadata: "{}".to_owned(),
        observed: Timestamp {
            utc_ms: at_ms,
            offset_s,
        },
        session_id: Some(session_id.to_owned()),
        evidence: None,
        gap_id: None,
    })
}

/// A session the detector manages: detected, not registered.
fn is_detected(session: &Session) -> bool {
    session.agent.kind == AgentKind::ClaudeCode
        && session.initial_origin == Origin::Detected
        && session.detection_key.is_some()
}

/// State of a session and since when, from its end and its latest
/// `session-*` event.
fn state_of(session: &Session, latest: Option<&(String, i64)>) -> (SessionStateView, i64) {
    if session.end_cause.is_some() {
        let since = session
            .ended_ms
            .or(latest.map(|(_, at)| *at))
            .unwrap_or(session.started_ms);
        return (SessionStateView::Ended, since);
    }
    match latest {
        Some((kind, at)) if kind == "session-inactive" => (SessionStateView::Inactive, *at),
        Some((_, at)) => (SessionStateView::Active, *at),
        None => (SessionStateView::Active, session.started_ms),
    }
}

/// Effective attribution of a session: its initial one until the records
/// of US-GRP-009 and US-GRP-010 exist.
pub(super) fn session_actor(session: &Session) -> Actor {
    Actor::Agent {
        kind: match session.agent.kind {
            AgentKind::ClaudeCode => ApiAgentKind::ClaudeCode,
            AgentKind::Other => ApiAgentKind::Other,
        },
        name: session.agent.name.clone().map(Untrusted::new),
        origin: match session.initial_origin {
            Origin::Detected => AgentOrigin::Detected,
            Origin::Registered => AgentOrigin::Registered,
        },
    }
}

fn session_view(repo_id: &str, session: &Session, latest: Option<&(String, i64)>) -> SessionView {
    let (state, since) = state_of(session, latest);
    SessionView {
        repo_id: repo_id.to_owned(),
        session_id: session.session_id.clone(),
        worktree: Untrusted::from_os(session.worktree.as_os_str()),
        actor: session_actor(session),
        state,
        started_utc_ms: session.started_ms,
        state_since_utc_ms: since,
        utc_offset_s: crate::watch::wall_now().1,
        ended_utc_ms: session.ended_ms,
        end_cause: session.end_cause.map(|c| match c {
            EndCause::ProcessGone => SessionEndCauseView::ProcessGone,
            EndCause::EndedDuringGap => SessionEndCauseView::EndedDuringGap,
            EndCause::RegistrationWithdrawn => SessionEndCauseView::RegistrationWithdrawn,
        }),
    }
}
