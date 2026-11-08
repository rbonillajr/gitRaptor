//! Which sessions `undo --agent <text>` names: a session id of the engine, or an agent name
//! matched against each session's current attribution.

use super::engine::SessionAttribution;
use super::oplog::Requester;

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
    pub fn resolve(text: &str, _sessions: &SessionAttribution) -> Self {
        Self::Name(text.to_owned())
    }

    /// Whether the work of `owner` (a current attribution) is selected.
    pub fn selects(&self, _owner: &Requester) -> bool {
        false
    }

    /// Whether the selector names the requester `who` itself.
    pub fn names_requester(&self, _who: &Requester) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "agent_selector_tests.rs"]
mod tests;
