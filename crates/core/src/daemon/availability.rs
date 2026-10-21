//! Whether an observed repo can be opened now: the state `repo.observation` and the
//! snapshots publish while its directory is moved, deleted or unreadable.

use gitraptor_api::Untrusted;
use gitraptor_api::event::REPO_OBSERVATION;
use gitraptor_api::messages::{RepoObservationData, RepoStateView};

use super::{Daemon, Field};

impl Daemon {
    /// Publishes `state` for the repo and records it in the snapshot the bus serves, in the
    /// same critical section. Does nothing if the repo is unknown or already in `state`, so
    /// calling it on every lost or regained report publishes one event per change.
    pub(super) fn set_repo_state(&mut self, repo_id: &str, state: RepoStateView) {
        let current = self
            .bus
            .snapshot()
            .1
            .repos
            .iter()
            .find(|r| r.repo_id == repo_id)
            .map(|r| r.state);
        if current.is_none_or(|c| c == state) {
            return;
        }
        let Some(entry) = self.profile.repo(repo_id).ok().flatten() else {
            return;
        };
        match state {
            RepoStateView::Unavailable => self
                .logger
                .warn("repo_unavailable", &[("repo", Field::id(repo_id))]),
            RepoStateView::Observed => self
                .logger
                .info("repo_available", &[("repo", Field::id(repo_id))]),
        }
        let id = repo_id.to_owned();
        self.bus.publish(
            REPO_OBSERVATION,
            RepoObservationData {
                repo_id: id.clone(),
                observed: true,
                state,
                path: Untrusted::from_os(entry.canonical_path.as_os_str()),
            },
            None,
            move |shared| {
                if let Some(repo) = shared.repos.iter_mut().find(|r| r.repo_id == id) {
                    repo.state = state;
                }
            },
        );
    }
}
