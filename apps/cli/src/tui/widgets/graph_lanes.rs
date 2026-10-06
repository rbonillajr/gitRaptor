//! GraphLanes: the agents' branches over the base, one lane per branch (DSYS-GRP-001 § 3;
//! US-CKP-022).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneOwner {
    Agent {
        color_index: usize,
        name: SafeText,
    },
    /// Commits no agent session claims (US-CKP-022): muted, with their label.
    Unattributed(SafeText),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneModel {
    pub owner: LaneOwner,
    /// Commits of the branch not in the base.
    pub ahead: u32,
    /// Commits of the base not in the branch.
    pub behind: u32,
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    Found(SafeText),
    /// The base branch does not exist: the message says so and the lanes hang loose.
    Missing(SafeText),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphModel {
    pub title: SafeText,
    pub base: Base,
    pub lanes: Vec<LaneModel>,
    /// Lanes that do not fit collapse into "+N <more>".
    pub more: SafeText,
    pub focused: bool,
}

/// Commit nodes drawn per lane; the count says the rest.
const MAX_NODES: u32 = 6;

impl Component for GraphModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let border = styles.fg(ColorToken::TextMuted);
        let inner = panel(buf, area, &self.title, None, self.focused, border, styles);
        if inner.height == 0 {
            return;
        }
        let inner = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        );
        let g = styles.glyphs;
        let base_style = styles.fg(ColorToken::GitBranchBase);
        let muted = styles.fg(ColorToken::TextMuted);
        let mut pen = Pen::new(buf, inner, inner.y, styles);
        let found = match &self.base {
            Base::Found(name) => {
                pen.text(g.commit, base_style)
                    .gap(1)
                    .safe(name, base_style.add_modifier(Modifier::BOLD));
                true
            }
            Base::Missing(message) => {
                let warn = styles.fg(ColorToken::StatusWarning);
                pen.symbol(styles.symbol(SymbolToken::Warning), warn)
                    .gap(1)
                    .safe(message, warn);
                false
            }
        };

        let rows = usize::from(inner.height - 1);
        let (shown, hidden) = if self.lanes.len() > rows {
            (
                rows.saturating_sub(1),
                self.lanes.len() - rows.saturating_sub(1),
            )
        } else {
            (self.lanes.len(), 0)
        };
        for (i, lane) in self.lanes.iter().take(shown).enumerate() {
            let y = inner.y + 1 + u16::try_from(i).unwrap_or(0);
            let last = i + 1 == self.lanes.len() && hidden == 0;
            let mut pen = Pen::new(buf, inner, y, styles);
            let fork = match (found, last) {
                (false, _) => g.lane_edge,
                (true, true) => g.lane_last,
                (true, false) => g.lane_fork,
            };
            let (lane_style, name, name_style) = match &lane.owner {
                LaneOwner::Agent { color_index, name } => {
                    let s = styles.agent(*color_index);
                    (s, name, s.add_modifier(Modifier::BOLD))
                }
                LaneOwner::Unattributed(label) => (muted, label, muted),
            };
            pen.text(fork, base_style).text(g.lane_edge, lane_style);
            for _ in 0..lane.ahead.min(MAX_NODES) {
                pen.text(g.lane_edge, lane_style).text(g.commit, lane_style);
            }
            pen.gap(1).safe(name, name_style).gap(1);
            let counts = format!("{}{} {}{}", g.ahead, lane.ahead, g.behind, lane.behind);
            pen.text(&counts, muted);
            if lane.conflict {
                pen.gap(1).symbol(
                    styles.symbol(SymbolToken::Conflict),
                    styles.fg(ColorToken::StatusWarning),
                );
            }
        }
        if hidden > 0 {
            let y = inner.y + 1 + u16::try_from(shown).unwrap_or(0);
            let mut pen = Pen::new(buf, inner, y, styles);
            if found {
                pen.text(g.lane_last, base_style).gap(1);
            }
            pen.text(&format!("+{hidden}"), muted)
                .gap(1)
                .safe(&self.more, muted);
        }
    }
}
