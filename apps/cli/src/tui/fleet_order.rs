//! What each fleet row shows of its worktree's sessions and where the row goes. Pure: `view`
//! recomputes it every frame, so a row only moves when it changes group.

use gitraptor_api::messages::{SessionEndCauseView, SessionStateView};

use crate::model::{RepoView, SessionRow, WorktreeRow};

/// An ended session stays visible this long after its end (24 h).
pub const ENDED_VISIBLE_MS: i64 = 86_400_000;

/// How the end time of a session is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    /// The engine published the end time.
    Published,
    /// It ended during an observation gap: the time is when the engine detected it.
    Unobserved,
    /// No time published and no gap to blame.
    NotAvailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndedAt {
    pub at_ms: i64,
    pub kind: EndKind,
}

/// The sessions a worktree row shows, best first.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Shown<'r> {
    pub sessions: Vec<&'r SessionRow>,
    /// The latest ended session is not shown by default.
    pub hidden: bool,
    /// The ended session to name on a row that has no agent now.
    pub last_agent: Option<&'r SessionRow>,
}

/// Position of a row in the fleet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Main,
    Attention,
    Active,
    Inactive,
    Ended,
    NoAgent,
}

/// When a session ended; `None` while it is present.
pub fn ended_at(session: &SessionRow) -> Option<EndedAt> {
    if session.state != SessionStateView::Ended {
        return None;
    }
    Some(match (session.ended_ms, session.end_cause) {
        (Some(at_ms), _) => EndedAt {
            at_ms,
            kind: EndKind::Published,
        },
        (None, Some(SessionEndCauseView::EndedDuringGap)) => EndedAt {
            at_ms: session.state_since_ms,
            kind: EndKind::Unobserved,
        },
        (None, _) => EndedAt {
            at_ms: session.state_since_ms,
            kind: EndKind::NotAvailable,
        },
    })
}

fn rank(state: SessionStateView) -> u8 {
    match state {
        SessionStateView::Active => 0,
        SessionStateView::Inactive => 1,
        SessionStateView::Ended => 2,
    }
}

/// The sessions of a worktree to show at `now_ms`. The latest ended one (by start) shows for
/// 24 h unless a later session started after its end, or always with `show_ended`.
pub fn shown(sessions: &[SessionRow], worktree: u64, now_ms: i64, show_ended: bool) -> Shown<'_> {
    let mine = || sessions.iter().filter(|s| s.worktree == worktree);
    let mut out: Vec<&SessionRow> = mine()
        .filter(|s| s.state != SessionStateView::Ended)
        .collect();
    let latest = mine()
        .filter(|s| s.state == SessionStateView::Ended)
        .max_by(|a, b| (a.started_ms, &a.session_id).cmp(&(b.started_ms, &b.session_id)));
    let mut hidden = false;
    let mut last_agent = None;
    if let Some(ended) = latest {
        let visible = ended_at(ended).is_some_and(|end| {
            now_ms.saturating_sub(end.at_ms) < ENDED_VISIBLE_MS
                && !mine().any(|s| s.started_ms > end.at_ms)
        });
        hidden = !visible;
        if visible || show_ended {
            out.push(ended);
        } else if out.is_empty() {
            last_agent = Some(ended);
        }
    }
    out.sort_by(|a, b| {
        (
            rank(a.state),
            std::cmp::Reverse(a.state_since_ms),
            &a.session_id,
        )
            .cmp(&(
                rank(b.state),
                std::cmp::Reverse(b.state_since_ms),
                &b.session_id,
            ))
    });
    Shown {
        sessions: out,
        hidden,
        last_agent,
    }
}

/// Two or more present sessions. Provisional: the engine does not publish "shared" yet
/// (US-GRP-011); an exception to BR-CKP-CALC-001 until it does, then this is deleted. An ended
/// session never counts, shown or not.
pub fn is_shared(shown: &Shown<'_>) -> bool {
    shown
        .sessions
        .iter()
        .filter(|s| s.state != SessionStateView::Ended)
        .count()
        >= 2
}

/// The group of a row: the main worktree, what asks for attention, then its lead session.
pub fn group(main: bool, attention: bool, lead: Option<SessionStateView>) -> Group {
    if main {
        return Group::Main;
    }
    if attention {
        return Group::Attention;
    }
    match lead {
        Some(SessionStateView::Active) => Group::Active,
        Some(SessionStateView::Inactive) => Group::Inactive,
        Some(SessionStateView::Ended) => Group::Ended,
        None => Group::NoAgent,
    }
}

/// The sort key: group, then folder name, path and key, so the order never depends on
/// activity.
pub fn order_key(group: Group, worktree: &WorktreeRow) -> (Group, &str, &str, u64) {
    (
        group,
        worktree.name.as_str(),
        worktree.path.as_str(),
        worktree.key,
    )
}

/// How many worktrees have an ended session hidden by default (the `f` hint count).
pub fn hidden_count(repo: &RepoView, now_ms: i64) -> usize {
    if repo.detection != Some(true) {
        return 0;
    }
    repo.worktrees
        .iter()
        .filter(|w| shown(&repo.sessions, w.key, now_ms, false).hidden)
        .count()
}

#[cfg(test)]
#[path = "fleet_order_tests.rs"]
mod tests;
