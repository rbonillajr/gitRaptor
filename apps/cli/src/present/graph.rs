//! The graph of the agents' branches over the base (DS-US-CKP-022): the panel of the fleet, fed
//! by the stream, and the rows of the full-screen view, fed by `graph.lanes`.
//!
//! This is the only place where the untrusted names and subjects of a [`GraphLanes`] become
//! [`SafeText`] (SEC-12): the model and the view hold [`Shown`], never the response.

use gitraptor_api::graph::{
    BaseGap, GraphBase, GraphCut, GraphLanes, LaneGap, LaneHead, LaneState,
};
use gitraptor_api::messages::{CommitCountView, DivergenceView};

use super::SafeText;
use super::graph_text::GraphText;
use super::i18n::Text;
use super::ingest::{key, leaf};
use crate::model::{Head, Model, RepoView, SessionRow, WorktreeRow, WorktreeState};
use crate::tui::fleet_order;
use crate::tui::graph::Anchor;
use crate::tui::selection::ordered_rows;
use crate::tui::view::{Say, agent_name};
use crate::tui::widgets::graph_lanes::{Base, GraphModel, LaneModel, LaneOwner};

/// Hex digits of a hash on screen.
const SHORT: usize = 7;

/// The last answer of `graph.lanes`, sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub at_utc_ms: i64,
    pub base: ShownBase,
    pub lanes: Vec<ShownLane>,
    pub cut: Option<GraphCut>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShownBase {
    Ready(SafeText),
    NotComputable(ShownGap),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShownGap {
    Unconfirmed(SafeText),
    NoBase,
    Missing(SafeText),
    RepoUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownLane {
    /// The key a worktree row and its sessions share.
    pub key: u64,
    /// The branch, or the folder of a worktree without one.
    pub branch: SafeText,
    pub state: ShownState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShownState {
    Ready {
        merge_base: String,
        total: CommitCountView,
        commits: Vec<ShownCommit>,
    },
    NotComputable(LaneGap),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownCommit {
    /// Hex digits only.
    pub oid: String,
    pub summary: SafeText,
    pub at_utc_ms: i64,
    pub merge: bool,
}

impl ShownCommit {
    pub fn short(&self) -> &str {
        &self.oid[..self.oid.len().min(SHORT)]
    }
}

/// Hex digits of an object id as the engine sent it; anything else is dropped.
fn hex(raw: &str) -> String {
    raw.chars().filter(char::is_ascii_hexdigit).collect()
}

/// The answer as sanitized text.
pub fn ingest(lanes: &GraphLanes) -> Shown {
    let base = match &lanes.base {
        GraphBase::Ready { name } => ShownBase::Ready(SafeText::name_from_untrusted(name)),
        GraphBase::NotComputable { reason } => ShownBase::NotComputable(match reason {
            BaseGap::BaseUnconfirmed { name } => {
                ShownGap::Unconfirmed(SafeText::name_from_untrusted(name))
            }
            BaseGap::NoBase => ShownGap::NoBase,
            BaseGap::BaseMissing { name } => ShownGap::Missing(SafeText::name_from_untrusted(name)),
            BaseGap::RepoUnavailable => ShownGap::RepoUnavailable,
        }),
    };
    let lanes_shown = lanes
        .lanes
        .iter()
        .map(|lane| ShownLane {
            key: key(lane.worktree.raw()),
            branch: match &lane.head {
                LaneHead::Branch { name } => SafeText::name_from_untrusted(name),
                LaneHead::Detached => SafeText::name(&leaf(lane.worktree.raw())),
            },
            state: match &lane.state {
                LaneState::Ready {
                    merge_base,
                    commits,
                    total,
                } => ShownState::Ready {
                    merge_base: hex(merge_base),
                    total: *total,
                    commits: commits
                        .iter()
                        .map(|c| ShownCommit {
                            oid: hex(&c.oid),
                            summary: SafeText::from_untrusted(&c.summary),
                            at_utc_ms: c.committed_utc_ms,
                            merge: c.merge,
                        })
                        .collect(),
                },
                LaneState::NotComputable { reason } => ShownState::NotComputable(*reason),
            },
        })
        .collect();
    Shown {
        at_utc_ms: lanes.computed_at_utc_ms,
        base,
        lanes: lanes_shown,
        cut: lanes.cut,
    }
}

impl Shown {
    /// Where a scroll can stop, top to bottom: each lane's header and each of its commits.
    pub fn stops(&self) -> Vec<Anchor> {
        let mut stops = Vec::new();
        for lane in &self.lanes {
            stops.push(Anchor::Lane(lane.key));
            if let ShownState::Ready { commits, .. } = &lane.state {
                stops.extend(commits.iter().map(|c| Anchor::Commit {
                    lane: lane.key,
                    oid: c.oid.clone(),
                }));
            }
        }
        stops
    }
}

/// The color slot and name of the first agent of a worktree, as its fleet row shows them.
fn first_agent(model: &Model, repo: &RepoView, worktree: u64, say: Say) -> Option<(usize, String)> {
    if repo.detection != Some(true) {
        return None;
    }
    let shown = fleet_order::shown(&repo.sessions, worktree, model.now_ms, model.ui.show_ended);
    let first: &SessionRow = shown.sessions.first()?;
    Some((
        model.ui.colors.index(&first.session_id).unwrap_or(0),
        agent_name(first, say),
    ))
}

/// One row of the full-screen view. Sentences come from the catalog; names and subjects are
/// already sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRow {
    /// A message that stands in for the lanes, or the reason a lane has no commits.
    Note { text: SafeText, warning: bool },
    Header {
        anchor: Anchor,
        color: Option<usize>,
        title: SafeText,
        since: Option<SafeText>,
        reason: Option<SafeText>,
    },
    Commit {
        anchor: Anchor,
        color: Option<usize>,
        short: String,
        summary: SafeText,
        merge: Option<SafeText>,
        unattributed: SafeText,
        ago: SafeText,
    },
    /// "22 more", "truncated: …".
    Tail(SafeText),
}

impl GraphRow {
    pub fn anchor(&self) -> Option<&Anchor> {
        match self {
            Self::Header { anchor, .. } | Self::Commit { anchor, .. } => Some(anchor),
            Self::Note { .. } | Self::Tail(_) => None,
        }
    }
}

/// The rows of the view, in order.
pub(crate) fn rows(shown: &Shown, model: &Model, say: Say) -> Vec<GraphRow> {
    let repo = model.engine.repo.as_ref().and_then(|r| r.data.as_ref());
    let base = match &shown.base {
        ShownBase::Ready(name) => name.as_str(),
        ShownBase::NotComputable(gap) => {
            return base_rows(gap, say);
        }
    };
    let mut rows = Vec::new();
    for lane in &shown.lanes {
        let agent = repo.and_then(|r| first_agent(model, r, lane.key, say));
        let color = agent.as_ref().map(|(index, _)| *index);
        let title = say.graph(GraphText::LaneTitle {
            branch: lane.branch.as_str(),
            agent: agent.as_ref().map(|(_, name)| name.as_str()),
        });
        let anchor = Anchor::Lane(lane.key);
        match &lane.state {
            ShownState::Ready {
                merge_base,
                total,
                commits,
            } => {
                let since = say.graph(GraphText::Since {
                    count: total.count,
                    exact: total.exact,
                    merge_base: &merge_base[..merge_base.len().min(SHORT)],
                });
                rows.push(GraphRow::Header {
                    anchor,
                    color,
                    title,
                    since: Some(since),
                    reason: None,
                });
                for commit in commits {
                    rows.push(GraphRow::Commit {
                        anchor: Anchor::Commit {
                            lane: lane.key,
                            oid: commit.oid.clone(),
                        },
                        color,
                        short: commit.short().to_owned(),
                        summary: commit.summary.clone(),
                        merge: commit.merge.then(|| say.graph(GraphText::Merge)),
                        unattributed: say.graph(GraphText::Unattributed),
                        ago: say.safe(Text::Ago(model.now_ms.saturating_sub(commit.at_utc_ms))),
                    });
                }
                let listed = u64::try_from(commits.len()).unwrap_or(u64::MAX);
                let hidden = total.count.saturating_sub(listed);
                if hidden > 0 {
                    rows.push(GraphRow::Tail(say.graph(GraphText::More {
                        count: hidden,
                        exact: total.exact,
                    })));
                }
            }
            ShownState::NotComputable(gap) => {
                let reason = match gap {
                    LaneGap::WorktreeUnavailable => GraphText::WorktreeUnavailable,
                    LaneGap::NoCommits => GraphText::NoCommits,
                    LaneGap::NoMergeBase => GraphText::NoMergeBase { base },
                    LaneGap::TooSlow => GraphText::TooSlow,
                    LaneGap::Unreadable => GraphText::Unreadable,
                };
                rows.push(GraphRow::Header {
                    anchor,
                    color,
                    title,
                    since: None,
                    reason: Some(say.graph(reason)),
                });
            }
        }
    }
    if let Some(cut) = shown.cut {
        rows.push(GraphRow::Tail(say.graph(match cut {
            GraphCut::Lanes => GraphText::CutLanes,
            GraphCut::Bytes => GraphText::CutBytes,
        })));
    }
    rows
}

/// The message of a base that cannot be computed, and how to fix it when there is a way.
fn base_rows(gap: &ShownGap, say: Say) -> Vec<GraphRow> {
    let note = |text: SafeText| GraphRow::Note {
        text,
        warning: true,
    };
    match gap {
        ShownGap::Unconfirmed(name) => vec![
            note(say.graph(GraphText::BaseUnconfirmed {
                name: name.as_str(),
            })),
            GraphRow::Note {
                text: say.graph(GraphText::ConfirmAction),
                warning: false,
            },
        ],
        ShownGap::NoBase => vec![note(say.graph(GraphText::NoBase))],
        ShownGap::Missing(name) => vec![note(say.graph(GraphText::BaseMissing {
            name: name.as_str(),
        }))],
        ShownGap::RepoUnavailable => vec![note(say.graph(GraphText::RepoUnavailable))],
    }
}

/// Whether the worktree has the base branch checked out: that is the trunk, not a lane.
fn is_trunk(row: &WorktreeRow, base: &SafeText) -> bool {
    matches!(
        &row.state,
        WorktreeState::Ready { head: Head::Branch(name), .. } if name == base
    )
}

/// The panel of the fleet: one lane per worktree but the trunk, in the order of the fleet, from
/// what the stream publishes. `None` when there is neither a lane nor a message about the base.
pub(crate) fn panel(model: &Model, say: Say) -> Option<GraphModel> {
    let repo = model.engine.repo.as_ref().and_then(|r| r.data.as_ref())?;
    let line = |text: GraphText<'_>| say.graph(text);
    let base = match &repo.base {
        None => Base::Missing(line(GraphText::NoBase)),
        Some(name) if !repo.base_confirmed => Base::Missing(line(GraphText::BaseUnconfirmedLine {
            name: name.as_str(),
        })),
        Some(name) if crate::tui::engine_state::base_not_found(repo).is_some() => {
            Base::Missing(line(GraphText::BaseMissing {
                name: name.as_str(),
            }))
        }
        Some(name) => Base::Found(name.clone()),
    };
    let lanes: Vec<LaneModel> = match &base {
        Base::Missing(_) => Vec::new(),
        Base::Found(name) => ordered_rows(model)
            .into_iter()
            .filter(|w| !is_trunk(w, name))
            .map(|w| lane_model(model, repo, w, say))
            .collect(),
    };
    if lanes.is_empty() && matches!(base, Base::Found(_)) {
        return None;
    }
    Some(GraphModel {
        title: line(GraphText::PanelTitle),
        base,
        lanes,
        more: line(GraphText::PanelMore),
        focused: false,
    })
}

fn lane_model(model: &Model, repo: &RepoView, row: &WorktreeRow, say: Say) -> LaneModel {
    let (ahead, behind) = match &row.state {
        WorktreeState::Ready {
            divergence: DivergenceView::Counted { ahead, behind },
            ..
        } => (count(ahead.count), count(behind.count)),
        _ => (0, 0),
    };
    let owner = match first_agent(model, repo, row.key, say) {
        Some((color_index, agent)) => LaneOwner::Agent {
            color_index,
            name: SafeText::name(&say.text(Text::AgentInWorktree {
                agent: &agent,
                worktree: row.name.as_str(),
            })),
        },
        None => LaneOwner::Unattributed(say.graph(GraphText::LaneTitle {
            branch: row.name.as_str(),
            agent: None,
        })),
    };
    LaneModel {
        owner,
        ahead,
        behind,
        conflict: false,
    }
}

fn count(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
