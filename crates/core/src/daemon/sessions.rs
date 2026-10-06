//! Agent sessions in the daemon loop (US-GRP-007, US-GRP-009, ADR-GRP-012,
//! ADR-GRP-013).
//!
//! The detector finds Claude Code sessions and their state; this loop, the
//! single writer, persists every change as a session row plus a `session-*`
//! event of the repo's history, then publishes `session.state`. It also
//! asks the detector, for each Git event it persists, whether S3 or a
//! registration points to one session. Explicit registrations (US-GRP-009)
//! create or confirm sessions here and hand them to the detector, which
//! follows their state until the registration is withdrawn.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitraptor_api::event::SESSION_STATE;
use gitraptor_api::messages::{
    AgentSupport, GitEventKind, InferredAgent, MAX_SESSIONS_PAGE, RegistrationOutcome,
    RegistrationRegisterResult, RegistrationRejection, RegistrationWithdrawResult,
    SessionEndCauseView, SessionStateView, SessionView, SessionsListParams,
};
use gitraptor_api::{Actor, AgentKind as ApiAgentKind, AgentOrigin, Timings, Untrusted, clock};

use super::{
    Daemon, Field, RegisterRequest, RegistrationError, RepoCommandError, WithdrawRequest, env,
    now_ms, profile_error_kind,
};
use crate::detect::{
    Detector, OpenSession, PresentSession, RegisteredSession, S3Outcome, SessionChange,
    SessionConfig, SystemProcLister, detection_supported,
};
use crate::observe::RepoRead;
use crate::profile::{
    Agent, AgentKind, Author, EndCause, NewEvent, Origin, RecordKind, RepoStore, Session,
    Timestamp, WriteOp,
};
use crate::watch::{ObservedBatch, ObserverHooks, RawEvent};

/// Evidence stored with an event that S3 attributed (ADR-GRP-013 § 1).
const S3_EVIDENCE: &str = r#"{"signals":["s3"]}"#;

/// Evidence stored with an event attributed by a registration: the only
/// present session of its worktree, a registered "other agent"
/// (ADR-GRP-012 rule 3).
const REGISTRATION_EVIDENCE: &str = r#"{"signals":["registration"]}"#;

/// Signal of an event inferred from the only active session of its
/// worktree, when S3 saw no `git` (amendment of ADR-GRP-012).
const SINGLE_SESSION_EVIDENCE: &str = "single-session";

/// The session an event points to, and the evidence of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attribution {
    pub session: PresentSession,
    pub evidence: &'static str,
    /// Only a hint (amendment of ADR-GRP-012): the event stays without a
    /// session, so unattributed, and its evidence names the session.
    pub inferred: bool,
}

impl Attribution {
    /// The session the event is stored with: none for a hint.
    pub fn session_id(&self) -> Option<&str> {
        (!self.inferred).then_some(self.session.session_id.as_str())
    }

    /// The evidence stored with the event; a hint names its session.
    pub fn evidence(&self) -> String {
        if self.inferred {
            serde_json::json!({
                "signals": [self.evidence],
                "session": self.session.session_id,
            })
            .to_string()
        } else {
            self.evidence.to_owned()
        }
    }
}

/// The hint stored with an unattributed event, if it has one.
pub(super) fn inferred_agent(evidence: Option<&str>) -> Option<InferredAgent> {
    let value: serde_json::Value = serde_json::from_str(evidence?).ok()?;
    let single = value["signals"]
        .as_array()?
        .iter()
        .any(|s| s == SINGLE_SESSION_EVIDENCE);
    Some(InferredAgent {
        kind: ApiAgentKind::ClaudeCode,
        session_id: value["session"].as_str().filter(|_| single)?.to_owned(),
    })
}

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
        // Registered sessions continue: present until withdrawn (Q41). Their
        // latest activity is at most when the engine last observed.
        let observed_until = store.observed_until().ok().flatten();
        for (s, state) in store.sessions_with_state().unwrap_or_default() {
            if s.end_cause.is_none() && s.initial_origin == Origin::Registered {
                let (state, since) = state_of(&s, state.as_ref());
                let last = observed_until.map_or(since, |o| o.max(since));
                detector.register(registered(repo_id, &s, state, Some(last)));
            }
        }
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
        self.publish_sessions(published, t_recv);
    }

    /// Publishes the current view of persisted sessions (`session.state`).
    fn publish_sessions(&self, published: Vec<(String, String)>, t_recv: u64) {
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
                .map(|(s, state)| session_view(&repo_id, store, &s, state.as_ref()))
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
    /// rules 2 to 6) or, without it, the registration (rule 3), and the
    /// activity it means for its worktree. The S3 outcome goes to the
    /// diagnostic log (SPIKE-GRP-001).
    pub(super) fn attribute(&self, batch: &ObservedBatch) -> Vec<Option<Attribution>> {
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
                let attribution = self.attribute_one(detector, batch, event);
                // After the hint, which wants a session already active.
                detector.activity(&batch.repo_id, &event.worktree);
                attribution
            })
            .collect()
    }

    fn attribute_one(
        &self,
        detector: &Detector,
        batch: &ObservedBatch,
        event: &RawEvent,
    ) -> Option<Attribution> {
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
        let by_registration = || {
            detector
                .registration_evidence(&batch.repo_id, &event.worktree)
                .map(|session| Attribution {
                    session,
                    evidence: REGISTRATION_EVIDENCE,
                    inferred: false,
                })
        };
        // S3 saw no `git` (the short-commit race): the worktree's
        // only active session, as a hint and not an attribution
        // (amendment of ADR-GRP-012).
        let inferred = || {
            by_registration().or_else(|| {
                detector
                    .single_session(&batch.repo_id, &event.worktree)
                    .map(|session| Attribution {
                        session,
                        evidence: SINGLE_SESSION_EVIDENCE,
                        inferred: true,
                    })
            })
        };
        let label = match &outcome {
            S3Outcome::NoSession => return by_registration(),
            S3Outcome::NoSighting => "no-sighting",
            S3Outcome::Ambiguous => "ambiguous",
            S3Outcome::Attributed(_) => "attributed",
        };
        let diag = detector.diagnostics();
        let count = |n: u64| Field::from(i64::try_from(n).unwrap_or(i64::MAX));
        self.logger.info(
            "s3_evidence",
            &[
                ("repo", Field::id(&batch.repo_id)),
                ("event", event.kind.as_str().into()),
                ("outcome", label.into()),
                ("samples", count(diag.samples)),
                ("s3_cwd_unreadable", count(diag.cwd_unreadable)),
                ("s3_placed_by_ancestor", count(diag.placed_by_ancestor)),
            ],
        );
        match outcome {
            S3Outcome::Attributed(session) => Some(Attribution {
                session,
                evidence: S3_EVIDENCE,
                inferred: false,
            }),
            S3Outcome::NoSighting => inferred(),
            S3Outcome::NoSession | S3Outcome::Ambiguous => by_registration(),
        }
    }

    /// `registration.register` (US-GRP-009): confirms the present session
    /// of the same agent in the worktree, or creates a registered one
    /// (BR-CONS-004, Q39). The channel already decided who asks and, for an
    /// agent, that the folder is its working folder (ADR-GRP-005 § 6.6).
    pub(super) fn register(
        &mut self,
        request: RegisterRequest,
    ) -> Result<RegistrationRegisterResult, RegistrationError> {
        let (repo_id, worktree) = self.locate_worktree(&request.folder)?;
        if request
            .named
            .as_ref()
            .is_some_and(|named| !named.starts_with(&worktree))
        {
            return Err(RegistrationError::Rejected(
                RegistrationRejection::WorktreeMismatch,
            ));
        }
        let t_recv = clock::monotonic_ns();
        let now = now_ms();
        let offset = crate::watch::wall_now().1;
        let store = self.store_mut(&repo_id)?;
        let present: Vec<Session> = store
            .sessions_for_worktree(&worktree)
            .map_err(|_| RegistrationError::Internal)?
            .into_iter()
            .filter(|s| s.end_cause.is_none() && same_agent(&s.agent, &request.agent))
            .collect();
        // The caller's own session first; the developer confirms the most
        // recent one of that agent.
        let same = present
            .iter()
            .find(|s| request.caller_session.as_deref() == Some(s.session_id.as_str()))
            .or_else(|| match request.author {
                Author::Developer => present.last(),
                Author::Agent => None,
            })
            .cloned();
        let (session, outcome) = match same {
            Some(session) => {
                if session_actor(store, &session) == registered_actor(&session.agent) {
                    (session, RegistrationOutcome::AlreadyRegistered)
                } else {
                    store
                        .write_batch(&[WriteOp::AppendAttribution {
                            session_id: session.session_id.clone(),
                            kind: RecordKind::Confirm,
                            agent: session.agent.clone(),
                            author: request.author,
                            recorded_ms: now,
                        }])
                        .map_err(|err| self.registration_failed(&err))?;
                    (session, RegistrationOutcome::Confirmed)
                }
            }
            None => {
                let session_id = new_session_id(store, now);
                let mut ops = worktree_op(store, &worktree, now);
                ops.extend([
                    WriteOp::StartSession {
                        session_id: session_id.clone(),
                        worktree: worktree.clone(),
                        agent: request.agent.clone(),
                        origin: Origin::Registered,
                        detection_key: None,
                        started_ms: now,
                    },
                    WriteOp::AppendAttribution {
                        session_id: session_id.clone(),
                        kind: RecordKind::Register,
                        agent: request.agent.clone(),
                        author: request.author,
                        recorded_ms: now,
                    },
                    state_event(worktree.clone(), "session-start", now, offset, &session_id),
                ]);
                store
                    .write_batch(&ops)
                    .map_err(|err| self.registration_failed(&err))?;
                let session = self
                    .store_mut(&repo_id)?
                    .session(&session_id)
                    .ok()
                    .flatten()
                    .ok_or(RegistrationError::Internal)?;
                if let Some(detector) = &self.detector {
                    detector.register(registered(
                        &repo_id,
                        &session,
                        SessionStateView::Active,
                        None,
                    ));
                }
                (session, RegistrationOutcome::Created)
            }
        };
        let store = self.store_mut(&repo_id)?;
        let actor = session_actor(store, &session);
        self.logger.info(
            "agent_registered",
            &[
                ("repo", Field::id(&repo_id)),
                ("agent", session.agent.kind.as_str().into()),
                ("author", request.author.as_str().into()),
                ("outcome", outcome_text(outcome).into()),
            ],
        );
        if outcome != RegistrationOutcome::AlreadyRegistered {
            self.publish_sessions(vec![(repo_id.clone(), session.session_id.clone())], t_recv);
        }
        Ok(RegistrationRegisterResult {
            repo_id,
            session_id: session.session_id,
            outcome,
            actor,
            support: match session.agent.kind {
                AgentKind::ClaudeCode => AgentSupport::Full,
                AgentKind::Other => AgentSupport::Observed,
            },
        })
    }

    /// `registration.withdraw` (US-GRP-009, Q41), already authorized as a
    /// reserved command: ends the present session that a registration of
    /// that agent created. A detected session, even confirmed, ends with
    /// its process instead.
    pub(super) fn withdraw(
        &mut self,
        request: WithdrawRequest,
    ) -> Result<RegistrationWithdrawResult, RegistrationError> {
        let (repo_id, worktree) = self.locate_worktree(&request.folder)?;
        let t_recv = clock::monotonic_ns();
        let now = now_ms();
        let offset = crate::watch::wall_now().1;
        let store = self.store_mut(&repo_id)?;
        let session = store
            .sessions_for_worktree(&worktree)
            .map_err(|_| RegistrationError::Internal)?
            .into_iter()
            .rev()
            .find(|s| {
                s.end_cause.is_none()
                    && s.initial_origin == Origin::Registered
                    && same_agent(&s.agent, &request.agent)
            })
            .ok_or(RegistrationError::Rejected(
                RegistrationRejection::NotRegistered,
            ))?;
        let ops = [
            WriteOp::AppendAttribution {
                session_id: session.session_id.clone(),
                kind: RecordKind::WithdrawRegistration,
                agent: session.agent.clone(),
                author: Author::Developer,
                recorded_ms: now,
            },
            WriteOp::EndSession {
                session_id: session.session_id.clone(),
                ended_ms: Some(now),
                cause: EndCause::RegistrationWithdrawn,
            },
            state_event(worktree, "session-end", now, offset, &session.session_id),
        ];
        store
            .write_batch(&ops)
            .map_err(|err| self.registration_failed(&err))?;
        if let Some(detector) = &self.detector {
            detector.end_registered(&session.session_id);
        }
        self.logger.info(
            "registration_withdrawn",
            &[
                ("repo", Field::id(&repo_id)),
                ("agent", session.agent.kind.as_str().into()),
            ],
        );
        self.publish_sessions(vec![(repo_id.clone(), session.session_id.clone())], t_recv);
        Ok(RegistrationWithdrawResult {
            repo_id,
            session_id: session.session_id,
        })
    }

    /// The observed worktree that holds `folder` (BR-VAL-002): the longest
    /// root, for nested worktrees. Outside every one, whether it is a Git
    /// repo the engine does not observe or not a worktree at all.
    fn locate_worktree(&self, folder: &Path) -> Result<(String, PathBuf), RegistrationError> {
        let folder = crate::observe::canonical(folder);
        let found = self
            .stores
            .iter()
            .flat_map(|(repo_id, _)| {
                self.observer
                    .as_ref()
                    .map(|o| o.worktrees(repo_id))
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |w| (repo_id.clone(), w))
            })
            .filter(|(_, w)| folder.starts_with(w))
            .max_by_key(|(_, w)| w.as_os_str().len());
        if let Some(found) = found {
            return Ok(found);
        }
        let reason = match crate::observe::locate(&folder) {
            Ok(common) => match self.profile.repo_by_common_dir(&common) {
                Ok(Some(entry)) if self.stores.iter().any(|(id, _)| *id == entry.repo_id) => {
                    RegistrationRejection::NotAWorktree
                }
                _ => RegistrationRejection::RepoNotObserved,
            },
            Err(_) => RegistrationRejection::NotAWorktree,
        };
        Err(RegistrationError::Rejected(reason))
    }

    fn store_mut(&mut self, repo_id: &str) -> Result<&mut RepoStore, RegistrationError> {
        self.stores
            .iter_mut()
            .find(|(id, _)| id == repo_id)
            .map(|(_, store)| store)
            .ok_or(RegistrationError::Rejected(
                RegistrationRejection::RepoNotObserved,
            ))
    }

    fn registration_failed(&self, err: &crate::profile::ProfileError) -> RegistrationError {
        self.logger.error(
            "registration_persist_failed",
            &[("kind", profile_error_kind(err).into())],
        );
        RegistrationError::Internal
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
                    .map(|(s, state)| session_view(repo_id, store, s, state.as_ref())),
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

/// A stored registered session, for the detector.
fn registered(
    repo_id: &str,
    session: &Session,
    state: SessionStateView,
    last_activity_ms: Option<i64>,
) -> RegisteredSession {
    RegisteredSession {
        repo_id: repo_id.to_owned(),
        session_id: session.session_id.clone(),
        worktree: session.worktree.clone(),
        started_ms: session.started_ms,
        state,
        last_activity_ms,
        registration_evidence: session.agent.kind == AgentKind::Other,
    }
}

/// The same agent (BR-CONS-004): the same kind and, for an "other agent",
/// the same declared name, without case or surrounding spaces.
fn same_agent(a: &Agent, b: &Agent) -> bool {
    let fold = |n: &Option<String>| n.as_deref().map(|n| n.trim().to_lowercase());
    a.kind == b.kind && fold(&a.name) == fold(&b.name)
}

/// The actor of `agent` once registered or confirmed.
fn registered_actor(agent: &Agent) -> Actor {
    Actor::Agent {
        kind: api_kind(agent.kind),
        name: agent.name.clone().map(gitraptor_api::UntrustedName::new),
        origin: AgentOrigin::Registered,
    }
}

fn api_kind(kind: AgentKind) -> ApiAgentKind {
    match kind {
        AgentKind::ClaudeCode => ApiAgentKind::ClaudeCode,
        AgentKind::Other => ApiAgentKind::Other,
    }
}

/// A new id for a registered session: not a `(pid, start)` one, so the
/// restart reconciliation of detected sessions never takes it for a process.
fn new_session_id(store: &RepoStore, now: i64) -> String {
    (0u32..)
        .map(|n| format!("reg:{now}:{n}"))
        .find(|id| store.session(id).ok().flatten().is_none())
        .unwrap_or_default()
}

/// The worktree row a new session needs, if the store lacks it.
fn worktree_op(store: &RepoStore, worktree: &Path, now: i64) -> Vec<WriteOp> {
    let known = store
        .worktrees()
        .map(|all| all.iter().any(|w| w.path == worktree))
        .unwrap_or(false);
    if known {
        return Vec::new();
    }
    vec![WriteOp::UpsertWorktree {
        path: worktree.to_owned(),
        admin_name: None,
        seen_ms: now,
    }]
}

fn outcome_text(outcome: RegistrationOutcome) -> &'static str {
    match outcome {
        RegistrationOutcome::Created => "created",
        RegistrationOutcome::Confirmed => "confirmed",
        RegistrationOutcome::AlreadyRegistered => "already-registered",
    }
}

/// Operations that create a detected session, if the store lacks it.
pub(super) fn start_ops(store: &RepoStore, session: &PresentSession) -> Vec<WriteOp> {
    if store.session(&session.session_id).ok().flatten().is_some() {
        return Vec::new();
    }
    let mut ops = worktree_op(store, &session.worktree, session.started_ms);
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

/// Effective attribution of a session (ADR-GRP-013 § 2): its initial one
/// and the records in force. Here only a confirmation changes it: the
/// origin becomes "registered" (P16). Corrections belong to US-GRP-010.
pub(super) fn session_actor(store: &RepoStore, session: &Session) -> Actor {
    let confirmed = session.initial_origin == Origin::Registered
        || store
            .attribution_records(&session.session_id)
            .unwrap_or_default()
            .iter()
            .any(|r| r.kind == RecordKind::Confirm);
    Actor::Agent {
        kind: api_kind(session.agent.kind),
        name: session
            .agent
            .name
            .clone()
            .map(gitraptor_api::UntrustedName::new),
        origin: if confirmed {
            AgentOrigin::Registered
        } else {
            AgentOrigin::Detected
        },
    }
}

fn session_view(
    repo_id: &str,
    store: &RepoStore,
    session: &Session,
    latest: Option<&(String, i64)>,
) -> SessionView {
    let (state, since) = state_of(session, latest);
    SessionView {
        repo_id: repo_id.to_owned(),
        session_id: session.session_id.clone(),
        worktree: Untrusted::from_os(session.worktree.as_os_str()),
        actor: session_actor(store, session),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn attribution(evidence: &'static str, inferred: bool) -> Attribution {
        Attribution {
            session: PresentSession {
                session_id: "20:2000".into(),
                worktree: PathBuf::from("/wt/feat-login"),
                started_ms: 1,
            },
            evidence,
            inferred,
        }
    }

    /// Amendment of ADR-GRP-012: a hint is stored without a session (the
    /// event stays unattributed) and its evidence names the session.
    #[test]
    fn a_hint_is_stored_without_a_session_and_read_back() {
        let hint = attribution(SINGLE_SESSION_EVIDENCE, true);
        assert_eq!(hint.session_id(), None);
        let stored = hint.evidence();
        assert_eq!(
            inferred_agent(Some(&stored)),
            Some(InferredAgent {
                kind: ApiAgentKind::ClaudeCode,
                session_id: "20:2000".into(),
            })
        );
        let s3 = attribution(S3_EVIDENCE, false);
        assert_eq!(s3.session_id(), Some("20:2000"));
        assert_eq!(s3.evidence(), S3_EVIDENCE);
        assert_eq!(inferred_agent(Some(S3_EVIDENCE)), None);
        assert_eq!(inferred_agent(Some(REGISTRATION_EVIDENCE)), None);
        assert_eq!(inferred_agent(None), None);
        assert_eq!(inferred_agent(Some("not json")), None);
    }
}
