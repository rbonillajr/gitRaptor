//! The effective attribution of a session and its correction by the developer.
//!
//! The attribution in force is a pure function of the session's initial attribution and its
//! append-only records, folded in order each time it is read: nothing stores a resolved actor, so
//! correcting appends a `correct` record and withdrawing appends a `withdraw-correction` one. The
//! session and its events never change.

use gitraptor_api::attribution::{
    ATTRIBUTION_CHANGED, AttributionChangeResult, AttributionChangedData, AttributionRejectedData,
    AttributionRejection, MAX_SESSION_CANDIDATES, SessionCandidate,
};
use gitraptor_api::messages::RegistrationRejection;
use gitraptor_api::{Actor, AgentOrigin, Timings, UntrustedName, clock};

use super::sessions::{api_kind, same_agent, session_actor};
use super::{
    AttributionError, CorrectRequest, Daemon, Field, RegistrationError, WithdrawCorrectionRequest,
    now_ms, profile_error_kind,
};
use crate::profile::{
    Agent, AttributionRecord, Author, Origin, RecordKind, RepoStore, Session, WriteOp,
};

/// The attribution of a session once its records are folded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Effective {
    pub agent: Agent,
    pub origin: Origin,
    /// A correction is active.
    pub corrected: bool,
}

/// Folds `records` (in `effective_seq` order) over the initial attribution of `session`.
///
/// `confirm` makes the detected agent registered; `correct` sets the active correction, replacing
/// an earlier one; `withdraw-correction` removes it, back to the detected attribution and never to
/// an earlier correction. Registration records do not change the actor.
pub(super) fn effective(session: &Session, records: &[AttributionRecord]) -> Effective {
    let mut ordered: Vec<&AttributionRecord> = records.iter().collect();
    ordered.sort_by_key(|r| r.effective_seq);
    let mut confirmed = session.initial_origin == Origin::Registered;
    let mut correction: Option<&Agent> = None;
    for record in ordered {
        match record.kind {
            RecordKind::Confirm => confirmed = true,
            RecordKind::Correct => correction = Some(&record.agent),
            RecordKind::WithdrawCorrection => correction = None,
            RecordKind::Register | RecordKind::WithdrawRegistration => {}
        }
    }
    match correction {
        Some(agent) => Effective {
            agent: agent.clone(),
            origin: Origin::Registered,
            corrected: true,
        },
        None => Effective {
            agent: session.agent.clone(),
            origin: if confirmed {
                Origin::Registered
            } else {
                Origin::Detected
            },
            corrected: false,
        },
    }
}

/// [`effective`] over the records of `session` read now. An unreadable store folds no record.
pub(super) fn effective_of(store: &RepoStore, session: &Session) -> Effective {
    let records = store
        .attribution_records(&session.session_id)
        .unwrap_or_default();
    effective(session, &records)
}

/// The actor the engine exposes for an effective attribution.
pub(super) fn actor_of(effective: &Effective) -> Actor {
    Actor::Agent {
        kind: api_kind(effective.agent.kind),
        name: effective.agent.name.clone().map(UntrustedName::new),
        origin: match effective.origin {
            Origin::Registered => AgentOrigin::Registered,
            Origin::Detected => AgentOrigin::Detected,
        },
    }
}

fn rejected(reason: AttributionRejection) -> AttributionError {
    AttributionError::Rejected(AttributionRejectedData {
        reason,
        candidates: Vec::new(),
    })
}

/// The worktree could not be located: the same reasons a registration gives.
fn not_located(err: RegistrationError) -> AttributionError {
    match err {
        RegistrationError::Rejected(RegistrationRejection::NotAWorktree) => {
            rejected(AttributionRejection::NotAWorktree)
        }
        RegistrationError::Rejected(_) => rejected(AttributionRejection::RepoNotObserved),
        RegistrationError::Internal => AttributionError::Internal,
    }
}

/// The only present session among `eligible`, or why there is none to pick.
fn pick_present(
    store: &RepoStore,
    eligible: Vec<(Session, Effective)>,
    none: AttributionRejection,
) -> Result<(Session, Effective), AttributionError> {
    let mut eligible = eligible;
    match eligible.len() {
        0 => Err(rejected(none)),
        1 => Ok(eligible.remove(0)),
        _ => Err(AttributionError::Rejected(AttributionRejectedData {
            reason: AttributionRejection::AmbiguousSession,
            candidates: eligible
                .iter()
                .take(MAX_SESSION_CANDIDATES)
                .map(|(s, _)| SessionCandidate {
                    session_id: s.session_id.clone(),
                    actor: session_actor(store, s),
                    started_utc_ms: s.started_ms,
                    present: s.end_cause.is_none(),
                })
                .collect(),
        })),
    }
}

impl Daemon {
    /// The store of an observed repo and the sessions of `folder`'s worktree with their
    /// attribution in force.
    fn worktree_sessions(
        &self,
        folder: &std::path::Path,
    ) -> Result<(String, Vec<(Session, Effective)>), AttributionError> {
        let (repo_id, worktree) = self.locate_worktree(folder).map_err(not_located)?;
        let store = self.attribution_store(&repo_id)?;
        let sessions = store
            .sessions_for_worktree(&worktree)
            .map_err(|_| AttributionError::Internal)?
            .into_iter()
            .map(|s| {
                let e = effective_of(store, &s);
                (s, e)
            })
            .collect();
        Ok((repo_id, sessions))
    }

    fn attribution_store(&self, repo_id: &str) -> Result<&RepoStore, AttributionError> {
        self.stores
            .iter()
            .find(|(id, _)| id == repo_id)
            .map(|(_, store)| store)
            .ok_or_else(|| rejected(AttributionRejection::RepoNotObserved))
    }

    /// `attribution.correct`, already authorized as a reserved command: appends a `correct`
    /// record to the chosen session. The session and its events are not touched.
    pub(super) fn correct(
        &mut self,
        request: CorrectRequest,
    ) -> Result<AttributionChangeResult, AttributionError> {
        let (repo_id, sessions) = self.worktree_sessions(&request.folder)?;
        let store = self.attribution_store(&repo_id)?;
        let (session, current) = match &request.session_id {
            Some(id) => {
                let (session, current) = sessions
                    .into_iter()
                    .find(|(s, _)| s.session_id == *id)
                    .ok_or_else(|| rejected(AttributionRejection::UnknownSession))?;
                if session.initial_origin == Origin::Registered {
                    return Err(rejected(AttributionRejection::RegisteredSession));
                }
                (session, current)
            }
            None => pick_present(
                store,
                sessions
                    .into_iter()
                    .filter(|(s, _)| s.end_cause.is_none() && s.initial_origin == Origin::Detected)
                    .collect(),
                AttributionRejection::NoDetectedAttribution,
            )?,
        };
        if same_agent(&current.agent, &request.agent) {
            return Err(rejected(AttributionRejection::SameAgent));
        }
        let matches_detected = same_agent(&session.agent, &request.agent);
        self.append_record(
            &repo_id,
            &session,
            RecordKind::Correct,
            request.agent,
            "attribution_corrected",
        )?;
        self.changed(repo_id, session, &current, matches_detected)
    }

    /// `attribution.withdraw-correction`, already authorized as a reserved command: appends a
    /// `withdraw-correction` record, so the session is back to its detected attribution.
    pub(super) fn withdraw_correction(
        &mut self,
        request: WithdrawCorrectionRequest,
    ) -> Result<AttributionChangeResult, AttributionError> {
        let (repo_id, sessions) = self.worktree_sessions(&request.folder)?;
        let store = self.attribution_store(&repo_id)?;
        let (session, current) = match &request.session_id {
            Some(id) => sessions
                .into_iter()
                .find(|(s, _)| s.session_id == *id)
                .ok_or_else(|| rejected(AttributionRejection::UnknownSession))?,
            None => pick_present(
                store,
                sessions
                    .into_iter()
                    .filter(|(s, e)| s.end_cause.is_none() && e.corrected)
                    .collect(),
                AttributionRejection::NotCorrected,
            )?,
        };
        if !current.corrected {
            return Err(rejected(AttributionRejection::NotCorrected));
        }
        // The record names the correction it withdraws, as a trace.
        self.append_record(
            &repo_id,
            &session,
            RecordKind::WithdrawCorrection,
            current.agent.clone(),
            "attribution_correction_withdrawn",
        )?;
        self.changed(repo_id, session, &current, false)
    }

    /// Appends one record by the developer in a write batch and logs it without names or paths.
    fn append_record(
        &mut self,
        repo_id: &str,
        session: &Session,
        kind: RecordKind,
        agent: Agent,
        log_event: &'static str,
    ) -> Result<(), AttributionError> {
        let agent_class = agent.kind.as_str();
        let store = self
            .stores
            .iter_mut()
            .find(|(id, _)| id == repo_id)
            .map(|(_, store)| store)
            .ok_or_else(|| rejected(AttributionRejection::RepoNotObserved))?;
        let written = store.write_batch(&[WriteOp::AppendAttribution {
            session_id: session.session_id.clone(),
            kind,
            agent,
            author: Author::Developer,
            recorded_ms: now_ms(),
        }]);
        if let Err(err) = written {
            self.logger.error(
                "attribution_persist_failed",
                &[("kind", profile_error_kind(&err).into())],
            );
            return Err(AttributionError::Internal);
        }
        self.logger.info(
            log_event,
            &[
                ("repo", Field::id(repo_id)),
                ("agent", agent_class.into()),
                ("author", Author::Developer.as_str().into()),
            ],
        );
        Ok(())
    }

    /// The result of a change, after publishing `attribution.changed` and `session.state`.
    fn changed(
        &self,
        repo_id: String,
        session: Session,
        previous: &Effective,
        matches_detected: bool,
    ) -> Result<AttributionChangeResult, AttributionError> {
        let t_recv = clock::monotonic_ns();
        let store = self.attribution_store(&repo_id)?;
        let actor = session_actor(store, &session);
        let timings = Timings {
            batch_id: self.bus.next_batch(),
            t_recv,
            t_flush: t_recv,
            t_computed: t_recv,
            t_persisted: clock::monotonic_ns(),
            t_published: 0,
        };
        self.bus.publish(
            ATTRIBUTION_CHANGED,
            AttributionChangedData {
                repo_id: repo_id.clone(),
                session_id: session.session_id.clone(),
                actor: actor.clone(),
            },
            Some(timings),
            |_| {},
        );
        self.publish_sessions(vec![(repo_id.clone(), session.session_id.clone())], t_recv);
        Ok(AttributionChangeResult {
            repo_id,
            session_id: session.session_id,
            previous: actor_of(previous),
            actor,
            matches_detected,
        })
    }
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
