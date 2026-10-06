//! Layout: the regions of the Cockpit, the StatusBar (header) and the minimum-size screen
//! (DSYS-GRP-001 § 3, ADR-CKP-003 § 7).
//!
//! One header line, the body and one KeyHints line. The body goes by priority:
//! list > alerts (⚡, ⛔) > graph > detail. Under 120 columns the graph collapses first and the
//! detail opens full screen on demand; from 120 columns the graph and the detail get a column.

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Component;
use super::key_hints::KeyHint;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, fill, width, wrap};

pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;
/// From this width the graph and the detail get a column of their own.
pub const WIDE: u16 = 120;
const LIST_MIN: u16 = 10;
const GRAPH_MIN: u16 = 6;
const ALERTS_MAX: u16 = 8;

/// What the body has to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BodyPlan {
    /// Rows the alerts want (0: no alerts).
    pub alerts: u16,
    pub graph: bool,
    pub detail: bool,
}

/// Where each part goes. `None` means collapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    pub header: Rect,
    pub list: Rect,
    pub alerts: Option<Rect>,
    pub graph: Option<Rect>,
    pub detail: Option<Rect>,
    pub hints: Rect,
}

/// Splits the screen. `None` below 80×24: only the minimum-size screen is painted.
pub fn layout(area: Rect, plan: BodyPlan) -> Option<Regions> {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return None;
    }
    let header = Rect::new(area.x, area.y, area.width, 1);
    let hints = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);

    let wide = area.width >= WIDE && (plan.graph || plan.detail);
    let left = if wide {
        Rect::new(body.x, body.y, body.width * 55 / 100, body.height)
    } else {
        body
    };

    let alerts_h = plan.alerts.min(ALERTS_MAX).min(left.height / 3);
    let alerts =
        (alerts_h > 0).then(|| Rect::new(left.x, left.bottom() - alerts_h, left.width, alerts_h));
    let rest = left.height - alerts_h;

    let (list, graph, detail);
    if wide {
        list = Rect::new(left.x, left.y, left.width, rest);
        let right = Rect::new(left.right(), body.y, body.width - left.width, body.height);
        match (plan.graph, plan.detail) {
            (true, true) => {
                let g = right.height * 2 / 5;
                graph = Some(Rect::new(right.x, right.y, right.width, g));
                detail = Some(Rect::new(
                    right.x,
                    right.y + g,
                    right.width,
                    right.height - g,
                ));
            }
            (true, false) => {
                graph = Some(right);
                detail = None;
            }
            _ => {
                graph = None;
                detail = Some(right);
            }
        }
    } else {
        let graph_h = if plan.graph && rest >= LIST_MIN + GRAPH_MIN {
            (rest - LIST_MIN).min(rest * 2 / 5).max(GRAPH_MIN)
        } else {
            0
        };
        list = Rect::new(left.x, left.y, left.width, rest - graph_h);
        graph =
            (graph_h > 0).then(|| Rect::new(left.x, left.y + rest - graph_h, left.width, graph_h));
        detail = None;
    }
    Some(Regions {
        header,
        list,
        alerts,
        graph,
        detail,
        hints,
    })
}

/// State of the connection to the engine (US-CKP-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Live,
    Reconnecting,
    Unavailable,
}

/// The header line. The ⚡ and ⛔ counts are mandatory and never cut: a filter or a preference
/// cannot hide an alert (L-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarModel {
    pub repo: SafeText,
    pub connection: Connection,
    pub connection_label: SafeText,
    /// Protection of the repo, when there is one (e.g. "main protected").
    pub protection: Option<SafeText>,
    /// Who is acting: the developer or "acting as claude-1" (US-CKP-019).
    pub requester: SafeText,
    pub conflicts: u32,
    pub blocked: u32,
}

impl Component for StatusBarModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        fill(buf, area, styles);
        if area.height == 0 {
            return;
        }
        let bar = styles.bg(ColorToken::BgHighlight);
        buf.set_style(area, bar);

        // Counts first, from the right, so nothing can push them out.
        let warn = styles.fg(ColorToken::StatusWarning).patch(bar);
        let danger = styles.fg(ColorToken::StatusDanger).patch(bar);
        let conflict = styles.symbol(SymbolToken::Conflict);
        let blocked = styles.symbol(SymbolToken::Blocked);
        let (c, b) = (self.conflicts.to_string(), self.blocked.to_string());
        let counts = u16::from(conflict.width)
            + 1
            + width(&c)
            + 2
            + u16::from(blocked.width)
            + 1
            + width(&b);
        let right = Rect::new(area.right().saturating_sub(counts + 1), area.y, counts, 1);
        let mut pen = Pen::new(buf, right, area.y, styles);
        pen.symbol(conflict, warn)
            .gap(1)
            .text(&c, warn.add_modifier(Modifier::BOLD))
            .gap(2)
            .symbol(blocked, danger)
            .gap(1)
            .text(&b, danger.add_modifier(Modifier::BOLD));

        let left = Rect::new(area.x + 1, area.y, area.width.saturating_sub(counts + 3), 1);
        let sep = styles.glyphs.separator;
        let muted = styles.fg(ColorToken::TextMuted).patch(bar);
        let text = styles.fg(ColorToken::TextDefault).patch(bar);
        let (token, sym) = match self.connection {
            Connection::Live => (ColorToken::StatusSuccess, SymbolToken::Success),
            Connection::Reconnecting => (ColorToken::StatusWarning, SymbolToken::Warning),
            Connection::Unavailable => (ColorToken::StatusDanger, SymbolToken::Error),
        };
        let state = styles.fg(token).patch(bar);
        let mut pen = Pen::new(buf, left, area.y, styles);
        pen.safe(&self.repo, text.add_modifier(Modifier::BOLD))
            .text(sep, muted)
            .symbol(styles.symbol(sym), state)
            .gap(1)
            .safe(&self.connection_label, state);
        if let Some(protection) = &self.protection {
            pen.text(sep, muted).safe(protection, text);
        }
        pen.text(sep, muted).safe(&self.requester, text);
    }

    fn height(&self, _width: u16) -> u16 {
        1
    }
}

/// What is painted below 80×24 instead of the Cockpit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooSmallModel {
    /// "The Cockpit needs at least 80×24", from the catalog.
    pub message: SafeText,
    /// The way out stays visible even here.
    pub quit: KeyHint,
}

impl Component for TooSmallModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        fill(buf, area, styles);
        let lines = wrap(self.message.as_str(), area.width.saturating_sub(4));
        let total = u16::try_from(lines.len()).unwrap_or(u16::MAX) + 2;
        let mut y = area.y + area.height.saturating_sub(total) / 2;
        let sym = styles.symbol(SymbolToken::Warning);
        let warn = styles.fg(ColorToken::StatusWarning);
        for (i, line) in lines.iter().enumerate() {
            if y >= area.bottom() {
                return;
            }
            let lead = if i == 0 { u16::from(sym.width) + 1 } else { 0 };
            let w = width(line) + lead;
            let x = area.x + area.width.saturating_sub(w) / 2;
            let mut pen = Pen::new(buf, Rect::new(x, y, area.right() - x, 1), y, styles);
            if i == 0 {
                pen.symbol(sym, warn).gap(1);
            }
            pen.text(line, styles.fg(ColorToken::TextDefault));
            y += 1;
        }
        y += 1;
        if y < area.bottom() {
            let mut pen = Pen::new(
                buf,
                Rect::new(area.x + 2, y, area.width.saturating_sub(4), 1),
                y,
                styles,
            );
            pen.to(area.x
                + area.width.saturating_sub(
                    width(self.quit.keys.as_str()) + width(self.quit.label.as_str()) + 1,
                ) / 2);
            self.quit.paint(&mut pen, styles);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: BodyPlan = BodyPlan {
        alerts: 4,
        graph: true,
        detail: true,
    };

    #[test]
    fn below_80x24_there_is_no_layout() {
        assert_eq!(layout(Rect::new(0, 0, 79, 24), PLAN), None);
        assert_eq!(layout(Rect::new(0, 0, 80, 23), PLAN), None);
    }

    #[test]
    fn hints_and_header_are_always_there() {
        for (w, h) in [(80, 24), (100, 30), (120, 40)] {
            let r = layout(Rect::new(0, 0, w, h), PLAN).unwrap();
            assert_eq!(r.header, Rect::new(0, 0, w, 1));
            assert_eq!(r.hints, Rect::new(0, h - 1, w, 1));
        }
    }

    #[test]
    fn the_graph_collapses_before_the_list_and_the_alerts() {
        let tight = BodyPlan {
            alerts: 8,
            graph: true,
            detail: false,
        };
        let r = layout(Rect::new(0, 0, 80, 24), tight).unwrap();
        assert!(r.alerts.is_some());
        assert_eq!(r.graph, None);
        assert!(r.list.height >= LIST_MIN);
        let r = layout(Rect::new(0, 0, 100, 30), tight).unwrap();
        assert!(r.graph.is_some() && r.list.height >= LIST_MIN);
    }

    #[test]
    fn narrow_screens_open_the_detail_on_demand_and_wide_ones_give_it_a_column() {
        assert_eq!(layout(Rect::new(0, 0, 100, 30), PLAN).unwrap().detail, None);
        let r = layout(Rect::new(0, 0, 120, 40), PLAN).unwrap();
        let (graph, detail) = (r.graph.unwrap(), r.detail.unwrap());
        assert_eq!(graph.x, r.list.right());
        assert_eq!(detail.y, graph.bottom());
    }
}
