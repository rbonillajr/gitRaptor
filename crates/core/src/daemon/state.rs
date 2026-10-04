//! Engine availability states of BR-WF-002, as types.
//!
//! Only the skeleton lives here: the three states and the transitions
//! BR-WF-002 allows. When the engine enters and leaves "Waiting for Git" and
//! "No repos", and how clients see them, belong to US-GRP-014 and
//! US-GRP-015. "Observing" is the only state with full behavior.

/// The state the engine exposes (BR-WF-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    /// No system Git, or older than 2.38. Nothing is observed. Takes
    /// priority over [`EngineState::NoRepos`].
    WaitingForGit,
    /// Git is fine but no repo is observed.
    NoRepos,
    /// Git is fine and at least one repo is observed.
    Observing,
}

/// Something that may move the engine to another state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A valid Git appeared; `observed_repos` repos are in the profile.
    GitReady { observed_repos: usize },
    /// Git disappeared or became older than 2.38 (S19).
    GitLost,
    /// The developer added the first repo.
    FirstRepoAdded,
    /// The developer retired the last observed repo.
    LastRepoRetired,
}

/// A transition BR-WF-002 does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: EngineState,
    pub trigger: Trigger,
}

impl EngineState {
    /// State at startup, from the Git found and the repos in the profile.
    /// Without Git the engine waits even if there are repos.
    pub fn initial(git_found: bool, observed_repos: usize) -> Self {
        match (git_found, observed_repos) {
            (false, _) => Self::WaitingForGit,
            (true, 0) => Self::NoRepos,
            (true, _) => Self::Observing,
        }
    }

    /// Applies `trigger`, or rejects it if BR-WF-002 has no such transition.
    pub fn on(self, trigger: Trigger) -> Result<Self, InvalidTransition> {
        use EngineState::*;
        use Trigger::*;
        let next = match (self, trigger) {
            (WaitingForGit, GitReady { observed_repos: 0 }) => NoRepos,
            (WaitingForGit, GitReady { .. }) => Observing,
            (NoRepos, FirstRepoAdded) => Observing,
            (Observing, LastRepoRetired) => NoRepos,
            (NoRepos | Observing, GitLost) => WaitingForGit,
            (from, trigger) => return Err(InvalidTransition { from, trigger }),
        };
        Ok(next)
    }

    /// Stable name for logs and, later, the channel.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WaitingForGit => "waiting-for-git",
            Self::NoRepos => "no-repos",
            Self::Observing => "observing",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EngineState::*;
    use super::Trigger::*;
    use super::*;

    #[test]
    fn initial_state_follows_git_then_repos() {
        assert_eq!(EngineState::initial(false, 0), WaitingForGit);
        assert_eq!(EngineState::initial(false, 3), WaitingForGit);
        assert_eq!(EngineState::initial(true, 0), NoRepos);
        assert_eq!(EngineState::initial(true, 1), Observing);
    }

    #[test]
    fn allowed_transitions() {
        let ok = [
            (WaitingForGit, GitReady { observed_repos: 0 }, NoRepos),
            (WaitingForGit, GitReady { observed_repos: 2 }, Observing),
            (NoRepos, FirstRepoAdded, Observing),
            (Observing, LastRepoRetired, NoRepos),
            (Observing, GitLost, WaitingForGit),
            (NoRepos, GitLost, WaitingForGit),
        ];
        for (from, trigger, to) in ok {
            assert_eq!(from.on(trigger), Ok(to), "{from:?} + {trigger:?}");
        }
    }

    #[test]
    fn every_other_transition_is_rejected() {
        let states = [WaitingForGit, NoRepos, Observing];
        let triggers = [
            GitReady { observed_repos: 0 },
            GitReady { observed_repos: 1 },
            GitLost,
            FirstRepoAdded,
            LastRepoRetired,
        ];
        let mut allowed = 0;
        for from in states {
            for trigger in triggers {
                match from.on(trigger) {
                    Ok(_) => allowed += 1,
                    Err(err) => assert_eq!(err, InvalidTransition { from, trigger }),
                }
            }
        }
        assert_eq!(allowed, 6, "only the BR-WF-002 transitions are allowed");
    }
}
