//! Stable color slots of the agents: a session keeps its slot while it is present, so a row
//! that moves, a resync or a reconnection never repaints it.

use std::collections::BTreeMap;

use gitraptor_api::messages::SessionStateView;

use super::SessionRow;

/// Stable color slots of the present sessions of the open repo, for this run (BR-CKP-EDGE-006).
/// A slot `i` paints `agent_color(i % 8)`: the ninth session repeats the first color.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentColors {
    repo_id: Option<String>,
    slots: BTreeMap<String, usize>,
}

impl AgentColors {
    /// Opens a repo: the register of another repo is emptied, the same repo keeps it.
    pub fn open(&mut self, repo_id: &str) {
        if self.repo_id.as_deref() != Some(repo_id) {
            self.slots.clear();
            self.repo_id = Some(repo_id.to_owned());
        }
    }

    /// The slot of a present session.
    pub fn index(&self, session_id: &str) -> Option<usize> {
        self.slots.get(session_id).copied()
    }

    /// Frees the slot of every ended session and gives the lowest free slot to each present one
    /// without it, oldest first. With `complete` (a whole list on a system that detects
    /// sessions) the sessions that are not in `sessions` free their slot too; a partial update
    /// or a list that may come empty never does.
    pub fn sync(&mut self, sessions: &[SessionRow], complete: bool) {
        self.slots.retain(
            |id, _| match sessions.iter().find(|s| &s.session_id == id) {
                Some(s) => s.state != SessionStateView::Ended,
                None => !complete,
            },
        );
        let mut present: Vec<&SessionRow> = sessions
            .iter()
            .filter(|s| s.state != SessionStateView::Ended)
            .collect();
        present.sort_by(|a, b| (a.started_ms, &a.session_id).cmp(&(b.started_ms, &b.session_id)));
        for s in present {
            if self.slots.contains_key(&s.session_id) {
                continue;
            }
            let slot = (0..)
                .find(|i| !self.slots.values().any(|used| used == i))
                .unwrap_or(0);
            self.slots.insert(s.session_id.clone(), slot);
        }
    }
}

#[cfg(test)]
#[path = "agent_colors_tests.rs"]
mod tests;
