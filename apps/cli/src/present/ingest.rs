//! The ingest: contract types into the view model (ADR-CKP-003 § 8). Every
//! untrusted text goes through [`SafeText`] here and nowhere else; the
//! model never keeps it raw.

use gitraptor_api::messages::{
    GitEventKind, GitEventView, HeadView, SessionView, TrailerCheck, WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{AttentionCount, ConnectionRequester, GlobalSnapshot, RepoSnapshot};
use gitraptor_api::{Actor, AgentKind};

use crate::model::{
    GlobalView, Head, LastCommit, RepoAttention, RepoChoice, RepoView, Requester, SessionRow,
    WorktreeRow, WorktreeState,
};
use crate::present::SafeText;

pub fn global(snapshot: &GlobalSnapshot) -> GlobalView {
    GlobalView {
        engine: snapshot.engine.state,
        git_version: snapshot.engine.git_version.as_deref().map(SafeText::name),
        autostart: snapshot.autostart,
        repo_count: snapshot.repos.len(),
        repos: snapshot
            .repos
            .iter()
            .map(|r| RepoChoice {
                repo_id: r.repo_id.clone(),
                name: SafeText::name(&repo_name(r.path.raw())),
                path: SafeText::from_untrusted(&r.path),
            })
            .collect(),
        attention: snapshot
            .repos
            .iter()
            .map(|r| {
                let count = |c: &AttentionCount| match c {
                    AttentionCount::Counted { count } => Some(*count),
                    AttentionCount::Unavailable { .. } => None,
                };
                RepoAttention {
                    repo_id: r.repo_id.clone(),
                    conflicts: count(&r.attention.conflicts),
                    denials: count(&r.attention.denials),
                }
            })
            .collect(),
    }
}

/// The repo of a snapshot. Its sessions come apart (`sessions.list`), so they start empty.
pub fn repo(snapshot: &RepoSnapshot) -> RepoView {
    RepoView {
        repo_id: snapshot.repo.repo_id.clone(),
        path: SafeText::from_untrusted(&snapshot.repo.path),
        name: SafeText::name(&repo_name(snapshot.repo.path.raw())),
        base: snapshot
            .repo
            .base
            .name
            .as_ref()
            .map(SafeText::name_from_untrusted),
        worktrees: worktrees(&snapshot.repo.worktrees),
        fetched_ms: snapshot.repo.fetched_utc_ms,
        sessions: Vec::new(),
        detection: None,
        commits: Vec::new(),
    }
}

/// The last commit a Git event records, with its declared authorship (US-CKP-026): only a
/// `commit` or a `merge` that carries it (`events.authorship`). The author's name is
/// sanitized; the emails are never kept.
pub fn last_commit(event: &GitEventView) -> Option<LastCommit> {
    let merge = match event.kind {
        GitEventKind::Commit => false,
        GitEventKind::Merge => true,
        _ => return None,
    };
    let declared = event.authorship.as_ref()?;
    let mut agents: Vec<AgentKind> = Vec::new();
    for kind in declared.coauthors.iter().filter_map(|c| c.agent) {
        if !agents.contains(&kind) {
            agents.push(kind);
        }
    }
    let ran_by = match &event.actor {
        Actor::Agent { kind, .. } if !agents.contains(kind) => Some(*kind),
        _ => None,
    };
    let inferred = match (&event.actor, &event.inferred) {
        (Actor::Unattributed, Some(hint))
            if hint.trailer != Some(TrailerCheck::Confirmed) && !agents.contains(&hint.kind) =>
        {
            Some(hint.kind)
        }
        _ => None,
    };
    Some(LastCommit {
        worktree: key(event.worktree.raw()),
        seq: event.seq,
        merge,
        author: SafeText::name_from_untrusted(&declared.author.name),
        agents,
        ran_by,
        inferred,
    })
}

/// The folder of a repo from its common dir: `shop` for `/w/shop/.git`, `shop.git` for a bare
/// `/w/shop.git`.
fn repo_name(common_dir: &str) -> String {
    let path = std::path::Path::new(common_dir);
    let leaf = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    match leaf(path) {
        Some(n) if n == ".git" => path.parent().and_then(leaf).unwrap_or(n),
        Some(n) => n,
        None => common_dir.to_owned(),
    }
}

/// The first seven digits of a commit hash; `None` for anything that is not one.
fn short_commit(hex: &str) -> Option<SafeText> {
    (hex.len() >= 7 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| SafeText::text(&hex[..7]))
}

/// The system's temporary folders, as written and resolved (on macOS `/var` is `/private/var`,
/// and the engine publishes resolved roots).
fn temp_roots() -> &'static [std::path::PathBuf] {
    static ROOTS: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut roots = vec![std::env::temp_dir()];
        if cfg!(unix) {
            roots.push("/tmp".into());
        }
        let resolved: Vec<_> = roots
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .collect();
        roots.extend(resolved);
        roots
    })
}

/// Whether a worktree root lies under one of the temporary folders.
fn temporary(root: &std::path::Path, roots: &[std::path::PathBuf]) -> bool {
    roots.iter().any(|r| root.starts_with(r))
}

/// The last component of a worktree root (the root itself when it has none).
fn leaf(root: &str) -> String {
    std::path::Path::new(root)
        .file_name()
        .map_or_else(|| root.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// The key a worktree and its sessions share: a hash of the raw root, so two roots that only
/// differ in what the sanitizer neutralizes stay apart.
fn key(raw: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    raw.hash(&mut h);
    h.finish()
}

pub fn worktrees(views: &[WorktreeView]) -> Vec<WorktreeRow> {
    views
        .iter()
        .map(|w| WorktreeRow {
            path: SafeText::from_untrusted(&w.path),
            name: SafeText::name(&leaf(w.path.raw())),
            key: key(w.path.raw()),
            main: w.main,
            last_activity_ms: w.last_activity_utc_ms,
            temporary: temporary(std::path::Path::new(w.path.raw()), temp_roots()),
            state: match &w.status {
                WorktreeStatus::Ready {
                    head,
                    counts,
                    divergence,
                    ..
                } => WorktreeState::Ready {
                    head: match head {
                        HeadView::Branch { name } => {
                            Head::Branch(SafeText::name_from_untrusted(name))
                        }
                        HeadView::Unborn { name } => {
                            Head::Unborn(SafeText::name_from_untrusted(name))
                        }
                        HeadView::Detached => {
                            Head::Detached(w.detached_at.as_deref().and_then(short_commit))
                        }
                    },
                    changes: counts.total(),
                    divergence: divergence.clone(),
                },
                WorktreeStatus::Unavailable { reason } => WorktreeState::Unavailable(*reason),
            },
        })
        .collect()
}

/// A session is always an agent (ADR-GRP-013); an unattributed actor would be a contract
/// error and is shown as an "other agent" without a name rather than dropped.
pub fn session(view: &SessionView) -> SessionRow {
    let (kind, name) = match &view.actor {
        Actor::Agent { kind, name, .. } => {
            (*kind, name.as_ref().map(SafeText::name_from_untrusted))
        }
        Actor::Unattributed => (AgentKind::Other, None),
    };
    SessionRow {
        session_id: view.session_id.clone(),
        worktree: key(view.worktree.raw()),
        kind,
        name,
        state: view.state,
        state_since_ms: view.state_since_utc_ms,
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
    use std::path::{Path, PathBuf};

    /// US-CKP-001: only a real hash becomes the short one; anything else is left out.
    #[test]
    fn only_a_commit_hash_is_shortened() {
        let short = |hex: &str| short_commit(hex).map(|s| s.as_str().to_owned());
        assert_eq!(
            short("39e852f0a1b2c3d4e5f60718293a4b5c6d7e8f90").as_deref(),
            Some("39e852f")
        );
        assert_eq!(short("39e85"), None);
        assert_eq!(short("39e852f\u{1b}[31m"), None);
    }

    /// A worktree is temporary when it lies under a temporary folder, by whole components.
    #[test]
    fn a_worktree_under_the_temporary_folder_is_temporary() {
        let roots = [
            PathBuf::from("/tmp"),
            PathBuf::from("/private/var/folders/x/T"),
        ];
        assert!(temporary(Path::new("/tmp/scratch"), &roots));
        assert!(temporary(Path::new("/private/var/folders/x/T/wt"), &roots));
        assert!(!temporary(Path::new("/tmpfoo/wt"), &roots));
        assert!(!temporary(Path::new("/w/shop"), &roots));
        assert!(temporary(&std::env::temp_dir().join("wt"), temp_roots()));
    }

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
