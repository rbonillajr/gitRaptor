//! `view(&Model, &mut Frame)` (ADR-CKP-003 § 2 and § 7): pure, it only
//! reads the model. Below 80×24 it paints only the minimum-size message.
//!
//! The regions are placeholders: the widgets come from the widget library
//! (TS-CKP-005, `tui::widgets`), each a pure `ratatui::widgets::Widget`
//! built from already derived data (`SafeText` or catalog text) and the
//! theme's styles. This module only lays out and composes. No literal
//! colors or glyphs here (ADR-GRP-003).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::model::{ConnState, Model, Requester};
use crate::present::i18n::{Lang, Text};
use crate::tui::keymap::BINDINGS;

/// Smallest size with the full layout (Q-CKP-18).
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

const SEPARATOR: &str = " · ";

/// The three regions of the layout: header, body and status bar.
pub struct Regions {
    pub header: Rect,
    pub body: Rect,
    pub status: Rect,
}

/// `None` below the minimum size.
pub fn layout(area: Rect) -> Option<Regions> {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return None;
    }
    let [header, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    Some(Regions {
        header,
        body,
        status,
    })
}

pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    let lang = model.ui.lang;
    let Some(regions) = layout(area) else {
        let text = Text::TooSmall {
            width: area.width,
            height: area.height,
        }
        .render(lang);
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    };
    frame.render_widget(header(model), regions.header);
    frame.render_widget(body(lang), regions.body);
    frame.render_widget(status_bar(model), regions.status);
}

/// Placeholder of the header: product, repo and freshness.
fn header(model: &Model) -> Paragraph<'static> {
    let lang = model.ui.lang;
    let repo = model.engine.repo.as_ref();
    let mut spans = vec![Span::styled(
        Text::Title.render(lang),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    spans.push(Span::raw(SEPARATOR));
    spans.push(Span::raw(match repo.and_then(|r| r.data.as_ref()) {
        Some(data) => Text::Repo(&data.path).render(lang),
        None => Text::NoRepo.render(lang),
    }));
    let stale = model.engine.global.stale || repo.is_some_and(|r| r.stale);
    if stale {
        spans.push(Span::raw(SEPARATOR));
        spans.push(Span::styled(
            Text::Stale.render(lang),
            Style::new().add_modifier(Modifier::REVERSED),
        ));
    }
    Paragraph::new(Line::from(spans))
}

/// Placeholder of the fleet panel, empty until US-CKP-001.
fn body(lang: Lang) -> Paragraph<'static> {
    Paragraph::new(Text::FleetEmpty.render(lang))
        .block(Block::bordered().title(Text::FleetTitle.render(lang)))
        .wrap(Wrap { trim: true })
}

/// Placeholder of the status bar: engine, connection, requester, the
/// answer to the last key and the key hints.
fn status_bar(model: &Model) -> Paragraph<'static> {
    let lang = model.ui.lang;
    let engine = model.engine.global.data.as_ref().map(|g| g.engine);
    let mut parts = vec![
        Text::Engine(engine).render(lang),
        Text::Conn(model.conn).render(lang),
    ];
    if let Some(requester) = &model.engine.requester {
        parts.push(
            match requester {
                Requester::Unattributed { .. } => Text::ActingAsYourself,
                Requester::Agent { name, .. } => Text::ActingAsAgent(name.as_ref()),
                Requester::Unverified => Text::Unverified,
            }
            .render(lang),
        );
    }
    if let Some(notice) = model.ui.notice {
        parts.push(Text::Notice(notice).render(lang));
    }
    let mut spans = Vec::new();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(SEPARATOR));
        }
        spans.push(Span::raw(part));
    }
    for binding in BINDINGS {
        let Some(key) = binding.keys.first() else {
            continue;
        };
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            key.label(),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::raw(binding.hint.render(lang)));
    }
    let style = if model.conn == ConnState::Live {
        Style::new()
    } else {
        Style::new().add_modifier(Modifier::REVERSED)
    };
    Paragraph::new(Line::from(spans)).style(style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GlobalView, Size};
    use gitraptor_api::messages::EngineStateView;
    use gitraptor_api::scope::AutostartView;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn render(model: &Model, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view(model, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn line(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    fn model(lang: Lang) -> Model {
        let mut model = Model::new(
            lang,
            Size {
                width: 80,
                height: 24,
            },
        );
        model.conn = ConnState::Live;
        model.engine.global.data = Some(GlobalView {
            engine: EngineStateView::Observing,
            git_version: None,
            autostart: AutostartView::Unknown,
            repo_count: 0,
        });
        model
    }

    #[test]
    fn the_skeleton_has_header_panel_and_status_bar() {
        let buffer = render(&model(Lang::En), 80, 24);
        assert_eq!(line(&buffer, 0), "GitRaptor · no repo selected");
        assert!(line(&buffer, 1).starts_with("┌Fleet"));
        assert!(line(&buffer, 2).contains("The live fleet will appear here."));
        assert_eq!(
            line(&buffer, 23),
            "engine observing · live  q quit  r retry"
        );
    }

    #[test]
    fn spanish_and_a_lost_connection() {
        let mut m = model(Lang::Es);
        m.conn = ConnState::Reconnecting { attempt: 2 };
        m.engine.global.stale = true;
        let buffer = render(&m, 100, 30);
        assert_eq!(
            line(&buffer, 0),
            "GitRaptor · sin repo seleccionado · desactualizado"
        );
        assert_eq!(
            line(&buffer, 29),
            "motor observando · desconectado, intento 2  q salir  r reintentar"
        );
    }

    #[test]
    fn below_80_by_24_only_the_size_message_is_painted() {
        let buffer = render(&model(Lang::En), 79, 24);
        assert!(line(&buffer, 0).starts_with("Terminal too small (79×24)"));
        assert!(!(0..24).any(|y| line(&buffer, y).contains("GitRaptor")));
    }

    /// A malicious path reaches the buffer visible and without ESC (SEC-12).
    #[test]
    fn a_malicious_repo_path_is_painted_inert() {
        let mut m = model(Lang::En);
        m.engine.repo = Some(crate::model::ScopeReplica {
            data: Some(crate::model::RepoView {
                repo_id: "r1".into(),
                path: crate::present::SafeText::text("/w/\u{1b}]52;c;eA==\u{7}"),
                worktree_count: 0,
            }),
            ..Default::default()
        });
        let buffer = render(&m, 80, 24);
        assert_eq!(line(&buffer, 0), "GitRaptor · repo /w/\\x1b]52;c;eA==\\x07");
        let painted: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(!painted.contains('\u{1b}'));
    }
}
