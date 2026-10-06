//! `view(&Model, &mut Frame)` (ADR-CKP-003 § 2 and § 7): pure, it only
//! reads the model. Below 80×24 it paints only the minimum-size screen.
//!
//! It composes the widget library (TS-CKP-005, `tui::widgets`): the header
//! is the StatusBar, the body the fleet (AgentList, US-CKP-001) and the last
//! line the KeyHints. This module only derives each widget's view model from
//! the engine replica and the catalog; no literal colors or glyphs here
//! (ADR-GRP-003). Nothing is computed that the engine does not publish: what
//! it does not publish is "not available" (BR-CKP-CALC-001).

use gitraptor_api::AgentKind;
use gitraptor_api::messages::{DivergenceView, SessionStateView};
use ratatui::Frame;

use crate::model::{
    ConnState, Head, Model, RepoView, Requester, SafeText, SessionRow, WorktreeRow, WorktreeState,
};
use crate::present::i18n::{Lang, Text};
use crate::tui::keymap::{Action, BINDINGS};
use crate::tui::style::Styles;
use crate::tui::widgets::agent_list::{
    AgentColumns, AgentListModel, AgentRowModel, AgentState, Branch, Sync,
};
use crate::tui::widgets::key_hints::{KeyHint, KeyHintsModel};
use crate::tui::widgets::layout::{self, BodyPlan, Connection, StatusBarModel, TooSmallModel};
use crate::tui::widgets::themed;

/// Smallest size with the full layout (Q-CKP-18).
pub const MIN_WIDTH: u16 = layout::MIN_WIDTH;
pub const MIN_HEIGHT: u16 = layout::MIN_HEIGHT;

pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let styles = Styles::new(model.ui.theme);
    let lang = model.ui.lang;
    let Some(regions) = layout::layout(area, BodyPlan::default()) else {
        let small = TooSmallModel {
            message: catalog(
                Text::TooSmall {
                    width: area.width,
                    height: area.height,
                },
                lang,
            ),
            quit: hint(Action::Quit, lang)
                .unwrap_or_else(|| KeyHint::new(SafeText::text("q"), catalog(Text::KeyQuit, lang))),
        };
        frame.render_widget(themed(&small, &styles), area);
        return;
    };
    frame.render_widget(themed(&status_bar(model, &styles), &styles), regions.header);
    frame.render_widget(themed(&fleet(model, &styles), &styles), regions.list);
    frame.render_widget(themed(&key_hints(lang), &styles), regions.hints);
}

fn catalog(text: Text<'_>, lang: Lang) -> SafeText {
    SafeText::text(&text.render(lang))
}

/// The selected repo when its snapshot arrived.
fn repo(model: &Model) -> Option<&RepoView> {
    model.engine.repo.as_ref().and_then(|r| r.data.as_ref())
}

/// The header: repo, connection (with the engine state, staleness and the answer to the last
/// key), requester and the ⚡/⛔ counts the engine publishes.
pub fn status_bar(model: &Model, styles: &Styles) -> StatusBarModel {
    let lang = model.ui.lang;
    let repo = repo(model);
    let connection = match model.conn {
        ConnState::Live => Connection::Live,
        ConnState::Connecting
        | ConnState::Syncing
        | ConnState::Resyncing
        | ConnState::Reconnecting { .. } => Connection::Reconnecting,
        ConnState::EngineUnavailable
        | ConnState::Incompatible
        | ConnState::Rejected
        | ConnState::Unsupported => Connection::Unavailable,
    };
    let engine = model.engine.global.data.as_ref().map(|g| g.engine);
    let mut label = vec![Text::Conn(model.conn).render(lang)];
    if model.conn == ConnState::Live {
        label.push(Text::Engine(engine).render(lang));
    }
    let stale = model.engine.global.stale || model.engine.repo.as_ref().is_some_and(|r| r.stale);
    if stale {
        label.push(Text::Stale.render(lang));
    }
    if let Some(notice) = model.ui.notice {
        label.push(Text::Notice(notice).render(lang));
    }
    let requester = match &model.engine.requester {
        None => Text::RequesterUnknown,
        Some(Requester::Unattributed { .. }) => Text::ActingAsYourself,
        Some(Requester::Agent { name, .. }) => Text::ActingAsAgent(name.as_ref()),
        Some(Requester::Unverified) => Text::Unverified,
    };
    let attention = repo.and_then(|r| {
        model
            .engine
            .global
            .data
            .as_ref()?
            .attention
            .iter()
            .find(|a| a.repo_id == r.repo_id)
    });
    StatusBarModel {
        repo: match repo {
            Some(r) => r.name.clone(),
            None => catalog(Text::NoRepo, lang),
        },
        connection,
        connection_label: SafeText::text(&label.join(styles.glyphs.separator)),
        // Protection is published by Guardrails (TS-GRD); not shown until then.
        protection: None,
        requester: catalog(requester, lang),
        conflicts: attention.and_then(|a| a.conflicts),
        blocked: attention.and_then(|a| a.denials),
    }
}

/// The fleet of the selected repo (US-CKP-001): one row per worktree, the main one first and
/// the rest in the order the engine published them (the order by attention is US-CKP-002's).
pub fn fleet(model: &Model, styles: &Styles) -> AgentListModel {
    let lang = model.ui.lang;
    let repo = repo(model);
    let rows = repo.map(|r| rows(r, lang, styles)).unwrap_or_default();
    let empty = match (&model.engine.repo, repo) {
        (Some(_), Some(_)) => Text::FleetEmpty,
        (Some(_), None) => Text::FleetWaiting,
        (None, _) if model.conn == ConnState::Live => Text::FleetNoRepo,
        (None, _) => Text::FleetWaiting,
    };
    AgentListModel {
        title: catalog(
            match repo {
                Some(r) => Text::FleetTitle {
                    base: r.base.as_ref(),
                },
                None => Text::FleetTitle { base: None },
            },
            lang,
        ),
        columns: AgentColumns {
            agent: catalog(Text::ColAgent, lang),
            branch: catalog(Text::ColBranch, lang),
            changes: catalog(Text::ColFiles, lang),
            sync: catalog(Text::ColSync, lang),
            activity: catalog(Text::ColActivity, lang),
        },
        rows,
        selected: None,
        offset: 0,
        focused: true,
        empty: catalog(empty, lang),
    }
}

fn rows(repo: &RepoView, lang: Lang, styles: &Styles) -> Vec<AgentRowModel> {
    let ordered = repo
        .worktrees
        .iter()
        .filter(|w| w.main)
        .chain(repo.worktrees.iter().filter(|w| !w.main));
    let mut agents = 0;
    ordered
        .map(|w| {
            let row = row(repo, w, agents, lang, styles);
            if matches!(row.state, AgentState::Active | AgentState::Idle) {
                agents += 1;
            }
            row
        })
        .collect()
}

/// The present sessions of a worktree, the one to show first: Active, then Inactive, then the
/// latest state change, then the id (a fixed order, Arquitecto 2026-10-06).
fn present<'r>(repo: &'r RepoView, worktree: &WorktreeRow) -> Vec<&'r SessionRow> {
    let mut sessions: Vec<&SessionRow> = repo
        .sessions
        .iter()
        .filter(|s| s.worktree == worktree.key && s.state != SessionStateView::Ended)
        .collect();
    sessions.sort_by(|a, b| {
        let rank = |s: &SessionRow| u8::from(s.state != SessionStateView::Active);
        rank(a)
            .cmp(&rank(b))
            .then(b.state_since_ms.cmp(&a.state_since_ms))
            .then(a.session_id.cmp(&b.session_id))
    });
    sessions
}

fn agent_name(session: &SessionRow, lang: Lang) -> String {
    match (&session.name, session.kind) {
        (Some(name), _) => name.as_str().to_owned(),
        (None, AgentKind::ClaudeCode) => Text::ClaudeCode.render(lang),
        (None, AgentKind::Other) => Text::OtherAgent.render(lang),
    }
}

fn row(
    repo: &RepoView,
    worktree: &WorktreeRow,
    color_index: usize,
    lang: Lang,
    styles: &Styles,
) -> AgentRowModel {
    let sessions = present(repo, worktree);
    let (name, state) = match (repo.detection, sessions.first()) {
        (Some(true), Some(first)) => {
            let name = agent_name(first, lang);
            let name = match sessions.len() - 1 {
                0 => name,
                more => Text::AgentAndMore { name: &name, more }.render(lang),
            };
            let state = match first.state {
                SessionStateView::Active => AgentState::Active,
                _ => AgentState::Idle,
            };
            (SafeText::name(&name), state)
        }
        (Some(true), None) => (catalog(Text::Unattributed, lang), AgentState::NoAgent),
        // Detection unknown or not available on this system: never "no agent".
        _ => (catalog(Text::AgentNotAvailable, lang), AgentState::NoAgent),
    };
    let g = styles.glyphs;
    let mut row = AgentRowModel {
        color_index,
        name,
        state,
        state_label: SafeText::default(),
        branch: Branch::Named(SafeText::default()),
        changes: 0,
        sync: Sync::Unknown(SafeText::default()),
        activity: catalog(Text::NotAvailable, lang),
        // Published by the predictor and Guardrails (US-CKP-006, TS-GRD); not before.
        conflict: false,
        blocked: false,
        operation: None,
    };
    match &worktree.state {
        WorktreeState::Unavailable(reason) => {
            row.state = AgentState::Unavailable;
            row.state_label = catalog(Text::WorktreeUnavailable(*reason), lang);
        }
        WorktreeState::Ready {
            head,
            changes,
            divergence,
        } => {
            row.branch = match head {
                Head::Branch(name) | Head::Unborn(name) => Branch::Named(name.clone()),
                Head::Detached => Branch::Detached(catalog(Text::Detached, lang)),
            };
            row.changes = u32::try_from(*changes).unwrap_or(u32::MAX);
            row.sync = match divergence {
                DivergenceView::Counted { ahead, behind } if ahead.exact && behind.exact => {
                    Sync::Known {
                        ahead: u32::try_from(ahead.count).unwrap_or(u32::MAX),
                        behind: u32::try_from(behind.count).unwrap_or(u32::MAX),
                    }
                }
                // The walk stopped at its bound: "at least", never an exact-looking count.
                DivergenceView::Counted { ahead, behind } => {
                    let side = |glyph: &str, c: &gitraptor_api::messages::CommitCountView| {
                        format!("{glyph}{}{}", c.count, if c.exact { "" } else { "+" })
                    };
                    Sync::Unknown(SafeText::text(&format!(
                        "{} {}",
                        side(g.ahead, ahead),
                        side(g.behind, behind)
                    )))
                }
                DivergenceView::BaseMissing => Sync::Unknown(catalog(Text::BaseMissing, lang)),
                DivergenceView::NoBase => Sync::Unknown(catalog(Text::NoBase, lang)),
                DivergenceView::NoCommits => Sync::Unknown(catalog(Text::NoCommits, lang)),
                DivergenceView::Unreadable => Sync::Unknown(catalog(Text::Unreadable, lang)),
            };
        }
    }
    row
}

/// The key hints, from the single keymap table: quit is the hint always shown.
fn key_hints(lang: Lang) -> KeyHintsModel {
    KeyHintsModel {
        hints: BINDINGS
            .iter()
            .filter(|b| b.action != Action::Quit)
            .filter_map(|b| hint(b.action, lang))
            .collect(),
        help: hint(Action::Quit, lang)
            .unwrap_or_else(|| KeyHint::new(SafeText::text("q"), catalog(Text::KeyQuit, lang))),
    }
}

fn hint(action: Action, lang: Lang) -> Option<KeyHint> {
    let binding = BINDINGS.iter().find(|b| b.action == action)?;
    let key = binding.keys.first()?;
    Some(KeyHint::new(
        SafeText::text(&key.label()),
        catalog(binding.hint, lang),
    ))
}

#[cfg(test)]
mod tests;
