//! The full-screen graph view (DS-US-CKP-022): a scrollable list of lane headers and commits
//! from rows that already hold sanitized text. Each lane wears the color of its agent; colors
//! are tokens only and the nodes come from the glyph set.

use gitraptor_theme::ColorToken;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Say;
use crate::model::{GraphFailure, Model};
use crate::present::diff::clock;
use crate::present::graph::{GraphRow, ShownBase, rows};
use crate::present::graph_text::GraphText;
use crate::tui::graph::{Anchor, GraphState};
use crate::tui::style::{Pen, Styles, panel};

/// Paints the view over `area`.
pub(super) fn paint(
    state: &GraphState,
    model: &Model,
    say: Say,
    styles: &Styles,
    frame: &mut Frame,
    area: Rect,
) {
    let repo = model
        .engine
        .repo
        .as_ref()
        .and_then(|r| r.data.as_ref())
        .map(|r| r.name.as_str().to_owned())
        .unwrap_or_default();
    let mut title = match state.shown.as_ref().map(|s| (&s.base, s.at_utc_ms)) {
        Some((ShownBase::Ready(base), at)) => say.graph(GraphText::Title {
            repo: &repo,
            base: base.as_str(),
            clock: &clock(at),
        }),
        _ => say.graph(GraphText::TitlePending { repo: &repo }),
    };
    if state.in_flight.is_some() || state.stale {
        let updating = say.graph(GraphText::Updating);
        title = crate::model::SafeText::text(&format!("{title} · {updating}"));
        title = crate::model::SafeText::text(&say.glyphs.fold(title.as_str()));
    }
    let muted = styles.fg(ColorToken::TextMuted);
    let warn = styles.fg(ColorToken::StatusWarning);
    let buf = frame.buffer_mut();
    let inner = panel(buf, area, &title, None, true, muted, styles);
    if inner.height == 0 {
        return;
    }
    let bottom = inner.bottom();
    let failure = state.failure.map(|f| match f {
        GraphFailure::Busy => GraphText::Busy,
        GraphFailure::Lost => GraphText::Lost,
        GraphFailure::Refused | GraphFailure::Cancelled => GraphText::Refused,
    });
    let mut top = inner.y;
    if let Some(failure) = failure {
        Pen::new(buf, inner, top, styles)
            .gap(1)
            .safe(&say.graph(failure), warn);
        top += 1;
    }
    let Some(shown) = &state.shown else {
        if failure.is_none() {
            Pen::new(buf, inner, top, styles)
                .gap(1)
                .safe(&say.graph(GraphText::Loading), muted);
        }
        return;
    };
    let all = rows(shown, model, say);
    let start = match &state.anchor {
        Anchor::Top => 0,
        anchor => all
            .iter()
            .position(|r| r.anchor() == Some(anchor))
            .unwrap_or(0),
    };
    let g = styles.glyphs;
    let room = usize::from(bottom.saturating_sub(top));
    for (y, row) in (top..).zip(all.iter().skip(start).take(room)) {
        let mut pen = Pen::new(buf, inner, y, styles);
        match row {
            GraphRow::Note { text, warning } => {
                pen.gap(1).safe(text, if *warning { warn } else { muted });
            }
            GraphRow::Header {
                color,
                title,
                since,
                reason,
                ..
            } => {
                let lane = color.map_or(muted, |c| styles.agent(c));
                pen.gap(1)
                    .text(g.lane_fork, lane)
                    .text(g.lane_edge, lane)
                    .gap(1)
                    .safe(title, lane.add_modifier(Modifier::BOLD));
                if let Some(since) = since {
                    pen.gap(2).safe(since, muted);
                }
                if let Some(reason) = reason {
                    pen.gap(2).safe(reason, warn);
                }
            }
            GraphRow::Commit {
                color,
                short,
                summary,
                merge,
                unattributed,
                ago,
                ..
            } => {
                let lane = color.map_or(muted, |c| styles.agent(c));
                pen.gap(1)
                    .text(g.lane_edge, lane)
                    .gap(1)
                    .text(g.commit, lane)
                    .gap(1)
                    .text(short, styles.fg(ColorToken::GitBranchBase))
                    .gap(1)
                    .safe(summary, styles.fg(ColorToken::TextDefault));
                if let Some(merge) = merge {
                    pen.gap(1).safe(merge, muted);
                }
                pen.gap(2).safe(unattributed, muted).gap(2).safe(ago, muted);
            }
            GraphRow::Tail(text) => {
                pen.gap(1).text(g.lane_edge, muted).gap(1).safe(text, muted);
            }
        }
    }
}
