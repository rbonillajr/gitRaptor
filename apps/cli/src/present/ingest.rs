//! The ingest: contract types into the view model (ADR-CKP-003 § 8). Every
//! untrusted text goes through [`SafeText`] here and nowhere else; the
//! model never keeps it raw.

use gitraptor_api::Actor;
use gitraptor_api::scope::{ConnectionRequester, GlobalSnapshot, RepoSnapshot};

use crate::model::{GlobalView, RepoView, Requester};
use crate::present::SafeText;

pub fn global(snapshot: &GlobalSnapshot) -> GlobalView {
    GlobalView {
        engine: snapshot.engine.state,
        git_version: snapshot.engine.git_version.as_deref().map(SafeText::name),
        autostart: snapshot.autostart,
        repo_count: snapshot.repos.len(),
    }
}

pub fn repo(snapshot: &RepoSnapshot) -> RepoView {
    RepoView {
        repo_id: snapshot.repo.repo_id.clone(),
        path: SafeText::from_untrusted(&snapshot.repo.path),
        worktree_count: snapshot.repo.worktrees.len(),
    }
}

pub fn requester(requester: &ConnectionRequester) -> Requester {
    match requester {
        ConnectionRequester::Resolved {
            actor: Actor::Unattributed,
            layer,
            ..
        } => Requester::Unattributed { layer: *layer },
        ConnectionRequester::Resolved {
            actor: Actor::Agent { name, .. },
            layer,
            ..
        } => Requester::Agent {
            name: name.as_ref().map(SafeText::name_from_untrusted),
            layer: *layer,
        },
        ConnectionRequester::Unverified => Requester::Unverified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::UntrustedName;
    use gitraptor_api::actor::{AgentKind, AgentOrigin};
    use gitraptor_api::catalog::Layer;

    #[test]
    fn an_agent_name_is_sanitized_on_the_way_in() {
        let r = requester(&ConnectionRequester::Resolved {
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: Some(UntrustedName::new("claude-1\u{1b}]0;x\u{7}")),
                origin: AgentOrigin::Detected,
            },
            layer: Layer::Mcp,
            confirmable: false,
        });
        let Requester::Agent {
            name: Some(name),
            layer: Layer::Mcp,
        } = r
        else {
            panic!("{r:?}");
        };
        assert_eq!(name.as_str(), "claude-1\\x1b]0;x\\x07");
    }
}
