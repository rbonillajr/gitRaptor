//! The full `mcp.status` of the daemon: whether the caller's repo and worktree can be read, the
//! scope of a caller whose worktree was deleted, the default answer, the pages of worktrees and
//! paths and the table of cursors of a connection (ADR-MCP-001 § 2, § 6).
//!
//! Everything here is a function of the published state and of [`WorktreeFacts`], the one thing
//! read from the OS, so each decision is tested without a daemon. The cursors the pages carry are
//! [`MCP_CURSOR_PLACEHOLDER`]s: the connection measures the page first and then mints the real
//! ones for what stayed in it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use gitraptor_api::guard::{HooksStatus, ProtectionState};
use gitraptor_api::mcp_view::{
    MCP_CURSOR_PLACEHOLDER, MCP_GAP_WINDOW, MCP_MAX_CURSORS, MCP_MAX_GAPS, MCP_MAX_SESSIONS,
    MCP_PATHS_PAGE, MCP_WORKTREES_PAGE,
};
use gitraptor_api::messages::{
    BaseStatusView, ChangeCounts, DivergenceView, FileChangeView, HeadView, RepoStateView,
    RepoTier, RepoView, SessionStateView, UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::{
    McpBase, McpBaseState, McpEngineState, McpGap, McpHere, McpListRef, McpPage,
    McpProtectionState, McpRepo, McpSession, McpUnavailable, McpUncounted, McpWorktree,
};
use gitraptor_api::{Untrusted, UntrustedName};

use crate::daemon::McpContext;
use crate::observe;

/// What the OS says about a worktree, read once per call: the only I/O of the availability
/// check, so the rest is decided from facts a test can state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorktreeFacts {
    /// The root of the worktree is there.
    pub exists: bool,
    /// The root and the common dir belong to the user running the daemon. `None` when the OS
    /// has no owner to compare (Windows): the engine's own verdict rules then.
    pub owned_by_me: Option<bool>,
    /// A linked worktree passes SEC-11 (its `.git` points back to the repo, its root is not
    /// `$HOME` or a drive root); a main one always does.
    pub trusted_link: bool,
}

/// Whether every one of `paths` belongs to the user running the daemon. The owner is the one of
/// the path itself, never of what a symlink points at. A path whose owner cannot be read is not
/// theirs: the check fails closed.
#[cfg(unix)]
fn owned_by_me(paths: &[&Path]) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    let me = crate::channel::peer::current_uid();
    Some(
        paths
            .iter()
            .all(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.uid() == me)),
    )
}

/// No owner to compare: the engine's verdict (`safe.directory` of gix) rules.
#[cfg(not(unix))]
fn owned_by_me(_paths: &[&Path]) -> Option<bool> {
    None
}

/// Reads the [`WorktreeFacts`] of worktree `w` of `repo`; `home` is the folder SEC-11 refuses
/// as a root.
pub(crate) fn facts(repo: &RepoView, w: usize, home: Option<&Path>) -> WorktreeFacts {
    let Some(view) = repo.worktrees.get(w) else {
        return WorktreeFacts {
            exists: false,
            owned_by_me: None,
            trusted_link: false,
        };
    };
    let root = Path::new(view.path.raw());
    let common = Path::new(repo.path.raw());
    // The root itself, not what a symlink there points at.
    let root_meta = std::fs::symlink_metadata(root).ok();
    let exists = root_meta.is_some();
    let root_is_link = root_meta.is_some_and(|m| m.file_type().is_symlink());
    // A root that is not there has no owner: the common dir still has.
    let owners: Vec<&Path> = if exists {
        vec![root, common]
    } else {
        vec![common]
    };
    // A root that is a symlink is not the folder the repo was observed in.
    let trusted_link = !root_is_link
        && (view.main
            || view
                .admin_name
                .as_ref()
                .is_some_and(|id| observe::linked_is_trusted_in(common, id.raw(), root, home)));
    WorktreeFacts {
        exists,
        owned_by_me: owned_by_me(&owners),
        trusted_link,
    }
}

/// Whether worktree `w` of `repo` can be read now: its root, its owner, SEC-11 and what the
/// engine published about it and about the repo.
pub(crate) fn availability(
    repo: &RepoView,
    w: usize,
    facts: &WorktreeFacts,
) -> Result<(), McpUnavailable> {
    if repo.state == RepoStateView::Unavailable {
        return Err(McpUnavailable::RepoUnreadable);
    }
    if !facts.exists {
        return Err(McpUnavailable::WorktreeMissing);
    }
    if facts.owned_by_me == Some(false) {
        return Err(McpUnavailable::OtherOwner);
    }
    if !facts.trusted_link {
        return Err(McpUnavailable::WorktreeUntrusted);
    }
    match repo.worktrees.get(w).map(|view| &view.status) {
        Some(WorktreeStatus::Ready { .. }) => Ok(()),
        Some(WorktreeStatus::Unavailable { reason }) => Err(match reason {
            UnavailableReason::Missing => McpUnavailable::WorktreeMissing,
            UnavailableReason::Untrusted => McpUnavailable::WorktreeUntrusted,
            UnavailableReason::Unreadable => McpUnavailable::RepoUnreadable,
        }),
        None => Err(McpUnavailable::RepoUnreadable),
    }
}

/// Why a read is refused before any data: the allowlist first, then availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    NotAllowlisted,
    Unavailable(McpUnavailable),
}

/// The checks of an `mcp.status` after the scope, in their order: a repo outside the allowlist
/// is refused as such whatever its state, so nothing of it is told before the developer enabled
/// it.
pub(crate) fn admit(
    allowed: bool,
    repo: &RepoView,
    w: usize,
    facts: &WorktreeFacts,
) -> Result<(), Refusal> {
    if !allowed {
        return Err(Refusal::NotAllowlisted);
    }
    availability(repo, w, facts).map_err(Refusal::Unavailable)
}

/// Where the caller is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Located {
    /// In worktree `worktree` of repo `repo` (indexes into the repos and its worktrees).
    Worktree { repo: usize, worktree: usize },
    /// Nowhere any more, but the last scope this connection served was in repo `repo` and its
    /// root is gone: the worktree was deleted during the session.
    Missing { repo: usize },
    /// In no observed worktree, or not known to be where it was.
    Outside,
}

/// The scope of a call. `cwd` is `None` when the working folder cannot be read or made
/// canonical; `last` is the repo id and root of the last scope served on this connection.
pub(crate) fn locate_with_last(
    cwd: Option<&Path>,
    repos: &[RepoView],
    last: Option<&(String, PathBuf)>,
) -> Located {
    if let Some(cwd) = cwd {
        return super::mcp_scope::locate(cwd, repos).map_or(
            Located::Outside,
            |(repo, worktree)| Located::Worktree { repo, worktree },
        );
    }
    // The memory only stands in for a folder that is gone: one that is merely unreadable is not
    // a deleted worktree.
    let Some((id, root)) = last else {
        return Located::Outside;
    };
    match repos.iter().position(|r| r.repo_id == *id) {
        Some(repo) if matches!(root.try_exists(), Ok(false)) => Located::Missing { repo },
        _ => Located::Outside,
    }
}

/// The name of a worktree: the last component of its root.
pub(crate) fn worktree_name(root: &str) -> UntrustedName {
    UntrustedName::new(
        Path::new(root)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

/// The branch `view` is on (or will be, when unborn).
pub(crate) fn branch_of(view: &WorktreeView) -> Option<UntrustedName> {
    match &view.status {
        WorktreeStatus::Ready {
            head: HeadView::Branch { name } | HeadView::Unborn { name },
            ..
        } => Some(name.clone()),
        _ => None,
    }
}

fn list_ref(total: u64) -> Option<McpListRef> {
    (total > 0).then(|| McpListRef {
        total,
        cursor: MCP_CURSOR_PLACEHOLDER.to_owned(),
    })
}

/// The situation of one worktree: its present sessions (not `own_session`), its changes as a
/// reference and its distance to the base. What there is nothing to say about is left out.
fn situation(view: &WorktreeView, ctx: &McpContext, own_session: Option<&str>) -> McpHere {
    let mut here = McpHere::default();
    let sessions: Vec<McpSession> = ctx
        .sessions
        .iter()
        .filter(|s| {
            s.worktree.raw() == view.path.raw()
                && s.state != SessionStateView::Ended
                && own_session != Some(s.session_id.as_str())
        })
        .map(|s| McpSession {
            actor: s.actor.clone(),
            state: s.state,
        })
        .collect();
    if sessions.len() > MCP_MAX_SESSIONS {
        here.sessions_total = u32::try_from(sessions.len()).ok();
    }
    here.sessions = sessions.into_iter().take(MCP_MAX_SESSIONS).collect();
    if let WorktreeStatus::Ready {
        counts, divergence, ..
    } = &view.status
    {
        here.changes = list_ref(counts.total());
        match divergence {
            DivergenceView::Counted { ahead, behind } => {
                here.ahead = (ahead.count > 0).then_some(ahead.count);
                here.behind = (behind.count > 0).then_some(behind.count);
                here.at_least = !ahead.exact || !behind.exact;
            }
            DivergenceView::BaseMissing => here.uncounted = Some(McpUncounted::BaseMissing),
            DivergenceView::NoBase => here.uncounted = Some(McpUncounted::NoBase),
            DivergenceView::NoCommits => here.uncounted = Some(McpUncounted::NoCommits),
            DivergenceView::Unreadable => here.uncounted = Some(McpUncounted::Unreadable),
        }
    }
    here
}

/// Seconds from `then_ms` to `now_ms`, never negative.
fn seconds_ago(now_ms: i64, then_ms: i64) -> u64 {
    u64::try_from(now_ms.saturating_sub(then_ms) / 1000).unwrap_or(0)
}

/// The observation gaps that touch the last [`MCP_GAP_WINDOW`], the most recent
/// [`MCP_MAX_GAPS`] of them, and how many there are in all when that is more.
fn gaps_of(ctx: &McpContext, now_ms: i64) -> (Vec<McpGap>, Option<u32>) {
    let window_ms = i64::try_from(MCP_GAP_WINDOW.as_millis()).unwrap_or(i64::MAX);
    let from = now_ms.saturating_sub(window_ms);
    let mut touching: Vec<&crate::profile::Gap> = ctx
        .gaps
        .iter()
        .filter(|g| g.ended_ms.is_none_or(|end| end >= from))
        .collect();
    touching.sort_by_key(|g| std::cmp::Reverse(g.started_ms));
    let total = (touching.len() > MCP_MAX_GAPS)
        .then(|| u32::try_from(touching.len()).ok())
        .flatten();
    let gaps = touching
        .into_iter()
        .take(MCP_MAX_GAPS)
        .map(|g| McpGap {
            from_s_ago: seconds_ago(now_ms, g.started_ms),
            to_s_ago: g.ended_ms.map(|end| seconds_ago(now_ms, end)),
        })
        .collect();
    (gaps, total)
}

/// The default answer: the caller's situation (`None` when there is nothing to say) and the
/// repo's. Lists are references with a placeholder cursor.
pub(crate) fn default_status(
    repo: &RepoView,
    w: usize,
    ctx: &McpContext,
    own_session: Option<&str>,
    now_ms: i64,
) -> (Option<McpHere>, McpRepo) {
    let here = repo
        .worktrees
        .get(w)
        .map(|view| situation(view, ctx, own_session))
        .filter(|here| *here != McpHere::default());
    let (gaps, gaps_total) = gaps_of(ctx, now_ms);
    let others = repo.worktrees.len().saturating_sub(1);
    let hooks = ctx.guard.hooks.as_ref();
    let repo_part = McpRepo {
        engine: match repo.tier {
            Some(RepoTier::Waking) => Some(McpEngineState::Reconciling),
            Some(RepoTier::Dormant) => Some(McpEngineState::Dormant),
            _ => None,
        },
        base: McpBase {
            name: repo.base.name.clone(),
            state: match repo.base.status {
                BaseStatusView::Confirmed => McpBaseState::Confirmed,
                BaseStatusView::Unconfirmed => McpBaseState::Unconfirmed,
                BaseStatusView::Invalid => McpBaseState::Invalid,
            },
        },
        protection: match ctx.guard.state {
            ProtectionState::HooksOnly => McpProtectionState::Full,
            ProtectionState::Unprotected => McpProtectionState::McpOnly,
        },
        protection_lost: hooks
            .filter(|h| h.status == HooksStatus::Inactive)
            .and_then(|h| h.cause),
        diagnostics: ctx.guard.diagnostics.clone(),
        fetch_age_s: repo.fetched_utc_ms.map(|at| seconds_ago(now_ms, at)),
        gaps,
        gaps_total,
        sessions_unknown: !ctx.detection_available,
        worktrees: list_ref(others as u64),
    };
    (here, repo_part)
}

/// The worktrees other than `w` that come after `after` in the order of the pages: the main one
/// first, then by root.
fn others_after(repo: &RepoView, w: usize, after: Option<&Path>) -> Vec<usize> {
    let mut others: Vec<(bool, &Path, usize)> = repo
        .worktrees
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != w)
        .map(|(i, view)| (!view.main, Path::new(view.path.raw()), i))
        .collect();
    others.sort();
    // `after` is a position, not a member: a worktree removed since the last page still works.
    let from = after.map(|a| {
        let main = repo
            .worktrees
            .iter()
            .any(|view| view.main && Path::new(view.path.raw()) == a);
        (!main, a)
    });
    others
        .into_iter()
        .filter(|(not_main, root, _)| from.is_none_or(|from| (*not_main, *root) > from))
        .map(|(_, _, i)| i)
        .collect()
}

/// The indexes of the worktrees [`worktrees_page`] lists for the same arguments, in its order.
pub(crate) fn page_indexes(repo: &RepoView, w: usize, after: Option<&Path>) -> Vec<usize> {
    let mut rest = others_after(repo, w, after);
    rest.truncate(MCP_WORKTREES_PAGE);
    rest
}

/// One page of the worktrees other than `w`, after `after`: each with its branch, sessions,
/// changes and distance to the base. `truncated` and a placeholder cursor while some remain.
pub(crate) fn worktrees_page(
    repo: &RepoView,
    w: usize,
    ctx: &McpContext,
    after: Option<&Path>,
) -> McpPage {
    let rest = others_after(repo, w, after);
    let truncated = rest.len() > MCP_WORKTREES_PAGE;
    let worktrees = rest
        .into_iter()
        .take(MCP_WORKTREES_PAGE)
        .filter_map(|i| repo.worktrees.get(i))
        .map(|view| McpWorktree {
            name: worktree_name(view.path.raw()),
            branch: branch_of(view),
            main: view.main,
            unavailable: match &view.status {
                WorktreeStatus::Unavailable { reason } => Some(*reason),
                WorktreeStatus::Ready { .. } => None,
            },
            state: situation(view, ctx, None),
        })
        .collect();
    McpPage {
        of: None,
        total: repo.worktrees.len().saturating_sub(1) as u64,
        worktrees,
        paths: Vec::new(),
        truncated,
        cursor: truncated.then(|| MCP_CURSOR_PLACEHOLDER.to_owned()),
    }
}

/// One page of the paths of a worktree: the unique paths after `after`, at most
/// `MCP_PATHS_PAGE`, in order, with the placeholder cursor while there are more. `changes` is the
/// full list as `(path, area)` sorted; `counts` gives the total.
pub(crate) fn paths_page(
    of: UntrustedName,
    counts: ChangeCounts,
    changes: &[FileChangeView],
    after: Option<&str>,
) -> McpPage {
    let mut paths: Vec<Untrusted> = Vec::new();
    let mut previous: Option<&str> = None;
    for change in changes {
        let path = change.path.raw();
        // Sorted by path then area: a path in two areas is adjacent to itself.
        if previous == Some(path) {
            continue;
        }
        previous = Some(path);
        if after.is_some_and(|after| path <= after) {
            continue;
        }
        paths.push(change.path.clone());
        // One more than a page tells whether another page exists.
        if paths.len() > MCP_PATHS_PAGE {
            break;
        }
    }
    let truncated = paths.len() > MCP_PATHS_PAGE;
    paths.truncate(MCP_PATHS_PAGE);
    McpPage {
        of: Some(of),
        total: counts.total(),
        worktrees: Vec::new(),
        paths,
        truncated,
        cursor: truncated.then(|| MCP_CURSOR_PLACEHOLDER.to_owned()),
    }
}

/// What a cursor stands for: the list and where it went on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorEntry {
    Worktrees {
        repo_id: String,
        after: Option<PathBuf>,
    },
    Paths {
        repo_id: String,
        root: PathBuf,
        after: Option<String>,
    },
}

impl CursorEntry {
    /// The repo the cursor was handed out for.
    pub(crate) fn repo_id(&self) -> &str {
        match self {
            Self::Worktrees { repo_id, .. } | Self::Paths { repo_id, .. } => repo_id,
        }
    }
}

/// The cursors of one connection: opaque handles into the daemon's memory, at most
/// `MCP_MAX_CURSORS`, the oldest dropped first.
#[derive(Debug, Default)]
pub(crate) struct McpCursors {
    entries: VecDeque<(String, CursorEntry)>,
}

/// Tries before giving up on a handle that does not collide: a collision of 64 random bits
/// is not expected once, let alone repeatedly.
const MINT_TRIES: usize = 8;

impl McpCursors {
    /// A new cursor for `entry`; `None` when the system cannot give random bytes.
    pub(crate) fn mint(&mut self, entry: CursorEntry) -> Option<String> {
        for _ in 0..MINT_TRIES {
            let mut bytes = [0u8; 8];
            getrandom::fill(&mut bytes).ok()?;
            let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            if id == MCP_CURSOR_PLACEHOLDER || self.entries.iter().any(|(known, _)| *known == id) {
                continue;
            }
            self.entries.push_back((id.clone(), entry));
            while self.entries.len() > MCP_MAX_CURSORS {
                self.entries.pop_front();
            }
            return Some(id);
        }
        None
    }

    /// The entry of a cursor of this table; `None` for any other.
    pub(crate) fn get(&self, id: &str) -> Option<&CursorEntry> {
        self.entries
            .iter()
            .find_map(|(known, entry)| (known == id).then_some(entry))
    }
}

#[cfg(test)]
#[path = "mcp_status_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mcp_status_more_tests.rs"]
mod more_tests;
