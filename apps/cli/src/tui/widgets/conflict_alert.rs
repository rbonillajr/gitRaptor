//! ConflictAlert: a predicted conflict between two agents (DSYS-GRP-001 § 3;
//! US-CKP-006, 007, 008, 009).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRef {
    pub color_index: usize,
    pub name: SafeText,
}

/// How fresh the prediction is (US-CKP-007, 009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prediction {
    Current,
    /// Out of date, recalculating.
    Stale,
    Calculating,
    /// The base is not confirmed yet.
    PendingBase,
    NotComputable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictAlertModel {
    pub title: SafeText,
    pub left: AgentRef,
    pub right: AgentRef,
    pub files: Vec<SafeText>,
    /// "+N <more>" when the files do not fit.
    pub more: SafeText,
    pub prediction: Prediction,
    /// The freshness as text: "3 min ago", "outdated, recalculating"…
    pub status: SafeText,
    /// Badge of a conflict that just appeared (US-CKP-008).
    pub new: Option<SafeText>,
    pub focused: bool,
}

impl Component for ConflictAlertModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let warn = styles.fg(ColorToken::StatusWarning);
        let inner = panel(
            buf,
            area,
            &self.title,
            Some(SymbolToken::Conflict),
            self.focused,
            warn,
            styles,
        );
        if inner.height == 0 {
            return;
        }
        let inner = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        let g = styles.glyphs;
        let muted = styles.fg(ColorToken::TextMuted);

        let mut pen = Pen::new(buf, inner, inner.y, styles);
        if let Some(badge) = &self.new {
            pen.text(g.button[0], warn)
                .safe(badge, warn.add_modifier(Modifier::BOLD))
                .text(g.button[1], warn)
                .gap(1);
        }
        let agent = |a: &AgentRef| styles.agent(a.color_index).add_modifier(Modifier::BOLD);
        pen.safe(&self.left.name, agent(&self.left))
            .gap(1)
            .text(g.versus, muted)
            .gap(1)
            .safe(&self.right.name, agent(&self.right));

        if inner.height < 2 {
            return;
        }
        let (sym, token) = match self.prediction {
            Prediction::Current => (None, ColorToken::TextMuted),
            Prediction::Stale => (Some(SymbolToken::Warning), ColorToken::StatusWarning),
            Prediction::Calculating | Prediction::PendingBase => {
                (Some(SymbolToken::Info), ColorToken::StatusInfo)
            }
            Prediction::NotComputable => (Some(SymbolToken::Error), ColorToken::StatusDanger),
        };
        let mut pen = Pen::new(buf, inner, inner.y + 1, styles);
        if let Some(sym) = sym {
            pen.symbol(styles.symbol(sym), styles.fg(token)).gap(1);
        }
        pen.safe(&self.status, styles.fg(token));

        let rows = usize::from(inner.height - 2);
        let (shown, hidden) = if self.files.len() > rows {
            (
                rows.saturating_sub(1),
                self.files.len() - rows.saturating_sub(1),
            )
        } else {
            (self.files.len(), 0)
        };
        let file = styles.fg(ColorToken::GitConflict);
        let mut y = inner.y + 2;
        for f in self.files.iter().take(shown) {
            Pen::new(buf, inner, y, styles)
                .gap(1)
                .text(g.bullet, muted)
                .gap(1)
                .safe(f, file);
            y += 1;
        }
        if hidden > 0 {
            Pen::new(buf, inner, y, styles)
                .gap(1)
                .text(&format!("+{hidden}"), muted)
                .gap(1)
                .safe(&self.more, muted);
        }
    }

    fn height(&self, _width: u16) -> u16 {
        // Border, agents, status and the files.
        4 + u16::try_from(self.files.len()).unwrap_or(u16::MAX)
    }
}
