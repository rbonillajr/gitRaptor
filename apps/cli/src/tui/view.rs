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
    ConnState, Head, LastCommit, Model, Pick, RepoView, Requester, SafeText, SessionRow,
    WorktreeRow, WorktreeState,
};
use crate::present::i18n::{Fetched, Lang, Text};
use crate::tui::keymap::{Action, BINDINGS};
use crate::tui::style::{Styles, follow};
use crate::tui::widgets::agent_list::{
    AgentColumns, AgentListModel, AgentRowModel, AgentState, Branch, CommitLine, Sync,
};
use crate::tui::widgets::key_hints::{KeyHint, KeyHintsModel};
use crate::tui::widgets::layout::{self, BodyPlan, Connection, StatusBarModel, TooSmallModel};
use crate::tui::widgets::repo_picker::{RepoChoiceModel, RepoPickerModel};
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
    let mut hints = key_hints(model);
    match picker(model, regions.list.height) {
        Some(picker) => frame.render_widget(themed(&picker, &styles), regions.list),
        None => {
            let fleet = fleet(model, &styles);
            if fleet.rows.iter().any(|r| r.state == AgentState::NoAgent) {
                hints.legend = Some(catalog(Text::NoAgentLegend, lang));
            }
            frame.render_widget(themed(&fleet, &styles), regions.list);
        }
    }
    frame.render_widget(themed(&hints, &styles), regions.hints);
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
        title: catalog(
            Text::FleetTitle {
                base: repo.and_then(|r| r.base.as_ref()),
                fetched,
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

/// The observed repos to choose from, while the developer chooses one.
pub fn picker(model: &Model, height: u16) -> Option<RepoPickerModel> {
    let Pick::Choosing { selected } = model.ui.pick else {
        return None;
    };
    let lang = model.ui.lang;
    let repos = model.engine.global.data.as_ref()?.repos.as_slice();
    let selected = selected.min(repos.len().saturating_sub(1));
    // Borders and the prompt line.
    let visible = usize::from(height.saturating_sub(4)).max(1);
    Some(RepoPickerModel {
        title: catalog(Text::PickTitle, lang),
        prompt: catalog(Text::PickPrompt, lang),
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
    let lang = model.ui.lang;
    let ordered = repo
        .worktrees
        .iter()
        .filter(|w| w.main)
        .chain(repo.worktrees.iter().filter(|w| !w.main));
    let mut agents = 0;
    ordered
        .map(|w| {
            let mut row = row(repo, w, agents, lang, styles);
            // With `scope.activity` the engine seeds what the store knows (ADR-GRP-013 § 6), so
            // an absent value is "nothing yet", painted as the set's unknown glyph.
            if model.engine.activity {
                row.activity = match w.last_activity_ms {
                    Some(at) => catalog(Text::Ago(model.now_ms.saturating_sub(at)), lang),
                    None => SafeText::text(styles.glyphs.unknown),
                };
            }
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

fn kind_name(kind: AgentKind, lang: Lang) -> String {
    match kind {
        AgentKind::ClaudeCode => Text::ClaudeCode.render(lang),
        AgentKind::Other => Text::OtherAgent.render(lang),
    }
}

/// "commit by Ana with Claude Code" under the row (US-CKP-026): who it went in under and with
/// which agents; the warning leads when an agent ran it without its trailer.
fn commit_line(commit: &LastCommit, lang: Lang) -> CommitLine {
    let agents: Vec<String> = commit.agents.iter().map(|k| kind_name(*k, lang)).collect();
    let ran_by = commit.ran_by.map(|k| kind_name(k, lang));
    let inferred = commit.inferred.map(|k| kind_name(k, lang));
    CommitLine {
        text: catalog(
            Text::LastCommit {
                merge: commit.merge,
                author: commit.author.as_str(),
                agents: &agents.join(", "),
                ran_by: ran_by.as_deref(),
                inferred: inferred.as_deref(),
            },
            lang,
        ),
        warning: commit.ran_by.is_some() && commit.agents.is_empty(),
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
            let name = Text::AgentInWorktree {
                agent: &agent_name(first, lang),
                worktree: worktree.name.as_str(),
            }
            .render(lang);
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
        (Some(true), None) => (
            catalog(
                Text::NoAgent {
                    worktree: worktree.name.as_str(),
                },
                lang,
            ),
            AgentState::NoAgent,
        ),
        // Detection unknown or not available on this system: never "no agent".
        _ => (catalog(Text::AgentNotAvailable, lang), AgentState::Unknown),
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
        tag: worktree.temporary.then(|| catalog(Text::Temporary, lang)),
        // Published by the predictor and Guardrails (US-CKP-006, TS-GRD); not before.
        conflict: false,
        blocked: false,
        operation: None,
        commit: repo.last_commit(worktree.key).map(|c| commit_line(c, lang)),
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
                Head::Detached(commit) => Branch::Detached(catalog(
                    Text::NoBranch(commit.as_ref().map(SafeText::as_str)),
                    lang,
                )),
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

/// The key hints, from the single keymap table: quit is the hint always shown, and the list
/// keys only while there is a list to move in.
fn key_hints(model: &Model) -> KeyHintsModel {
    let lang = model.ui.lang;
    let list = matches!(model.ui.pick, Pick::Choosing { .. });
    KeyHintsModel {
        hints: BINDINGS
            .iter()
            .filter(|b| b.action.is_hinted())
            .filter(|b| list || !b.action.is_list())
            .filter_map(|b| hint(b.action, lang))
            .collect(),
        help: hint(Action::Quit, lang)
            .unwrap_or_else(|| KeyHint::new(SafeText::text("q"), catalog(Text::KeyQuit, lang))),
        legend: None,
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

    fn render(model: &Model, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view(model, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn lines(buffer: &Buffer) -> Vec<String> {
        buffer_text(buffer)
    }

    fn screen(model: &Model, width: u16, height: u16) -> String {
        lines(&render(model, width, height)).join("\n")
    }

    fn engine(msg: EngineMsg) -> Msg {
        Msg::Engine(Stamped {
            recv_ns: 0,
            decoded_ns: 0,
            msg,
        })
    }

    fn ready(branch: &str, changes: u32, ahead: u64, behind: u64) -> WorktreeStatus {
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

    fn worktree(path: &str, main: bool, status: WorktreeStatus) -> WorktreeView {
        WorktreeView {
            last_activity_utc_ms: None,
            last_activity_in_gap: false,
            detached_at: None,
            path: Untrusted::new(path),
            main,
            admin_name: None,
            status,
        }
    }

    /// The repo "shop" of the story: main, feat-pagos with claude-1 Active, 2 files, ↑3 ↓1, and
    /// feat-docs without an agent.
    fn shop() -> Vec<WorktreeView> {
        vec![
            worktree("/w/shop/feat-pagos", false, ready("feat-pagos", 2, 3, 1)),
            worktree("/w/shop", true, ready("main", 0, 0, 0)),
            worktree("/w/shop/feat-docs", false, ready("feat-docs", 0, 1, 0)),
        ]
    }

    fn session(id: &str, worktree: &str, name: &str, state: SessionStateView) -> SessionView {
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

    fn model_with(
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

    fn shop_model(lang: Lang) -> Model {
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
        assert!(row(&screen, "folder missing").contains('⚠'), "{screen:#?}");
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
        assert!(row(&screen, " b ").contains("base absent"), "{screen:#?}");
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

    fn outside(lang: Lang, repos: &[&str]) -> Model {
        let mut model = shop_model(lang);
        model.engine.repo = None;
        model.engine.global.data.as_mut().unwrap().repos = repos
            .iter()
            .map(|name| crate::model::RepoChoice {
                repo_id: format!("id-{name}"),
                name: SafeText::name(name),
                path: SafeText::text(&format!("/w/{name}/.git")),
            })
            .collect();
        model
    }

    fn key(code: ratatui::crossterm::event::KeyCode) -> Msg {
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
            all.contains("` o [!] commit by Ana · run by Claude Code · no trailer"),
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
}
