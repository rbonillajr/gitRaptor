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
    /// Waiting for something other than the base (an operation, power saving…).
    Pending,
    NotComputable,
}

impl Prediction {
    /// The symbol (if any) and color a freshness line is drawn with.
    fn look(self) -> (Option<SymbolToken>, ColorToken) {
        match self {
            Self::Current => (None, ColorToken::TextMuted),
            Self::Stale => (Some(SymbolToken::Warning), ColorToken::StatusWarning),
            Self::Calculating | Self::PendingBase | Self::Pending => {
                (Some(SymbolToken::Info), ColorToken::StatusInfo)
            }
            Self::NotComputable => (Some(SymbolToken::Error), ColorToken::StatusDanger),
        }
    }
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
        let (sym, token) = self.prediction.look();
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

/// What a pair is, one line each: ⚡ or ⚠, its two sides and the files the panel has room for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictEntry {
    /// ⚡ (a predicted conflict) rather than ⚠ (an overlap).
    pub conflict: bool,
    pub left: AgentRef,
    pub right: AgentRef,
    /// The age of the result, or "outdated … recalculating" while it is being replaced.
    pub status: Option<SafeText>,
    pub files: Vec<AlertFile>,
    /// The result is being replaced: the status is painted as a warning.
    pub stale: bool,
    /// Draw the ⚡ or ⚠. A pair recalculating without either has none, and no files.
    pub marked: bool,
    /// The ⚡ appeared in this run: "new", for as long as it lasts.
    pub new: Option<SafeText>,
}

/// An aggregate line of the panel: pairs calculating, pending, not computable or all clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelLine {
    pub prediction: Prediction,
    pub text: SafeText,
}

/// A file of a pair: its path, how it clashes (line ranges per side, or the kind) and, when a
/// merge rule of its own applies, the mark that says the prediction may not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertFile {
    pub path: SafeText,
    pub detail: SafeText,
    pub merge_rule: Option<SafeText>,
}

impl AgentRef {
    /// The color index of the base branch, which is no agent: it is painted muted.
    pub const BASE: usize = usize::MAX;
}

/// The conflicts panel under the fleet: a line per pair and its files while there is room, then
/// "+N more pairs", and always the limits line on the last row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictPanelModel {
    pub title: SafeText,
    pub entries: Vec<ConflictEntry>,
    /// "+N pairs more": empty when every pair fits.
    pub hidden: SafeText,
    /// The aggregate lines, above the limits line.
    pub summary: Vec<PanelLine>,
    /// The three pieces of the limits line: with uncommitted work, commits only, no merge rules.
    pub limits: [SafeText; 3],
}

impl ConflictPanelModel {
    /// Rows the panel wants at `width` ([`Component::height`]), callable without the trait.
    pub fn height(&self, width: u16) -> u16 {
        Component::height(self, width)
    }
}

impl Component for ConflictPanelModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let warn = styles.fg(ColorToken::StatusWarning);
        let inner = panel(
            buf,
            area,
            &self.title,
            Some(SymbolToken::Conflict),
            false,
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
        let file = styles.fg(ColorToken::GitConflict);
        let agent = |a: &AgentRef| {
            if a.color_index == AgentRef::BASE {
                muted.add_modifier(Modifier::BOLD)
            } else {
                styles.agent(a.color_index).add_modifier(Modifier::BOLD)
            }
        };
        let limits_row = inner.bottom() - 1;
        let summary_rows = u16::try_from(self.summary.len()).unwrap_or(u16::MAX);
        // The aggregate lines sit just above the limits line; the pairs get what is above them.
        let last = limits_row.saturating_sub(summary_rows).max(inner.y);
        let mut y = inner.y;
        for entry in &self.entries {
            if y >= last {
                break;
            }
            let (symbol, token) = if entry.conflict {
                (SymbolToken::Conflict, ColorToken::StatusWarning)
            } else {
                (SymbolToken::Warning, ColorToken::StatusWarning)
            };
            let mut pen = Pen::new(buf, inner, y, styles);
            if entry.marked {
                pen.symbol(styles.symbol(symbol), styles.fg(token)).gap(1);
            }
            if let Some(badge) = &entry.new {
                pen.text(g.button[0], warn)
                    .safe(badge, warn.add_modifier(Modifier::BOLD))
                    .text(g.button[1], warn)
                    .gap(1);
            }
            pen.safe(&entry.left.name, agent(&entry.left))
                .gap(1)
                .text(g.versus, muted)
                .gap(1)
                .safe(&entry.right.name, agent(&entry.right));
            if let Some(status) = &entry.status {
                let style = if entry.stale {
                    styles.fg(Prediction::Stale.look().1)
                } else {
                    muted.add_modifier(Modifier::DIM)
                };
                pen.gap(2).safe(status, style);
            }
            y += 1;
            for f in &entry.files {
                if y >= last {
                    break;
                }
                let mut pen = Pen::new(buf, inner, y, styles);
                pen.gap(1).text(g.bullet, muted).gap(1).safe(&f.path, file);
                if !f.detail.as_str().is_empty() {
                    pen.gap(1).safe(&f.detail, muted);
                }
                if let Some(rule) = &f.merge_rule {
                    pen.gap(1).safe(rule, warn);
                }
                y += 1;
            }
        }
        if y < last && !self.hidden.as_str().is_empty() {
            Pen::new(buf, inner, y, styles).safe(&self.hidden, muted);
        }
        for (i, line) in self.summary.iter().enumerate() {
            let row = last + u16::try_from(i).unwrap_or(0);
            if row >= limits_row {
                break;
            }
            let (symbol, token) = line.prediction.look();
            let mut pen = Pen::new(buf, inner, row, styles);
            if let Some(symbol) = symbol {
                pen.symbol(styles.symbol(symbol), styles.fg(token)).gap(1);
            }
            pen.safe(&line.text, styles.fg(token));
        }
        let mut pen = Pen::new(buf, inner, limits_row, styles);
        // Unpadded: the line reads as the catalog's text, one space after each symbol.
        pen.text(styles.symbol(SymbolToken::Warning).text, warn)
            .gap(1)
            .safe(&self.limits[0], muted)
            .text(g.separator, muted)
            .symbol(styles.symbol(SymbolToken::Conflict), warn)
            .gap(1)
            .safe(&self.limits[1], muted)
            .text(g.separator, muted)
            .safe(&self.limits[2], muted);
    }

    fn height(&self, _width: u16) -> u16 {
        // Border, the pairs with their files, the "more pairs" row, the aggregate lines and the
        // limits line.
        let lines: usize = self.entries.iter().map(|e| 1 + e.files.len()).sum();
        let hidden = usize::from(!self.hidden.as_str().is_empty());
        u16::try_from(3 + lines + hidden + self.summary.len()).unwrap_or(u16::MAX)
    }
}
