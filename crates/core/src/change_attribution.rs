//! Who the uncommitted changes of a worktree are attributed to (US-GRP-008).

use gitraptor_api::actor::Actor;
use gitraptor_api::messages::ChangeCounts;

/// The actor published for the uncommitted changes of a worktree: `None`
/// when it is clean, [`Actor::Unattributed`] otherwise.
///
/// A file change would only have an agent with positive per-file evidence
/// (transcripts or an editor integration, ADR-GRP-012); the MVP has none.
/// The function takes no sessions, registrations or activity on purpose:
/// none of them is evidence for a file change, so "doubtful" never becomes
/// an agent (BR-EDGE-004).
#[must_use]
pub fn changes_actor(counts: &ChangeCounts) -> Option<Actor> {
    if counts.is_clean() {
        None
    } else {
        Some(Actor::Unattributed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(staged: u32, unstaged: u32, untracked: u32) -> ChangeCounts {
        ChangeCounts {
            staged,
            unstaged,
            untracked,
        }
    }

    #[test]
    fn clean_worktree_has_no_changes_actor() {
        assert_eq!(changes_actor(&counts(0, 0, 0)), None);
    }

    #[test]
    fn changes_are_unattributed_whatever_the_sessions() {
        // The function has no session input: every shape of change gives the same answer.
        for c in [
            counts(1, 0, 0),
            counts(0, 1, 0),
            counts(0, 0, 1),
            counts(5, 3, 2),
        ] {
            assert_eq!(changes_actor(&c), Some(Actor::Unattributed));
        }
    }
}
