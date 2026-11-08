//! Which sessions `undo --agent <text>` names: a session id of the engine, or an agent name
//! matched against each session's current attribution.

use super::engine::SessionAttribution;
use super::oplog::Requester;
use super::timeline::name_matches;

/// What the text of `--agent` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSelector {
    /// One engine session, by its id.
    Session(String),
    /// Every session whose current attribution carries this agent name.
    Name(String),
}

impl AgentSelector {
    /// Reads `text` as a session id when `sessions` knows it, as an agent name otherwise.
    pub fn resolve(text: &str, sessions: &SessionAttribution) -> Self {
        if sessions.0.contains_key(text) {
            Self::Session(text.to_owned())
        } else {
            Self::Name(text.to_owned())
        }
    }

    /// Whether the work of `owner` (a current attribution) is selected. Unattributed work never
    /// is, and `unattributed` is no agent name.
    pub fn selects(&self, owner: &Requester) -> bool {
        let Requester::Agent {
            name, session_id, ..
        } = owner
        else {
            return false;
        };
        match self {
            Self::Session(id) => id == session_id,
            Self::Name(wanted) => wanted != "unattributed" && name_matches(wanted, name),
        }
    }

    /// Whether the selector names the requester `who` itself: the same rule as [`Self::selects`].
    pub fn names_requester(&self, who: &Requester) -> bool {
        self.selects(who)
    }
}

#[cfg(test)]
#[path = "agent_selector_tests.rs"]
mod tests;
