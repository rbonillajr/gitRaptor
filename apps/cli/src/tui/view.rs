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
use gitraptor_api::guard::HooksStatus;
use gitraptor_api::messages::{DivergenceView, SessionStateView};
use gitraptor_theme::SymbolSet;
use ratatui::Frame;

use crate::model::{
    ConnState, Head, LastCommit, Model, Notice, Pick, RepoView, Requester, SafeText, SessionRow,
    WorktreeRow, WorktreeState,
};
use crate::present::i18n::{Fetched, Lang, Legend, Text};
use crate::tui::engine_state::{self, HeaderNotice};
use crate::tui::fleet_order::{self, Group, Shown};
use crate::tui::help;
use crate::tui::keymap::{Action, BINDINGS, Key};
use crate::tui::style::{Glyphs, Styles, fill, follow, modal_area};
use crate::tui::widgets::Component;
use crate::tui::widgets::agent_list::{
    AgentColumns, AgentListModel, AgentRowModel, AgentState, Branch, CommitLine, SessionLine,
    SessionMark, Sync,
};
use crate::tui::widgets::key_hints::{KeyHint, KeyHintsModel, LegendRow};
use crate::tui::widgets::layout::{self, BodyPlan, Connection, StatusBarModel, TooSmallModel};
use crate::tui::widgets::observe_prompt::ObservePromptModel;
use crate::tui::widgets::repo_picker::{RepoChoiceModel, RepoPickerModel};
use crate::tui::widgets::themed;
use ratatui::layout::Rect;

/// Smallest size with the full layout (Q-CKP-18).
pub const MIN_WIDTH: u16 = layout::MIN_WIDTH;
pub const MIN_HEIGHT: u16 = layout::MIN_HEIGHT;

/// The translation context of the view: language and symbol set. Every text of the catalog
/// reaches the screen through it, folded to ASCII when the set is ASCII; nothing else in
/// this module calls `Text::render`.
#[derive(Clone, Copy)]
pub(crate) struct Say {
    pub lang: Lang,
    pub glyphs: &'static Glyphs,
    pub set: SymbolSet,
}

impl Say {
    pub(crate) fn new(model: &Model, styles: &Styles) -> Self {
        Self {
            lang: model.ui.lang,
            glyphs: styles.glyphs,
            set: model.ui.theme.symbol_set(),
        }
    }

    pub(crate) fn text(self, text: Text<'_>) -> String {
        self.glyphs.fold(&text.render(self.lang)).into_owned()
    }

    pub(crate) fn safe(self, text: Text<'_>) -> SafeText {
        SafeText::text(&self.text(text))
    }
}

pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let styles = Styles::new(model.ui.theme);
    let say = Say::new(model, &styles);
    let Some(regions) = layout::layout(area, BodyPlan::default()) else {
        let small = TooSmallModel {
            message: say.safe(Text::TooSmall {
                width: area.width,
                height: area.height,
                min_width: layout::MIN_WIDTH,
                min_height: layout::MIN_HEIGHT,
            }),
            quit: hint(model, Action::Quit, say)
                .unwrap_or_else(|| KeyHint::new(SafeText::text("q"), say.safe(Text::KeyQuit))),
        };
        frame.render_widget(themed(&small, &styles), area);
        return;
    };
    frame.render_widget(themed(&status_bar(model, &styles), &styles), regions.header);
    let mut hints = key_hints(model, say);
    // Without an engine, or while it waits for Git, nothing else on the body is true.
    if let Some(panel) = engine_state::blocking_panel(model) {
        frame.render_widget(themed(&panel, &styles), regions.list);
        paint_help(model, say, &styles, frame, regions.header, regions.hints);
        frame.render_widget(themed(&hints, &styles), regions.hints);
        return;
    }
    if let Some(prompt) = observe_prompt(model, say) {
        frame.render_widget(themed(&prompt, &styles), regions.list);
        paint_help(model, say, &styles, frame, regions.header, regions.hints);
        frame.render_widget(themed(&hints, &styles), regions.hints);
        return;
    }
    match picker(model, say, regions.list.height) {
        Some(picker) => frame.render_widget(themed(&picker, &styles), regions.list),
        None => {
            let fleet = fleet(model, &styles);
            // With no repos the panel takes the fleet's place, and there are no rows to explain.
            let no_repos = engine_state::fleet_panel(model);
            if no_repos.is_none() {
                hints.legend = state_legend(&fleet, say);
            }
            let paint_body = |frame: &mut Frame, area: Rect| match &no_repos {
                Some(panel) => frame.render_widget(themed(panel, &styles), area),
                None => frame.render_widget(themed(&fleet, &styles), area),
            };
            // A discovered repo is asked under the fleet, which stays where it is.
            match discovered_prompt(model, say) {
                Some(prompt) => {
                    let panel = DISCOVERED_PANEL_HEIGHT.min(regions.list.height);
                    let (above, below) = split_bottom(regions.list, panel);
                    paint_body(frame, above);
                    frame.render_widget(themed(&prompt, &styles), below);
                }
                None => paint_body(frame, regions.list),
            }
        }
    }
    paint_help(model, say, &styles, frame, regions.header, regions.hints);
    frame.render_widget(themed(&hints, &styles), regions.hints);
}

/// The `?` help over the body (between the header and the key bar), which both stay visible.
/// It clears its rectangle first, so what is under it does not show through.
fn paint_help(
    model: &Model,
    say: Say,
    styles: &Styles,
    frame: &mut Frame,
    header: Rect,
    bar: Rect,
) {
    if !model.ui.help {
        return;
    }
    let top = header.bottom();
    let body = Rect::new(header.x, top, header.width, bar.y.saturating_sub(top));
    let help = help::help(model, say);
    let rows = help.height(body.width);
    let area = modal_area(body, help::HELP_WIDTH, rows);
    fill(frame.buffer_mut(), area, styles);
    frame.render_widget(themed(&help, styles), area);
}

/// What the symbols of the rows on screen mean, one entry per state and in a fixed order:
/// no agent (the words of the dogfooding amendment), a session ended when no row has no
/// agent, active, idle and warning. Empty when no row carries any of them.
fn state_legend(fleet: &AgentListModel, say: Say) -> Vec<LegendRow> {
    let has = |state: AgentState| fleet.rows.iter().any(|r| r.state == state);
    let mut legend = Vec::new();
    if has(AgentState::NoAgent) {
        let mut row = help::legend_row(Legend::Done, say);
        row.label = say.safe(Text::NoAgentLegend);
        legend.push(row);
    }
    // Rows of the same hollow symbol: the shorter legend is enough when no row has no agent.
    if has(AgentState::Done) && !has(AgentState::NoAgent) {
        legend.push(help::legend_row(Legend::Done, say));
    }
    if has(AgentState::Active) {
        legend.push(help::legend_row(Legend::Active, say));
    }
    if has(AgentState::Idle) {
        legend.push(help::legend_row(Legend::Idle, say));
    }
    let warns = fleet
        .rows
        .iter()
        .any(|r| r.commit.as_ref().is_some_and(|c| c.warning));
    if has(AgentState::Unavailable) || warns {
        legend.push(help::legend_row(Legend::Warning, say));
    }
    legend
}

/// The selected repo when its snapshot arrived.
fn repo(model: &Model) -> Option<&RepoView> {
    model.engine.repo.as_ref().and_then(|r| r.data.as_ref())
}

/// The header: repo, connection (with the engine state, staleness and the answer to the last
/// key), requester and the ⚡/⛔ counts the engine publishes.
pub fn status_bar(model: &Model, styles: &Styles) -> StatusBarModel {
    let say = Say::new(model, styles);
    let repo = repo(model);
    let mut connection = match model.conn {
        ConnState::Live => Connection::Live,
        ConnState::Connecting
        | ConnState::Starting
        | ConnState::Syncing
        | ConnState::Resyncing
        | ConnState::Reconnecting { .. } => Connection::Reconnecting,
        ConnState::EngineUnavailable
        | ConnState::Incompatible
        | ConnState::Rejected
        | ConnState::Unsupported => Connection::Unavailable,
    };
    let engine = model.engine.global.data.as_ref().map(|g| g.engine);
    let mut label = match engine_state::header_notice(model) {
        // A tier notice stands where "live · engine …" would be.
        Some(notice @ (HeaderNotice::Reconciling | HeaderNotice::Dormant)) => {
            connection = Connection::Reconnecting;
            vec![say.text(match notice {
                HeaderNotice::Dormant => Text::Dormant,
                _ => Text::Reconciling,
            })]
        }
        Some(HeaderNotice::Resync(reason)) => {
            connection = Connection::Reconnecting;
            let mut label = vec![say.text(Text::Conn(model.conn))];
            label.extend(reason.map(|r| say.text(Text::Resync(r))));
            label
        }
        None => {
            let mut label = vec![say.text(Text::Conn(model.conn))];
            if model.conn == ConnState::Live {
                label.push(say.text(Text::Engine(engine)));
            }
            label
        }
    };
    let stale = model.engine.global.stale || model.engine.repo.as_ref().is_some_and(|r| r.stale);
    if stale {
        label.push(say.text(Text::Stale));
    }
    match (model.ui.notice, &model.ui.here) {
        (Some(Notice::ObserveFailed(reason)), Some(here)) => {
            label.push(say.text(Text::ObserveFailed {
                name: &here.name,
                path: &here.path,
                reason,
            }))
        }
        (Some(notice), _) => label.push(say.text(Text::Notice(notice))),
        (None, _) => {}
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
            None => say.safe(Text::NoRepo),
        },
        connection,
        connection_label: SafeText::text(&label.join(styles.glyphs.separator)),
        // Only what the developer must know: the hook layer stopped being active or lost its
        // record (US-GRD-004). A healthy or absent protection takes no room in the bar.
        protection: repo
            .and_then(|r| r.protection.as_ref())
            .and_then(|hooks| match hooks.status {
                HooksStatus::Inactive => Some(Text::ProtectionLost(hooks.cause)),
                HooksStatus::Orphaned => Some(Text::ProtectionOrphaned),
                HooksStatus::Active | HooksStatus::NotInstalled => None,
            })
            .map(|text| say.safe(text)),
        requester: say.safe(requester),
        conflicts: attention.and_then(|a| a.conflicts),
        blocked: attention.and_then(|a| a.denials),
    }
}

/// The fleet of the selected repo (US-CKP-001): one row per worktree, the main one first and
/// the rest in the order the engine published them (the order by attention is US-CKP-002's).
pub fn fleet(model: &Model, styles: &Styles) -> AgentListModel {
    let say = Say::new(model, styles);
    let repo = repo(model);
    let rows = repo.map(|r| rows(r, model, styles)).unwrap_or_default();
    let empty = match (&model.engine.repo, repo) {
        (Some(_), Some(_)) => Text::FleetEmpty,
        (Some(_), None) => Text::FleetWaiting,
        (None, _) if model.conn == ConnState::Live && model.ui.pick == Pick::None => {
            Text::FleetNoRepo
        }
        (None, _) => Text::FleetWaiting,
    };
    let fetched = match repo.map(|r| r.fetched_ms) {
        Some(_) if !model.engine.activity => Fetched::NotAvailable,
        Some(Some(at)) => Fetched::Ago(model.now_ms.saturating_sub(at)),
        Some(None) => Fetched::Never,
        None => Fetched::NotAvailable,
    };
    AgentListModel {
        title: match repo.and_then(engine_state::base_not_found) {
            // The base the engine published does not exist: no count is made against another.
            Some(base) => say.safe(Text::FleetTitleBaseNotFound { base }),
            None => say.safe(Text::FleetTitle {
                base: repo.and_then(|r| r.base.as_ref()),
                fetched,
            }),
        },
        columns: AgentColumns {
            agent: say.safe(Text::ColAgent),
            branch: say.safe(Text::ColBranch),
            changes: say.safe(Text::ColFiles),
            sync: say.safe(Text::ColSync),
            activity: say.safe(Text::ColActivity),
        },
        rows,
        selected: None,
        offset: 0,
        focused: true,
        empty: say.safe(empty),
    }
}

/// "Observe this repo? [y/N]" while it is asked, and "Observing notes…" once answered yes
/// (US-CKP-025).
pub(crate) fn observe_prompt(model: &Model, say: Say) -> Option<ObservePromptModel> {
    let here = model.ui.here.as_ref()?;
    let question = match model.ui.pick {
        Pick::Asking => Text::ObserveQuestion,
        Pick::Observing => Text::Observing(&here.name),
        _ => return None,
    };
    Some(ObservePromptModel {
        title: say.safe(Text::ObserveTitle),
        why: say.safe(Text::ObserveWhy),
        name: here.name.clone(),
        path: here.path.clone(),
        question: say.safe(question),
    })
}

/// Rows of the discovered repo's panel: border, where, name, question and border, with the
/// blank lines of the panel.
const DISCOVERED_PANEL_HEIGHT: u16 = 7;

/// `area` without its last `bottom` rows, and those rows.
fn split_bottom(area: Rect, bottom: u16) -> (Rect, Rect) {
    let top = area.height.saturating_sub(bottom);
    (
        Rect::new(area.x, area.y, area.width, top),
        Rect::new(area.x, area.y + top, area.width, area.height - top),
    )
}

/// "Discovered repo: Observe notes?" for the developer, when nothing else is being asked
/// (US-GRP-020).
pub(crate) fn discovered_prompt(model: &Model, say: Say) -> Option<ObservePromptModel> {
    let found = model.discovery_prompt()?;
    Some(ObservePromptModel {
        title: say.safe(Text::DiscoveredTitle),
        why: say.safe(Text::DiscoveredWhy(&found.root)),
        name: found.name.clone(),
        path: found.shown.clone(),
        question: say.safe(Text::DiscoveredQuestion(&found.name)),
    })
}

/// The observed repos to choose from, while the developer chooses one.
pub(crate) fn picker(model: &Model, say: Say, height: u16) -> Option<RepoPickerModel> {
    let Pick::Choosing { selected } = model.ui.pick else {
        return None;
    };
    let repos = model.engine.global.data.as_ref()?.repos.as_slice();
    let selected = selected.min(repos.len().saturating_sub(1));
    // Borders and the prompt line.
    let visible = usize::from(height.saturating_sub(4)).max(1);
    Some(RepoPickerModel {
        title: say.safe(Text::PickTitle),
        prompt: say.safe(Text::PickPrompt),
        repos: repos
            .iter()
            .map(|r| RepoChoiceModel {
                name: r.name.clone(),
                path: r.path.clone(),
            })
            .collect(),
        selected,
        offset: follow(0, selected, visible),
    })
}

fn rows(repo: &RepoView, model: &Model, styles: &Styles) -> Vec<AgentRowModel> {
    let say = Say::new(model, styles);
    let mut rows: Vec<(Group, &WorktreeRow, AgentRowModel)> = repo
        .worktrees
        .iter()
        .map(|w| {
            let shown =
                fleet_order::shown(&repo.sessions, w.key, model.now_ms, model.ui.show_ended);
            let mut row = row(repo, w, &shown, model, say, styles);
            // With `scope.activity` the engine seeds what the store knows (ADR-GRP-013 § 6), so
            // an absent value is "nothing yet", painted as the set's unknown glyph.
            if model.engine.activity {
                row.activity = match w.last_activity_ms {
                    Some(at) => say.safe(Text::Ago(model.now_ms.saturating_sub(at))),
                    None => SafeText::text(styles.glyphs.unknown),
                };
            }
            // A system that does not detect sessions has no lead: "not available" goes with
            // the rows without an agent.
            let lead = shown
                .sessions
                .first()
                .filter(|_| repo.detection == Some(true))
                .map(|s| s.state);
            let group = fleet_order::group(w.main, row.conflict || row.blocked, lead);
            (group, w, row)
        })
        .collect();
    rows.sort_by(|a, b| fleet_order::order_key(a.0, a.1).cmp(&fleet_order::order_key(b.0, b.1)));
    rows.into_iter().map(|(_, _, row)| row).collect()
}

fn agent_name(session: &SessionRow, say: Say) -> String {
    match (&session.name, session.kind) {
        (Some(name), _) => name.as_str().to_owned(),
        (None, kind) => kind_name(kind, say),
    }
}

fn kind_name(kind: AgentKind, say: Say) -> String {
    match kind {
        AgentKind::ClaudeCode => say.text(Text::ClaudeCode),
        AgentKind::Other => say.text(Text::OtherAgent),
    }
}

/// "commit by Ana with Claude Code" under the row (US-CKP-026): who it went in under and with
/// which agents; the warning leads when an agent ran it without its trailer.
fn commit_line(commit: &LastCommit, say: Say) -> CommitLine {
    let agents: Vec<String> = commit.agents.iter().map(|k| kind_name(*k, say)).collect();
    let ran_by = commit.ran_by.map(|k| kind_name(k, say));
    let inferred = commit.inferred.map(|k| kind_name(k, say));
    CommitLine {
        text: say.safe(Text::LastCommit {
            merge: commit.merge,
            author: commit.author.as_str(),
            agents: &agents.join(", "),
            ran_by: ran_by.as_deref(),
            inferred: inferred.as_deref(),
        }),
        warning: commit.ran_by.is_some() && commit.agents.is_empty(),
    }
}

/// "ended 2 h ago", from what the engine published or detected.
fn ended_text(session: &SessionRow, model: &Model, say: Say) -> Option<String> {
    let end = fleet_order::ended_at(session)?;
    Some(say.text(Text::Ended(
        end.kind,
        model.now_ms.saturating_sub(end.at_ms),
    )))
}

/// The line under a row with several sessions, or with only an ended one: who else is there.
fn sessions_line(shown: &Shown<'_>, model: &Model, say: Say) -> Option<SessionLine> {
    let sessions = &shown.sessions;
    if sessions.len() > 1 {
        let marks = sessions[1..]
            .iter()
            .map(|s| {
                let agent = agent_name(s, say);
                match ended_text(s, model, say) {
                    Some(end) => SessionMark {
                        state: AgentState::Done,
                        color_index: None,
                        text: say.safe(Text::AgentEnded {
                            agent: &agent,
                            end: &end,
                        }),
                    },
                    None => SessionMark {
                        state: match s.state {
                            SessionStateView::Active => AgentState::Active,
                            _ => AgentState::Idle,
                        },
                        color_index: Some(model.ui.colors.index(&s.session_id).unwrap_or(0)),
                        text: SafeText::name(&agent),
                    },
                }
            })
            .collect();
        let lead = if fleet_order::is_shared(shown) {
            say.safe(Text::Shared)
        } else {
            SafeText::default()
        };
        return Some(SessionLine { lead, marks });
    }
    let only = sessions.first()?;
    let end = ended_text(only, model, say)?;
    Some(SessionLine {
        lead: SafeText::text(&end),
        marks: Vec::new(),
    })
}

fn row(
    repo: &RepoView,
    worktree: &WorktreeRow,
    shown: &Shown<'_>,
    model: &Model,
    say: Say,
    styles: &Styles,
) -> AgentRowModel {
    let sessions = &shown.sessions;
    let mut color_index = 0;
    let mut line = None;
    let (name, state) = match (repo.detection, sessions.first()) {
        (Some(true), Some(first)) => {
            let name = say.text(Text::AgentInWorktree {
                agent: &agent_name(first, say),
                worktree: worktree.name.as_str(),
            });
            let name = match sessions.len() - 1 {
                0 => name,
                more => say.text(Text::AgentAndMore { name: &name, more }),
            };
            let state = match first.state {
                SessionStateView::Active => AgentState::Active,
                SessionStateView::Inactive => AgentState::Idle,
                SessionStateView::Ended => AgentState::Done,
            };
            color_index = model.ui.colors.index(&first.session_id).unwrap_or(0);
            line = sessions_line(shown, model, say);
            (SafeText::name(&name), state)
        }
        (Some(true), None) => {
            line = shown.last_agent.and_then(|last| {
                let end = ended_text(last, model, say)?;
                Some(SessionLine {
                    lead: say.safe(Text::LastAgent {
                        agent: &agent_name(last, say),
                        end: &end,
                    }),
                    marks: Vec::new(),
                })
            });
            (
                say.safe(Text::NoAgent {
                    worktree: worktree.name.as_str(),
                }),
                AgentState::NoAgent,
            )
        }
        // Detection unknown or not available on this system: never "no agent".
        _ => (say.safe(Text::AgentNotAvailable), AgentState::Unknown),
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
        activity: say.safe(Text::NotAvailable),
        tag: worktree.temporary.then(|| say.safe(Text::Temporary)),
        // Published by the predictor and Guardrails (US-CKP-006, TS-GRD); not before.
        conflict: false,
        blocked: false,
        operation: None,
        commit: repo.last_commit(worktree.key).map(|c| commit_line(c, say)),
        sessions: line,
    };
    match &worktree.state {
        WorktreeState::Unavailable(reason) => {
            row.state = AgentState::Unavailable;
            row.state_label = say.safe(Text::WorktreeUnavailable(*reason));
        }
        WorktreeState::Ready {
            head,
            changes,
            divergence,
        } => {
            row.branch = match head {
                Head::Branch(name) | Head::Unborn(name) => Branch::Named(name.clone()),
                Head::Detached(commit) => Branch::Detached(
                    say.safe(Text::NoBranch(commit.as_ref().map(SafeText::as_str))),
                ),
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
                DivergenceView::BaseMissing => Sync::Unknown(say.safe(Text::BaseMissing)),
                DivergenceView::NoBase => Sync::Unknown(say.safe(Text::NoBase)),
                DivergenceView::NoCommits => Sync::Unknown(say.safe(Text::NoCommits)),
                DivergenceView::Unreadable => Sync::Unknown(say.safe(Text::Unreadable)),
            };
        }
    }
    row
}

/// The key bar: what the panel on screen accepts (the same list the help shows), quit first,
/// and `?` in the fixed place at the right.
fn key_hints(model: &Model, say: Say) -> KeyHintsModel {
    KeyHintsModel {
        hints: help::available(model)
            .filter(|b| b.action.is_hinted())
            .filter(|b| b.action != Action::ShowEnded || show_ended_hint(model))
            .filter_map(|b| hint(model, b.action, say))
            .collect(),
        help: hint(model, Action::Help, say)
            .unwrap_or_else(|| KeyHint::new(SafeText::text("?"), say.safe(Text::KeyHelp))),
        legend: Vec::new(),
    }
}

/// The `f` hint shows in the bar only when it would change the screen: an ended session is
/// hidden somewhere, or the filter is on.
fn show_ended_hint(model: &Model) -> bool {
    model.ui.show_ended
        || repo(model).is_some_and(|r| fleet_order::hidden_count(r, model.now_ms) > 0)
}

fn hint(model: &Model, action: Action, say: Say) -> Option<KeyHint> {
    let binding = BINDINGS.iter().find(|b| b.action == action)?;
    // "[s/N]" in Spanish: the yes key shown is the one of the language.
    let key = match (action, say.lang) {
        (Action::Yes, Lang::Es) => binding.keys.iter().find(|k| **k == Key::Char('s'))?,
        // The arrows have a word in ASCII, but the next key is the one to show there.
        _ if say.set == SymbolSet::Ascii => binding
            .keys
            .iter()
            .find(|k| !matches!(k, Key::Up | Key::Down))
            .or(binding.keys.first())?,
        _ => binding.keys.first()?,
    };
    Some(KeyHint::new(
        SafeText::text(&key.label(say.set)),
        say.safe(help::hint_text(model, binding)),
    ))
}

#[cfg(test)]
mod a11y_tests;

// The screens the help tests of `tui::help` build on.
#[cfg(test)]
pub(crate) use tests::{asking, discovering, outside, shop_model};

#[cfg(test)]
mod tests {
    //! The screen as US-CKP-001 shows it: the fleet composed from the widget library, fed only by
    //! what the engine published, plus snapshots of the whole screen on a dark and a light
    //! terminal.

    use super::*;
    use crate::model::{EngineMsg, GlobalView, Msg, Size, Stamped};
    use crate::tui::gallery::buffer_text;
    use crate::tui::update::update;
    use crate::tui::widgets::tests::style_runs;
    use gitraptor_api::messages::{
        BaseBranchView, BaseStatusView, ChangeCounts, CommitCountView, EngineStateView, HeadView,
        RepoStateView, RepoView as ApiRepo, SessionView, SessionsListResult, WorktreeStatus,
        WorktreeView,
    };
    use gitraptor_api::scope::{AutostartView, RepoSnapshot, ScopeSnapshot};
    use gitraptor_api::{Actor, AgentKind, AgentOrigin, Untrusted, UntrustedName};
    use gitraptor_theme::{Background, ColorMode, Contrast, SymbolSet, Theme};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use unicode_width::UnicodeWidthStr;

    pub(crate) fn render(model: &Model, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view(model, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    pub(crate) fn lines(buffer: &Buffer) -> Vec<String> {
        buffer_text(buffer)
    }

    pub(crate) fn screen(model: &Model, width: u16, height: u16) -> String {
        lines(&render(model, width, height)).join("\n")
    }

    pub(crate) fn engine(msg: EngineMsg) -> Msg {
        Msg::Engine(Stamped {
            recv_ns: 0,
            decoded_ns: 0,
            msg,
        })
    }

    pub(crate) fn ready(branch: &str, changes: u32, ahead: u64, behind: u64) -> WorktreeStatus {
        WorktreeStatus::Ready {
            head: HeadView::Branch {
                name: UntrustedName::new(branch),
            },
            counts: ChangeCounts {
                staged: 0,
                unstaged: changes,
                untracked: 0,
            },
            changes: Vec::new(),
            divergence: DivergenceView::Counted {
                ahead: CommitCountView {
                    count: ahead,
                    exact: true,
                },
                behind: CommitCountView {
                    count: behind,
                    exact: true,
                },
            },
        }
    }

    pub(crate) fn worktree(path: &str, main: bool, status: WorktreeStatus) -> WorktreeView {
        WorktreeView {
            last_activity_utc_ms: None,
            last_activity_in_gap: false,
            detached_at: None,
            operation: None,
            unmerged_paths: 0,
            path: Untrusted::new(path),
            main,
            admin_name: None,
            status,
        }
    }

    /// The repo "shop" of the story: main, feat-pagos with claude-1 Active, 2 files, ↑3 ↓1, and
    /// feat-docs without an agent.
    pub(crate) fn shop() -> Vec<WorktreeView> {
        vec![
            worktree("/w/shop/feat-pagos", false, ready("feat-pagos", 2, 3, 1)),
            worktree("/w/shop", true, ready("main", 0, 0, 0)),
            worktree("/w/shop/feat-docs", false, ready("feat-docs", 0, 1, 0)),
        ]
    }

    pub(crate) fn session(
        id: &str,
        worktree: &str,
        name: &str,
        state: SessionStateView,
    ) -> SessionView {
        SessionView {
            repo_id: "r1".into(),
            session_id: id.into(),
            worktree: Untrusted::new(worktree),
            actor: Actor::Agent {
                kind: AgentKind::Other,
                name: Some(UntrustedName::new(name)),
                origin: AgentOrigin::Registered,
            },
            state,
            started_utc_ms: 0,
            state_since_utc_ms: 1,
            utc_offset_s: 0,
            ended_utc_ms: None,
            end_cause: None,
        }
    }

    pub(crate) fn model_with(
        lang: Lang,
        worktrees: Vec<WorktreeView>,
        sessions: Option<Vec<SessionView>>,
    ) -> Model {
        let mut model = Model::new(
            lang,
            Size {
                width: 100,
                height: 30,
            },
        );
        model.conn = ConnState::Live;
        model.engine.requester = Some(Requester::Unattributed {
            layer: gitraptor_api::catalog::Layer::Cockpit,
        });
        model.engine.global.data = Some(GlobalView {
            repos: Vec::new(),
            engine: EngineStateView::Observing,
            git_version: None,
            autostart: AutostartView::Unknown,
            repo_count: 1,
            attention: Vec::new(),
        });
        update(
            &mut model,
            engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
                RepoSnapshot {
                    run_id: "run".into(),
                    scope_seq: 1,
                    repo: ApiRepo {
                        fetched_utc_ms: None,
                        repo_id: "r1".into(),
                        state: RepoStateView::Observed,
                        path: Untrusted::new("/w/shop/.git"),
                        base: BaseBranchView {
                            name: Some(UntrustedName::new("main")),
                            status: BaseStatusView::Confirmed,
                        },
                        worktrees,
                        tier: None,
                        checked_utc_ms: None,
                        kept_temps: None,
                    },
                },
            )))),
        );
        if let Some(sessions) = sessions {
            update(
                &mut model,
                engine(EngineMsg::Sessions {
                    repo_id: "r1".into(),
                    result: Box::new(SessionsListResult {
                        detection_available: true,
                        sessions,
                    }),
                }),
            );
        }
        model
    }

    pub(crate) fn shop_model(lang: Lang) -> Model {
        model_with(
            lang,
            shop(),
            Some(vec![session(
                "s1",
                "/w/shop/feat-pagos",
                "claude-1",
                SessionStateView::Active,
            )]),
        )
    }

    fn row<'a>(screen: &'a [String], needle: &str) -> &'a str {
        screen
            .iter()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no row with {needle}: {screen:#?}"))
    }

    /// US-GRD-004: the status bar tells that the protection stopped being active, in both
    /// languages, from the answer to `guard.status` and from the events; it says nothing while
    /// the protection is fine, and clears when it is back.
    #[test]
    fn the_status_bar_tells_that_the_protection_was_lost() {
        use gitraptor_api::guard::{HooksLayer, HooksStatus, LossCause};
        for (lang, lost) in [
            (Lang::En, "protection lost: a hook is gone"),
            (Lang::Es, "protección perdida: falta un hook"),
        ] {
            let mut model = shop_model(lang);
            assert!(!screen(&model, 120, 30).contains("protect"));
            update(
                &mut model,
                engine(EngineMsg::Protection {
                    repo_id: "r1".into(),
                    hooks: Some(HooksLayer::lost(LossCause::DispatcherMissing)),
                }),
            );
            let shown = screen(&model, 120, 30);
            assert!(shown.contains(lost), "{shown}");
            update(
                &mut model,
                engine(EngineMsg::Protection {
                    repo_id: "r1".into(),
                    hooks: Some(HooksLayer::of(HooksStatus::Active)),
                }),
            );
            assert!(!screen(&model, 120, 30).contains("protect"));
        }
    }

    /// A `full` repo whose hooks were replaced still tells that the protection was lost: the bar
    /// reads the hooks half (`GuardStatus.hooks`), never the four-valued state.
    #[test]
    fn full_with_inactive_hooks_keeps_the_protection_lost_text() {
        use gitraptor_api::guard::{GuardStatus, HooksLayer, LossCause, ProtectionState};
        let wire = serde_json::json!({
            "repo_id": "r1",
            "state": "full",
            "permission": "granted",
            "offer": false,
            "protected_bases": [],
            "base_confirmed": true,
            "not_preventable": [],
            "last_refusal": [],
            "hooks": HooksLayer::lost(LossCause::DispatcherMissing),
        });
        let status: GuardStatus = serde_json::from_value(wire).expect("a full status");
        assert_eq!(status.state, ProtectionState::Full);
        let mut model = shop_model(Lang::En);
        update(
            &mut model,
            engine(EngineMsg::Protection {
                repo_id: "r1".into(),
                hooks: status.hooks,
            }),
        );
        let shown = screen(&model, 120, 30);
        assert!(shown.contains("protection lost: a hook is gone"), "{shown}");
    }

    /// Scenario 1: one row per worktree, the main one first and the agent first in the row.
    #[test]
    fn one_row_per_worktree_with_the_agent_first() {
        let screen = lines(&render(&shop_model(Lang::En), 100, 30));
        let rows: Vec<&String> = screen
            .iter()
            .filter(|l| l.contains("feat-") || l.contains(" main "))
            .collect();
        assert!(rows[0].contains("main"), "main first: {rows:#?}");
        let pagos = row(&screen, "feat-pagos");
        let agent = pagos.find("claude-1").unwrap();
        assert!(agent < pagos.find("feat-pagos").unwrap(), "{pagos}");
        // Active is the ● of DSYS § 2.2, before the name.
        assert!(pagos.contains("●  claude-1"), "{pagos}");
        assert!(pagos.contains("~2"), "{pagos}");
        assert!(pagos.contains("↑3 ↓1"), "{pagos}");
        assert!(
            screen[1].contains("↑↓ vs main (local copy, fetch age not available)"),
            "{}",
            screen[1]
        );
    }

    /// Scenario 3: a worktree without an agent session says so with its folder and the hollow
    /// ○, never "human"; the key hints say what the ○ means (dogfooding amendment 2026-10-06).
    #[test]
    fn a_worktree_without_an_agent_says_no_agent_never_human() {
        for (lang, text, legend) in [
            (
                Lang::En,
                "○  No agent · feat-docs",
                "○ no agent: changes by you or another tool",
            ),
            (
                Lang::Es,
                "○  Sin agente · feat-docs",
                "○ sin agente: cambios tuyos o de otra herramienta",
            ),
        ] {
            let all = screen(&shop_model(lang), 100, 30);
            let screen = lines(&render(&shop_model(lang), 100, 30));
            assert!(row(&screen, "feat-docs").contains(text), "{all}");
            assert!(screen[29].contains(legend), "{all}");
            assert!(!all.to_lowercase().contains("human"), "{all}");
            assert!(!all.to_lowercase().contains("humano"), "{all}");
        }
    }

    /// The legend only explains a ○ that is on screen.
    #[test]
    fn without_a_row_without_an_agent_there_is_no_legend() {
        let model = model_with(
            Lang::En,
            vec![worktree("/w/shop", true, ready("main", 0, 0, 0))],
            Some(vec![claude("s1", "/w/shop")]),
        );
        let all = screen(&model, 100, 30);
        assert!(!all.contains("no agent"), "{all}");
    }

    /// A worktree in the system's temporary folder at a commit without a branch: the short
    /// hash "(no branch)" and the "temporary" label, in English and in Spanish.
    fn scratch(lang: Lang, published: bool) -> Model {
        let root = std::env::temp_dir().join("scratch");
        let mut status = ready("", 1, 0, 0);
        if let WorktreeStatus::Ready { head, .. } = &mut status {
            *head = HeadView::Detached;
        }
        let mut scratch = worktree(root.to_str().unwrap(), false, status);
        if published {
            scratch.detached_at = Some("39e852f0a1b2c3d4e5f60718293a4b5c6d7e8f90".into());
        }
        let mut worktrees = shop();
        worktrees.push(scratch);
        model_with(
            lang,
            worktrees,
            Some(vec![claude("s1", "/w/shop/feat-pagos")]),
        )
    }

    #[test]
    fn a_scratch_worktree_without_a_branch_says_so() {
        for (lang, branch, label, unknown) in [
            (
                Lang::En,
                "39e852f (no branch)",
                "· temporary",
                "(no branch)",
            ),
            (Lang::Es, "39e852f (sin rama)", "· temporal", "(sin rama)"),
        ] {
            let screen = lines(&render(&scratch(lang, true), 100, 30));
            let line = row(&screen, "scratch");
            assert!(line.contains(branch), "{line}");
            assert!(line.contains(label), "{line}");
            assert!(!row(&screen, "feat-docs").contains(label), "{screen:#?}");
            // An engine that does not publish the commit: no hash, never a made-up one.
            let screen = lines(&render(&scratch(lang, false), 100, 30));
            let line = row(&screen, "scratch");
            assert!(line.contains(unknown), "{line}");
            assert!(!line.contains("39e852f"), "{line}");
        }
    }

    /// The fleet with rows without an agent, a scratch worktree without a branch and the legend,
    /// in English and in Spanish (snapshots of the dogfooding amendment of US-CKP-001).
    #[test]
    fn the_fleet_without_agents_in_english_and_spanish() {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (name, lang) in [("en", Lang::En), ("es", Lang::Es)] {
                let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode)
                    .with_background(Background::Dark);
                let model = published(scratch(lang, true)).with_theme(theme);
                let buffer = render(&model, 100, 24);
                let snap = format!(
                    "{}\n=== styles ===\n{}",
                    lines(&buffer).join("\n"),
                    style_runs(&buffer)
                );
                insta::assert_snapshot!(format!("fleet_no_agent_100x24_{name}"), snap);
            }
        });
    }

    /// Names that do not fit are cut by display width and end in the ellipsis of the symbol
    /// set: wide characters are never split and no line goes past the screen, in English and
    /// in Spanish (DS-INF-CKP-001 § 9, Entrega 2a).
    #[test]
    fn long_names_end_in_an_ellipsis() {
        let long = "feat/界面-a-branch-name-far-too-long-for-any-column-of-the-fleet";
        let fleet = || {
            let mut worktrees = shop();
            worktrees.push(worktree(
                "/w/shop/un-worktree-con-un-nombre-larguísimo-que-no-cabe",
                false,
                ready(long, 1, 0, 0),
            ));
            worktrees
        };
        let agent = "agente-con-un-nombre-muy-largo-que-tampoco-cabe";
        let sessions = || {
            Some(vec![session(
                "s9",
                "/w/shop/un-worktree-con-un-nombre-larguísimo-que-no-cabe",
                agent,
                SessionStateView::Active,
            )])
        };
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (name, lang) in [("en", Lang::En), ("es", Lang::Es)] {
                for (set, ellipsis) in [(SymbolSet::Unicode, "…"), (SymbolSet::Ascii, "...")] {
                    let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, set)
                        .with_background(Background::Dark);
                    let model = model_with(lang, fleet(), sessions()).with_theme(theme);
                    let screen = lines(&render(&model, 80, 24));
                    let line = row(&screen, "feat/");
                    assert!(line.contains(ellipsis), "{line}");
                    assert!(!line.contains(long), "{line}");
                    for l in &screen {
                        assert!(UnicodeWidthStr::width(l.as_str()) <= 80, "{l}");
                    }
                    if set == SymbolSet::Unicode {
                        insta::assert_snapshot!(
                            format!("fleet_ellipsis_80x24_{name}"),
                            screen.join("\n")
                        );
                    }
                }
            }
        });
    }

    /// Without the session list the TUI cannot tell: "agent not available", not "no agent".
    #[test]
    fn unknown_detection_is_not_presented_as_no_agent() {
        let model = model_with(Lang::En, shop(), None);
        let screen = lines(&render(&model, 100, 30));
        assert!(row(&screen, "feat-pagos").contains("agent not available"));
        assert!(!screen.join("\n").contains("No agent"));
    }

    /// Scenario 4: the last activity is not published, so every row says so.
    #[test]
    fn unpublished_activity_is_not_available() {
        for (lang, text) in [(Lang::En, "not available"), (Lang::Es, "no disponible")] {
            let screen = lines(&render(&shop_model(lang), 100, 30));
            for name in ["feat-pagos", "feat-docs"] {
                assert!(
                    row(&screen, name)
                        .trim_end_matches('┃')
                        .trim_end()
                        .ends_with(text),
                    "{name}: {screen:#?}"
                );
            }
        }
    }

    /// Scenario 5: a branch with an escape sequence is painted inert and visible.
    #[test]
    fn a_malicious_branch_is_painted_inert() {
        let model = model_with(
            Lang::En,
            vec![worktree("/w/x", true, ready("evil\u{1b}[2J", 0, 0, 0))],
            Some(Vec::new()),
        );
        let buffer = render(&model, 100, 30);
        let screen = lines(&buffer);
        assert!(row(&screen, "evil").contains("evil\\x1b[2J"), "{screen:#?}");
        let painted: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(!painted.contains('\u{1b}'));
    }

    /// The ⚡ and ⛔ counts the engine does not publish are "–", never 0 (BR-CKP-CALC-001).
    #[test]
    fn unpublished_attention_counts_are_not_zero() {
        let model = shop_model(Lang::En);
        let header = &lines(&render(&model, 100, 30))[0];
        assert!(header.contains("shop"), "{header}");
        assert!(header.contains("live · engine observing"), "{header}");
        assert!(header.contains("acting as you"), "{header}");
        assert!(header.ends_with("⚡ –  ⛔ –"), "{header}");
    }

    #[test]
    fn a_worktree_the_engine_cannot_read_says_why() {
        let model = model_with(
            Lang::En,
            vec![worktree(
                "/w/gone",
                false,
                WorktreeStatus::Unavailable {
                    reason: gitraptor_api::messages::UnavailableReason::Missing,
                },
            )],
            Some(Vec::new()),
        );
        let screen = lines(&render(&model, 100, 30));
        assert!(
            row(&screen, "unavailable: folder missing").contains('⚠'),
            "{screen:#?}"
        );
    }

    #[test]
    fn a_bounded_count_is_at_least_and_an_uncounted_one_says_why() {
        let mut pagos = worktree("/w/a", true, ready("a", 0, 0, 0));
        if let WorktreeStatus::Ready { divergence, .. } = &mut pagos.status {
            *divergence = DivergenceView::Counted {
                ahead: CommitCountView {
                    count: 10_000,
                    exact: false,
                },
                behind: CommitCountView {
                    count: 2,
                    exact: true,
                },
            };
        }
        let mut lost = worktree("/w/b", false, ready("b", 0, 0, 0));
        if let WorktreeStatus::Ready { divergence, .. } = &mut lost.status {
            *divergence = DivergenceView::BaseMissing;
        }
        let screen = lines(&render(
            &model_with(Lang::En, vec![pagos, lost], Some(Vec::new())),
            100,
            30,
        ));
        assert!(row(&screen, " a ").contains("↑10000+ ↓2"), "{screen:#?}");
        assert!(
            row(&screen, " b ").contains("not computable"),
            "{screen:#?}"
        );
    }

    #[test]
    fn two_present_sessions_show_the_active_one_and_how_many_more() {
        let mut idle = session(
            "s0",
            "/w/shop/feat-pagos",
            "claude-0",
            SessionStateView::Inactive,
        );
        idle.state_since_utc_ms = 9;
        let model = model_with(
            Lang::En,
            shop(),
            Some(vec![
                idle,
                session(
                    "s1",
                    "/w/shop/feat-pagos",
                    "claude-1",
                    SessionStateView::Active,
                ),
            ]),
        );
        let screen = lines(&render(&model, 100, 30));
        assert!(
            row(&screen, "feat-pagos").contains("●  claude-1 · feat-pagos +1"),
            "{screen:#?}"
        );
    }

    #[test]
    fn below_80_by_24_only_the_size_message_is_painted() {
        let screen = screen(&shop_model(Lang::En), 79, 24);
        assert!(screen.contains("Terminal too small (79×24)"), "{screen}");
        assert!(!screen.contains("feat-pagos"));
        assert!(screen.contains("q quit"), "{screen}");
    }

    #[test]
    fn without_a_repo_the_fleet_says_how_to_add_one() {
        let mut model = shop_model(Lang::Es);
        model.engine.repo = None;
        let screen = screen(&model, 100, 30);
        assert!(screen.contains("sin repo seleccionado"), "{screen}");
        assert!(screen.contains("raptor repo add"), "{screen}");
    }

    const NOW: i64 = 1_800_000_000_000;

    /// The shop with what `scope.activity` publishes: feat-pagos changed 2 minutes ago, the
    /// others not since the engine started, and the repo fetched 3 hours ago.
    fn published(mut model: Model) -> Model {
        model.engine.activity = true;
        model.now_ms = NOW;
        let data = model.engine.repo.as_mut().unwrap().data.as_mut().unwrap();
        data.fetched_ms = Some(NOW - 3 * 3_600_000);
        for w in &mut data.worktrees {
            if w.name.as_str() == "feat-pagos" {
                w.last_activity_ms = Some(NOW - 120_000);
            }
        }
        model
    }

    fn claude(id: &str, worktree: &str) -> SessionView {
        let mut s = session(id, worktree, "", SessionStateView::Active);
        s.actor = Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        };
        s
    }

    /// Dogfooding 2026-10-06: three "Claude Code" rows could not be told apart; each one now
    /// names the folder of its worktree.
    #[test]
    fn agents_of_the_same_kind_are_told_apart_by_their_worktree() {
        let model = model_with(
            Lang::En,
            shop(),
            Some(vec![
                claude("s1", "/w/shop/feat-pagos"),
                claude("s2", "/w/shop/feat-docs"),
                claude("s3", "/w/shop"),
            ]),
        );
        let screen = lines(&render(&model, 100, 30));
        assert!(row(&screen, "feat-pagos").contains("Claude Code · feat-pagos"));
        assert!(row(&screen, "feat-docs").contains("Claude Code · feat-docs"));
        assert!(
            row(&screen, "Claude Code · shop").contains(" main "),
            "{screen:#?}"
        );
    }

    /// Dogfooding 2026-10-06: with `scope.activity` the column shows the age of the last
    /// activity (also when seeded from the store after a restart) and the title the age of the
    /// last fetch; a worktree without any activity yet shows the unknown glyph, in both
    /// languages, never "not available".
    #[test]
    fn published_activity_and_fetch_show_their_age() {
        for (lang, ago, unseen, fetched) in [
            (Lang::En, "2 min ago", "–", "(local copy, fetched 3 h ago)"),
            (Lang::Es, "hace 2 min", "–", "(copia local, fetch hace 3 h)"),
        ] {
            let screen = lines(&render(&published(shop_model(lang)), 100, 30));
            let end = |name: &str| {
                row(&screen, name)
                    .trim_end_matches('┃')
                    .trim_end()
                    .to_owned()
            };
            assert!(end("feat-pagos").ends_with(ago), "{screen:#?}");
            assert!(end("feat-docs").ends_with(unseen), "{screen:#?}");
            assert!(screen[1].contains(fetched), "{}", screen[1]);
        }
    }

    #[test]
    fn a_repo_never_fetched_says_so() {
        for (lang, text) in [(Lang::En, "never fetched"), (Lang::Es, "sin fetch")] {
            let mut model = published(shop_model(lang));
            model
                .engine
                .repo
                .as_mut()
                .unwrap()
                .data
                .as_mut()
                .unwrap()
                .fetched_ms = None;
            let screen = lines(&render(&model, 100, 30));
            assert!(screen[1].contains(text), "{}", screen[1]);
        }
    }

    #[test]
    fn ages_use_the_largest_whole_unit() {
        for (ms, en, es) in [
            (400, "just now", "ahora"),
            (59_999, "59 s ago", "hace 59 s"),
            (3_599_999, "59 min ago", "hace 59 min"),
            (7_200_000, "2 h ago", "hace 2 h"),
            (3 * 86_400_000, "3 d ago", "hace 3 d"),
        ] {
            assert_eq!(Text::Ago(ms).render(Lang::En), en);
            assert_eq!(Text::Ago(ms).render(Lang::Es), es);
        }
    }

    pub(crate) fn outside(lang: Lang, repos: &[&str]) -> Model {
        let mut model = shop_model(lang);
        model.engine.repo = None;
        model.engine.global.data.as_mut().unwrap().repos = repos
            .iter()
            .map(|name| crate::model::RepoChoice {
                repo_id: format!("id-{name}"),
                name: SafeText::name(name),
                path: SafeText::text(&format!("/w/{name}/.git")),
                tier: None,
            })
            .collect();
        model
    }

    pub(crate) fn key(code: ratatui::crossterm::event::KeyCode) -> Msg {
        Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            code,
            ratatui::crossterm::event::KeyModifiers::NONE,
        ))
    }

    /// Dogfooding 2026-10-06: outside every observed repo, the only one opens by itself.
    #[test]
    fn outside_a_repo_the_only_observed_one_opens() {
        let mut model = outside(Lang::En, &["shop"]);
        let cmds = update(&mut model, Msg::Conn(crate::model::ConnEvent::Unlocated));
        assert_eq!(
            cmds,
            vec![crate::model::Cmd::Open {
                repo_id: "id-shop".into()
            }]
        );
        let screen = screen(&model, 100, 30);
        assert!(!screen.contains("raptor repo add"), "{screen}");
        assert!(screen.contains("Waiting for the engine"), "{screen}");
    }

    /// Only without any observed repo does the fleet suggest `raptor repo add`.
    #[test]
    fn outside_a_repo_with_none_observed_it_says_how_to_add_one() {
        let mut model = outside(Lang::En, &[]);
        assert!(update(&mut model, Msg::Conn(crate::model::ConnEvent::Unlocated)).is_empty());
        assert!(screen(&model, 100, 30).contains("raptor repo add"));
    }

    /// With several, the developer chooses with ↑↓ and Enter; the choice opens that repo.
    #[test]
    fn outside_a_repo_the_developer_chooses_among_several() {
        use ratatui::crossterm::event::KeyCode;
        let mut model = outside(Lang::En, &["shop", "blog", "api"]);
        assert!(update(&mut model, Msg::Conn(crate::model::ConnEvent::Unlocated)).is_empty());
        let first = screen(&model, 100, 30);
        assert!(
            first.contains("not in an observed repo: choose one"),
            "{first}"
        );
        assert!(!first.contains("raptor repo add"), "{first}");
        assert!(first.contains("↑ up"), "{first}");
        assert!(first.contains("Enter open"), "{first}");
        for code in [KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Up] {
            assert!(update(&mut model, key(code)).is_empty());
        }
        let lines = lines(&render(&model, 100, 30));
        assert!(row(&lines, "blog").contains('›'), "{lines:#?}");
        assert!(!row(&lines, "api").contains('›'), "{lines:#?}");
        assert_eq!(
            update(&mut model, key(KeyCode::Enter)),
            vec![crate::model::Cmd::Open {
                repo_id: "id-blog".into()
            }]
        );
        // The list keys are not hinted (nor act) outside the picker.
        let after = screen(&model, 100, 30);
        assert!(!after.contains("Enter open"), "{after}");
        assert!(update(&mut model, key(KeyCode::Enter)).is_empty());
    }

    #[test]
    fn the_repo_picker_in_spanish_and_on_screen() {
        let mut model = outside(Lang::Es, &["shop", "blog"]);
        update(&mut model, Msg::Conn(crate::model::ConnEvent::Unlocated));
        let painted = screen(&model, 80, 24);
        assert!(painted.contains("Repos observados"), "{painted}");
        assert!(painted.contains("elige uno"), "{painted}");
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            let english = outside(Lang::En, &["shop", "blog"]);
            let mut english = english;
            update(&mut english, Msg::Conn(crate::model::ConnEvent::Unlocated));
            let buffer = render(&english, 80, 24);
            let snap = format!(
                "{}\n=== styles ===\n{}",
                lines(&buffer).join("\n"),
                style_runs(&buffer)
            );
            insta::assert_snapshot!("repo_picker_80x24", snap);
        });
    }

    /// The whole screen at 80×24 and 120×40 on a dark and a light terminal: the text is the same,
    /// only the colors change (DSYS-GRP-001, TS-CKP-004 amendment).
    #[test]
    fn the_fleet_on_a_dark_and_a_light_terminal() {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (w, h) in [(80, 24), (120, 40)] {
                let mut text = None;
                for (name, background) in [("dark", Background::Dark), ("light", Background::Light)]
                {
                    let theme =
                        Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode)
                            .with_background(background);
                    let model = published(shop_model(Lang::En)).with_theme(theme);
                    let buffer = render(&model, w, h);
                    let painted = lines(&buffer).join("\n");
                    if let Some(text) = &text {
                        assert_eq!(text, &painted, "the background changed the text");
                    } else {
                        text = Some(painted.clone());
                    }
                    let snap = format!(
                        "{painted}\n=== styles ({name}) ===\n{}",
                        style_runs(&buffer)
                    );
                    insta::assert_snapshot!(format!("fleet_{w}x{h}_{name}"), snap);
                }
            }
        });
    }

    /// A commit in `worktree` by "Ana", run by `actor`, with the trailers of `agents` and an
    /// optional unconfirmed hint (US-CKP-026, from US-GRD-019's `events.authorship`).
    fn commit_by_ana(
        seq: i64,
        worktree: &str,
        actor: Actor,
        agents: &[AgentKind],
        inferred: Option<AgentKind>,
    ) -> gitraptor_api::messages::GitEventView {
        use gitraptor_api::messages::{
            CoAuthor, DeclaredAuthorship, GitEventDetails, GitEventKind, GitEventView, GitIdentity,
            InferredAgent, TrailerCheck,
        };
        let ana = || GitIdentity {
            name: Untrusted::new("Ana"),
            email: Untrusted::new("ana@example.com"),
        };
        GitEventView {
            repo_id: "r1".into(),
            seq,
            worktree: Untrusted::new(worktree),
            kind: GitEventKind::Commit,
            actor,
            observed_utc_ms: 0,
            utc_offset_s: 0,
            details: GitEventDetails::default(),
            gap_id: None,
            inferred: inferred.map(|kind| InferredAgent {
                kind,
                session_id: "1:1".into(),
                trailer: Some(TrailerCheck::Unconfirmed),
            }),
            authorship: Some(DeclaredAuthorship {
                author: ana(),
                committer: ana(),
                coauthors: agents
                    .iter()
                    .map(|k| CoAuthor {
                        name: Untrusted::new("Claude"),
                        email: Untrusted::new("noreply@anthropic.com"),
                        agent: Some(*k),
                    })
                    .collect(),
            }),
        }
    }

    fn detected_claude() -> Actor {
        Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        }
    }

    /// The fleet of "shop" with the three cases of US-CKP-026: a person's commit with an
    /// agent's trailer (feat-pagos), an agent that left no trailer (feat-api) and a person's
    /// commit without an agent (feat-docs).
    fn authored(lang: Lang) -> Model {
        let mut worktrees = shop();
        worktrees.push(worktree(
            "/w/shop/feat-api",
            false,
            ready("feat-api", 0, 1, 0),
        ));
        let mut model = model_with(
            lang,
            worktrees,
            Some(vec![claude("s1", "/w/shop/feat-pagos")]),
        );
        update(
            &mut model,
            engine(EngineMsg::History {
                repo_id: "r1".into(),
                events: vec![
                    commit_by_ana(
                        1,
                        "/w/shop/feat-pagos",
                        Actor::Unattributed,
                        &[AgentKind::ClaudeCode],
                        None,
                    ),
                    commit_by_ana(2, "/w/shop/feat-api", detected_claude(), &[], None),
                    commit_by_ana(3, "/w/shop/feat-docs", Actor::Unattributed, &[], None),
                ],
            }),
        );
        published(model)
    }

    /// US-CKP-026: the three cases under their rows, in English and in Spanish, with names only
    /// (never the email) and the author sanitized.
    #[test]
    fn the_last_commit_authorship_in_english_and_spanish() {
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (name, lang, human, agent, alone) in [
                (
                    "en",
                    Lang::En,
                    "└ • commit by Ana with Claude Code",
                    "└ • ⚠  commit by Ana · run by Claude Code · no trailer",
                    "└ • commit by Ana",
                ),
                (
                    "es",
                    Lang::Es,
                    "└ • commit de Ana con Claude Code",
                    "└ • ⚠  commit de Ana · ejecutado por Claude Code · sin trailer",
                    "└ • commit de Ana",
                ),
            ] {
                let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode)
                    .with_background(Background::Dark);
                let buffer = render(&authored(lang).with_theme(theme), 100, 24);
                let screen = lines(&buffer);
                let under = |name: &str| {
                    let i = screen.iter().position(|l| l.contains(name)).unwrap();
                    screen[i + 1].clone()
                };
                assert!(under("feat-pagos").contains(human), "{screen:#?}");
                assert!(under("feat-api").contains(agent), "{screen:#?}");
                assert!(under("feat-docs").contains(alone), "{screen:#?}");
                assert!(!under("feat-docs").contains(" · "), "{screen:#?}");
                // The main worktree has no known commit: no line under it.
                assert!(!under("main").contains("commit"), "{screen:#?}");
                let all = screen.join("\n");
                assert!(
                    !all.contains("example.com") && !all.contains('\u{1b}'),
                    "{all}"
                );
                let snap = format!("{all}\n=== styles ===\n{}", style_runs(&buffer));
                insta::assert_snapshot!(format!("fleet_authorship_100x24_{name}"), snap);
            }
        });
    }

    /// The commit line reads without color: in ASCII the structural glyphs and the warning
    /// symbol have their fallbacks.
    #[test]
    fn the_last_commit_authorship_in_ascii() {
        let theme = Theme::new(ColorMode::NoColor, Contrast::Normal, SymbolSet::Ascii);
        let screen = lines(&render(&authored(Lang::En).with_theme(theme), 100, 24));
        let all = screen.join("\n");
        assert!(all.contains("` o commit by Ana with Claude Code"), "{all}");
        assert!(
            all.contains("` o [!] commit by Ana | run by Claude Code | no trailer"),
            "{all}"
        );
    }

    /// An unattributed commit with an unconfirmed hint says "possibly", never as a fact; a
    /// commit without published authorship has no line, never a placeholder.
    #[test]
    fn authorship_edge_cases() {
        for (lang, text) in [
            (Lang::En, "commit by Ana · possibly Claude Code (inferred)"),
            (Lang::Es, "commit de Ana · posible Claude Code (inferido)"),
        ] {
            let mut model = shop_model(lang);
            let mut bare = commit_by_ana(9, "/w/shop/feat-pagos", Actor::Unattributed, &[], None);
            bare.authorship = None;
            update(
                &mut model,
                engine(EngineMsg::History {
                    repo_id: "r1".into(),
                    events: vec![
                        commit_by_ana(
                            1,
                            "/w/shop/feat-docs",
                            Actor::Unattributed,
                            &[],
                            Some(AgentKind::ClaudeCode),
                        ),
                        bare,
                    ],
                }),
            );
            let screen = lines(&render(&model, 100, 30));
            let i = screen.iter().position(|l| l.contains("feat-docs")).unwrap();
            assert!(screen[i + 1].contains(text), "{screen:#?}");
            let i = screen
                .iter()
                .position(|l| l.contains("feat-pagos"))
                .unwrap();
            assert!(!screen[i + 1].contains("commit"), "{screen:#?}");
        }
    }

    /// The rows win (PO, 2026-10-07): when the rows and their commit lines do not fit, no
    /// commit line is painted and every row that fits is there.
    #[test]
    fn rows_win_over_sublines_when_short() {
        let mut worktrees = shop();
        let mut events = Vec::new();
        for i in 0..12 {
            let path = format!("/w/shop/wt-{i:02}");
            worktrees.push(worktree(
                &path,
                false,
                ready(&format!("wt-{i:02}"), 0, 0, 0),
            ));
            events.push(commit_by_ana(i, &path, Actor::Unattributed, &[], None));
        }
        let mut model = model_with(Lang::En, worktrees, Some(Vec::new()));
        update(
            &mut model,
            engine(EngineMsg::History {
                repo_id: "r1".into(),
                events,
            }),
        );
        let all = screen(&model, 80, 24);
        assert!(!all.contains("commit by"), "{all}");
        for i in 0..12 {
            assert!(all.contains(&format!("wt-{i:02}")), "{all}");
        }
        // With room for both, the lines are back.
        assert!(screen(&model, 80, 40).contains("commit by Ana"));
    }

    /// "notes" not observed, asked as the developer (US-CKP-025), with "shop" observed.
    pub(crate) fn asking(lang: Lang) -> Model {
        let mut model = outside(lang, &["shop"]);
        model.engine.requester = Some(Requester::Unattributed {
            layer: gitraptor_api::catalog::Layer::Cockpit,
        });
        update(
            &mut model,
            Msg::Conn(crate::model::ConnEvent::Unobserved(
                crate::model::Candidate {
                    root: "/w/notes".into(),
                    name: SafeText::name("notes"),
                    path: SafeText::text("/w/notes"),
                },
            )),
        );
        model
    }

    /// The question with its default, the repo by name and path, and its keys, in both
    /// languages; no list keys and no "add it with raptor repo add" while it is asked.
    #[test]
    fn observe_prompt_in_english_and_spanish() {
        for (lang, question, keys) in [
            (Lang::En, "Observe this repo? [y/N]", ["y observe", "n no"]),
            (
                Lang::Es,
                "¿Observar este repo? [s/N]",
                ["s observar", "n no"],
            ),
        ] {
            let model = asking(lang);
            let painted = screen(&model, 80, 24);
            assert!(painted.contains(question), "{painted}");
            assert!(painted.contains("notes  /w/notes"), "{painted}");
            for key in keys {
                assert!(painted.contains(key), "{painted}");
            }
            assert!(!painted.contains("Enter"), "{painted}");
            assert!(!painted.contains("raptor repo add"), "{painted}");
        }
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (lang, name) in [
                (Lang::En, "observe_prompt_80x24_en"),
                (Lang::Es, "observe_prompt_80x24_es"),
            ] {
                let buffer = render(&asking(lang), 80, 24);
                let snap = format!(
                    "{}\n=== styles ===\n{}",
                    lines(&buffer).join("\n"),
                    style_runs(&buffer)
                );
                insta::assert_snapshot!(name, snap);
            }
        });
    }

    /// Once answered yes, the panel says it is observing until the repo's snapshot arrives.
    #[test]
    fn observe_prompt_says_it_is_observing() {
        use ratatui::crossterm::event::KeyCode;
        let mut model = asking(Lang::Es);
        update(&mut model, key(KeyCode::Char('s')));
        let painted = screen(&model, 80, 24);
        assert!(painted.contains("Observando notes…"), "{painted}");
        assert!(!painted.contains("[s/N]"), "{painted}");
    }

    /// A repo found in a code folder, asked as the developer, with the fleet of "shop" open.
    pub(crate) fn discovering(lang: Lang) -> Model {
        let mut model = shop_model(lang);
        model.engine.requester = Some(Requester::Unattributed {
            layer: gitraptor_api::catalog::Layer::Cockpit,
        });
        update(
            &mut model,
            Msg::Conn(crate::model::ConnEvent::Discovered(vec![
                crate::model::Found {
                    path: "/code/notes".into(),
                    name: SafeText::name("notes"),
                    shown: SafeText::text("/code/notes"),
                    root: SafeText::text("/code"),
                },
            ])),
        );
        model
    }

    /// US-GRP-020: the discovered repo is asked under the fleet, which stays on screen, with
    /// its title, where it was found and the question, in both languages.
    #[test]
    fn discovered_prompt_in_english_and_spanish() {
        for (lang, title, why, question, keys) in [
            (
                Lang::En,
                "Discovered repo",
                "In your code folder /code",
                "Observe notes?",
                ["y observe", "n no", "Esc later"],
            ),
            (
                Lang::Es,
                "Repo descubierto",
                "En tu carpeta de código /code",
                "¿Observar notes?",
                ["s observar", "n no", "Esc luego"],
            ),
        ] {
            let painted = screen(&discovering(lang), 80, 24);
            for text in [title, why, question, "notes  /code/notes"] {
                assert!(painted.contains(text), "{text}\n{painted}");
            }
            for key in keys {
                assert!(painted.contains(key), "{key}\n{painted}");
            }
            // The fleet is still there.
            assert!(painted.contains("shop"), "{painted}");
        }
        let mut settings = insta::Settings::clone_current();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_snapshot_path("snapshots");
        settings.bind(|| {
            for (lang, name) in [
                (Lang::En, "discovered_prompt_80x24_en"),
                (Lang::Es, "discovered_prompt_80x24_es"),
            ] {
                let buffer = render(&discovering(lang), 80, 24);
                let snap = format!(
                    "{}\n=== styles ===\n{}",
                    lines(&buffer).join("\n"),
                    style_runs(&buffer)
                );
                insta::assert_snapshot!(name, snap);
            }
        });
    }

    /// Not human, or a screen without room: no panel (it stays pending).
    #[test]
    fn discovered_prompt_only_with_room_and_for_the_developer() {
        let mut agent = discovering(Lang::En);
        agent.engine.requester = Some(Requester::Agent {
            name: None,
            layer: gitraptor_api::catalog::Layer::Mcp,
        });
        assert!(!screen(&agent, 80, 24).contains("Discovered repo"));
        let painted = screen(&discovering(Lang::En), 80, 16);
        assert!(!painted.contains("Discovered repo"), "{painted}");
    }

    /// A failed `repo.add`: what happened, why and how to retry, and the observed repos.
    #[test]
    fn observe_failure_says_what_why_and_how_to_retry() {
        use crate::model::ObserveFailure;
        use ratatui::crossterm::event::KeyCode;
        for (lang, failure, text) in [
            (
                Lang::En,
                ObserveFailure::NotTrusted,
                "notes not observed: Git does not trust it",
            ),
            (
                Lang::Es,
                ObserveFailure::Refused,
                "notes sin observar: solo tú puedes observarlo",
            ),
        ] {
            let mut model = asking(lang);
            update(&mut model, key(KeyCode::Char('y')));
            update(
                &mut model,
                Msg::Conn(crate::model::ConnEvent::ObserveFailed(failure)),
            );
            let notice = status_bar(&model, &Styles::new(model.ui.theme));
            let label = notice.connection_label.as_str();
            assert!(label.contains(text), "{label}");
            assert!(label.contains("→ raptor repo add /w/notes"), "{label}");
        }
        // A path with a space is quoted, so the command can be copied as it is.
        assert_eq!(
            Text::ObserveFailed {
                name: &SafeText::name("my notes"),
                path: &SafeText::text("/w/my notes"),
                reason: ObserveFailure::Unknown,
            }
            .render(Lang::En),
            "my notes not observed: the engine refused it → raptor repo add '/w/my notes'"
        );
    }
}
