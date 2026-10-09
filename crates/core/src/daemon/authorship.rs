//! Declared authorship of the commits the observer sees (US-GRD-019, DS-US-GRD-018 D10 and
//! § 6.2; amendment of ADR-GRP-013, autoría declarada).
//!
//! When an event creates a commit (`commit`, `merge`), the engine reads that commit's author,
//! committer and `Co-Authored-By` trailers with the isolated reader of the guardrails (no
//! ambient configuration, programs neutralized) and keeps only those facts: the message is
//! parsed in memory, bounded like D7, and dropped. The `inferred` hint is checked against the
//! trailers here, once, when the event is stored: a later change of policy rewrites nothing.

use std::path::Path;

use gitraptor_api::messages::{
    CoAuthor, DeclaredAuthorship, GitEventKind, GitIdentity, TrailerCheck,
};
use gitraptor_api::{AgentKind, Untrusted};
use gitraptor_git::RepoReader;
use gitraptor_policy::authorship::{self as parser, Cleanup, MAX_MESSAGE_BYTES, MessageOptions};
use gitraptor_policy::guard::authorship::Policy;

use super::Daemon;
use super::sessions::Attribution;
use crate::watch::{ObservedBatch, RawEvent};

impl Daemon {
    /// The declared authorship of each event of `batch` and its attribution with the `inferred`
    /// hint checked against it (dropped when the trailers contradict it or the repo is
    /// `human-author`).
    pub(super) fn declared_authorship(
        &self,
        batch: &ObservedBatch,
        attributed: Vec<Option<Attribution>>,
    ) -> (Vec<Option<DeclaredAuthorship>>, Vec<Option<Attribution>>) {
        if !batch.events.iter().any(|e| creates_commit(e.kind)) {
            return (vec![None; batch.events.len()], attributed);
        }
        let common = self
            .profile
            .repo(&batch.repo_id)
            .ok()
            .flatten()
            .map(|e| e.canonical_path);
        let reader = common.as_deref().and_then(reader);
        let declared: Vec<_> = batch
            .events
            .iter()
            .map(|e| reader.as_ref().and_then(|r| declared(r, e)))
            .collect();
        let policy = |event: &RawEvent| {
            let Some(reader) = reader.as_ref() else {
                return Policy::AgentsCommit;
            };
            let common = common.as_deref().unwrap_or(Path::new(""));
            let worktree = crate::guardrails::authorship::worktree_reader(&event.worktree, common);
            let confirmed = self.guard.get(&batch.repo_id).and_then(|e| e.confirmed);
            crate::guardrails::authorship::policy_for(
                worktree.as_ref().unwrap_or(reader),
                confirmed.as_ref(),
                self.guard.profile().as_ref(),
                Some(&batch.repo_id),
            )
            .policy
        };
        let attributed = attributed
            .into_iter()
            .zip(batch.events.iter().zip(&declared))
            .map(|(a, (event, d))| {
                let mut a = a?;
                if a.inferred {
                    a.trailer = checked_hint(
                        AgentKind::ClaudeCode,
                        d.as_ref(),
                        creates_commit(event.kind),
                        || policy(event),
                    )?;
                }
                Some(a)
            })
            .collect();
        (declared, attributed)
    }
}

/// Whether an event of this kind creates a commit whose authorship is declared.
pub(super) fn creates_commit(kind: GitEventKind) -> bool {
    matches!(kind, GitEventKind::Commit | GitEventKind::Merge)
}

/// The declared authorship of the commit `event` created, if it created one and it can be
/// read. A failure to read leaves the event without it; it never holds the event back.
pub(super) fn declared(reader: &RepoReader, event: &RawEvent) -> Option<DeclaredAuthorship> {
    if !creates_commit(event.kind) {
        return None;
    }
    let id = event.details.new_commit.as_deref()?;
    let commit = reader.commit_identity(id, MAX_MESSAGE_BYTES).ok()?;
    let identity = |(name, email): (String, String)| GitIdentity {
        name: Untrusted::new(name),
        email: Untrusted::new(email),
    };
    // The stored message is already cleaned: comment lines are content now.
    let options = MessageOptions {
        cleanup: Cleanup::Verbatim,
        ..MessageOptions::default()
    };
    let coauthors = commit
        .message
        .as_deref()
        .map(|m| parser::coauthors(&String::from_utf8_lossy(m), &options))
        .unwrap_or_default()
        .into_iter()
        .map(|c| CoAuthor {
            agent: parser::recognise(&c.name, &c.email),
            name: Untrusted::new(c.name),
            email: Untrusted::new(c.email),
        })
        .collect();
    Some(DeclaredAuthorship {
        author: identity(commit.author),
        committer: identity(commit.committer),
        coauthors,
    })
}

/// The `inferred` hint of `agent` checked against the trailers (BR-AUTH-005): `None` when the
/// hint must not be recorded: the commit names another agent, or the repo is `human-author`.
/// An event that created no commit keeps its hint unchecked (`Some(None)`).
pub(super) fn checked_hint(
    agent: AgentKind,
    declared: Option<&DeclaredAuthorship>,
    creates_commit: bool,
    policy: impl FnOnce() -> Policy,
) -> Option<Option<TrailerCheck>> {
    if creates_commit && matches!(policy(), Policy::HumanAuthorWarn | Policy::HumanAuthorDeny) {
        return None;
    }
    let Some(declared) = declared else {
        return Some(creates_commit.then_some(TrailerCheck::Unconfirmed));
    };
    let agents = || declared.coauthors.iter().filter_map(|c| c.agent);
    if agents().any(|a| a == agent) {
        Some(Some(TrailerCheck::Confirmed))
    } else if agents().next().is_some() {
        None
    } else {
        Some(Some(TrailerCheck::Unconfirmed))
    }
}

/// The isolated reader of a repo, as the guardrails open it.
pub(super) fn reader(path: &Path) -> Option<RepoReader> {
    crate::guardrails::evaluate::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(agents: &[Option<AgentKind>]) -> DeclaredAuthorship {
        let id = || GitIdentity {
            name: Untrusted::new("Ana Pérez"),
            email: Untrusted::new("ana@example.com"),
        };
        DeclaredAuthorship {
            author: id(),
            committer: id(),
            coauthors: agents
                .iter()
                .map(|a| CoAuthor {
                    name: Untrusted::new("x"),
                    email: Untrusted::new("x@y"),
                    agent: *a,
                })
                .collect(),
        }
    }

    const CLAUDE: AgentKind = AgentKind::ClaudeCode;

    #[test]
    fn the_hint_is_confirmed_unconfirmed_or_dropped() {
        let agents_commit = || Policy::AgentsCommit;
        assert_eq!(
            checked_hint(CLAUDE, Some(&with(&[Some(CLAUDE)])), true, agents_commit),
            Some(Some(TrailerCheck::Confirmed))
        );
        assert_eq!(
            checked_hint(CLAUDE, Some(&with(&[None])), true, agents_commit),
            Some(Some(TrailerCheck::Unconfirmed))
        );
        // Another agent's trailer contradicts the hint: it is not recorded.
        assert_eq!(
            checked_hint(
                CLAUDE,
                Some(&with(&[Some(AgentKind::Other)])),
                true,
                agents_commit
            ),
            None
        );
        // A commit that could not be read stays unconfirmed.
        assert_eq!(
            checked_hint(CLAUDE, None, true, agents_commit),
            Some(Some(TrailerCheck::Unconfirmed))
        );
    }

    #[test]
    fn human_author_records_no_hint_on_a_commit() {
        for p in [Policy::HumanAuthorWarn, Policy::HumanAuthorDeny] {
            assert_eq!(
                checked_hint(CLAUDE, Some(&with(&[Some(CLAUDE)])), true, || p),
                None
            );
        }
        // Events that create no commit are not about authorship.
        assert_eq!(
            checked_hint(CLAUDE, None, false, || Policy::HumanAuthorDeny),
            Some(None)
        );
    }
}
