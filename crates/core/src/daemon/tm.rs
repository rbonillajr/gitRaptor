//! The engine as the Time Machine reads it (US-TMC-004): marks, calm and raw events, and the
//! continuous capture fed by what the loop persists.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gitraptor_api::Actor;
use gitraptor_api::messages::GitEventKind;

use super::shutdown::ShutdownHandle;
use super::{Daemon, sessions};
use crate::profile::RepoStore;
use crate::timemachine::continuous::{CaptureConfig, CaptureLayer};
use crate::timemachine::engine::{EngineLink, RawGitEvent, RepoMarks, generation_floor};
use crate::timemachine::oplog::{Requester, RequesterOrigin};

/// The continuous capture of the daemon: its cadence and, in tests, a fault layer.
#[derive(Clone, Default)]
pub struct TmCapture {
    pub config: CaptureConfig,
    /// Tests only: answers captures with an error. Honored only in debug builds.
    #[doc(hidden)]
    pub layer: Option<CaptureLayer>,
    /// Tests only: the free-space floor of SEC-TMC-12 is not checked (a CI disk may be small).
    #[doc(hidden)]
    pub no_free_space_floor: bool,
}

impl std::fmt::Debug for TmCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TmCapture")
            .field("config", &self.config)
            .field("layer", &self.layer.is_some())
            .finish_non_exhaustive()
    }
}

/// [`EngineLink`] over the daemon: marks in memory, raw events through the loop.
pub(super) struct DaemonEngine {
    pub marks: Arc<RepoMarks>,
    pub handle: ShutdownHandle,
}

impl EngineLink for DaemonEngine {
    fn mark(&self, repo_id: &str) -> i64 {
        self.marks.seq(repo_id)
    }

    fn settle(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration) -> Option<i64> {
        self.marks.settle(repo_id, worktrees, limit)
    }

    fn raw_events(&self, repo_id: &str, worktree: &Path) -> Option<Vec<RawGitEvent>> {
        self.handle.raw_events(repo_id, worktree)
    }

    fn generation_floor(&self, repo_id: &str) -> i64 {
        self.marks.floor(repo_id)
    }
}

impl Daemon {
    /// The marks of a repo whose engine store and Time Machine are both open: its last sequence
    /// and where its generation starts in the oplog.
    pub(super) fn link_marks(&self, repo_id: &str) {
        let Some((_, store)) = self.stores.iter().find(|(id, _)| id == repo_id) else {
            return;
        };
        self.marks.set_seq(repo_id, store.last_seq());
        let floor = match (self.tm.oplog(repo_id), store.generation()) {
            (Some(oplog), Ok(generation)) => {
                let log = oplog.lock().unwrap_or_else(|e| e.into_inner());
                generation_floor(&self.config.dirs, repo_id, &generation, &log)
            }
            (Some(oplog), Err(_)) => {
                let log = oplog.lock().unwrap_or_else(|e| e.into_inner());
                log.last_seq().unwrap_or(i64::MAX - 1) + 1
            }
            (None, _) => return,
        };
        self.marks.set_floor(repo_id, floor);
    }

    /// The raw Git events of a worktree, with the actor the engine attributed (loop side).
    pub(super) fn raw_events(&self, repo_id: &str, worktree: &Path) -> Option<Vec<RawGitEvent>> {
        let (_, store) = self.stores.iter().find(|(id, _)| id == repo_id)?;
        let events = store.events_for_worktree(worktree).ok()?;
        Some(
            events
                .into_iter()
                .filter_map(|e| {
                    let kind = GitEventKind::parse(&e.kind)?;
                    let details: gitraptor_api::messages::GitEventDetails =
                        serde_json::from_str(&e.metadata).unwrap_or_default();
                    let actor = e
                        .session_id
                        .as_deref()
                        .map_or(Requester::Unattributed, |id| requester_of(store, id));
                    Some(RawGitEvent {
                        seq: e.seq,
                        kind,
                        branch: details.branch.map(|b| b.raw().to_owned()),
                        actor,
                    })
                })
                .collect(),
        )
    }
}

/// The frozen requester of an engine session: the agent and its origin as the engine shows it.
fn requester_of(store: &RepoStore, session_id: &str) -> Requester {
    let Some(session) = store.session(session_id).ok().flatten() else {
        return Requester::Unattributed;
    };
    match sessions::session_actor(store, &session) {
        Actor::Agent { kind, name, origin } => Requester::Agent {
            name: name.map_or_else(
                || {
                    serde_json::to_value(kind)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "agent".into())
                },
                |n| n.raw().to_owned(),
            ),
            origin: match origin {
                gitraptor_api::AgentOrigin::Registered => RequesterOrigin::Registered,
                _ => RequesterOrigin::Detected,
            },
            session_id: session_id.to_owned(),
        },
        Actor::Unattributed => Requester::Unattributed,
    }
}
