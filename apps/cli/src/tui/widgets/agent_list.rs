//! AgentList / AgentRow: agents and worktrees with state, branch, changes, ahead/behind and
//! last activity (DSYS-GRP-001 § 3; US-CKP-001, 002, 003, 016).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel, width};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Active,
    Idle,
    Done,
    /// The engine cannot read the worktree now (US-CKP-003).
    Unavailable,
    /// No agent session in the worktree: the hollow ○, the name says so ("No agent · <worktree>",
    /// BR-CKP-CONS-003) and the row is muted, so it reads without color (US-CKP-001).
    NoAgent,
    /// Whether the worktree has an agent is not known: no state symbol, muted.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Branch {
    Named(SafeText),
    /// Detached HEAD, with its label (e.g. "a1b2c3d (no branch)").
    Detached(SafeText),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sync {
    Known {
        ahead: u32,
        behind: u32,
    },
    /// Ahead/behind cannot be computed, with the reason as a short text.
    Unknown(SafeText),
}

/// An operation in progress on the worktree (US-CKP-016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub label: SafeText,
    pub elapsed: SafeText,
    pub cancel: super::key_hints::KeyHint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRowModel {
    /// 0-based position: picks `agent.n`, reused from the ninth agent on (BR-CKP-EDGE-006).
    pub color_index: usize,
    pub name: SafeText,
    pub state: AgentState,
    /// Shown instead of the branch when the state is [`AgentState::Unavailable`].
    pub state_label: SafeText,
    pub branch: Branch,
    pub changes: u32,
    pub sync: Sync,
    pub activity: SafeText,
    /// A label after the branch, muted (e.g. "temporary").
    pub tag: Option<SafeText>,
    pub conflict: bool,
    pub blocked: bool,
    pub operation: Option<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentColumns {
    pub agent: SafeText,
    pub branch: SafeText,
    pub changes: SafeText,
    pub sync: SafeText,
    pub activity: SafeText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentListModel {
    pub title: SafeText,
    pub columns: AgentColumns,
    pub rows: Vec<AgentRowModel>,
    pub selected: Option<usize>,
    /// First visible row; `follow` keeps the selection in view.
    pub offset: usize,
    pub focused: bool,
    /// Shown when there are no rows.
    pub empty: SafeText,
}

const MARK: u16 = 2;
/// The name column grows from `NAME` up to `NAME_MAX` to fit the widest name, as long as the
/// branch keeps `BRANCH_MIN` columns (US-CKP-001: "No agent · <worktree>").
const NAME: u16 = 14;
const NAME_MAX: u16 = 26;
const BRANCH_MIN: u16 = 12;
const CHANGES: u16 = 6;
const SYNC: u16 = 12;
/// The activity column grows the same way up to `ACTIVITY_MAX` ("not available").
const ACTIVITY: u16 = 11;
const ACTIVITY_MAX: u16 = 14;

/// Column widths for an inner width: activity goes first, then ahead/behind.
#[derive(Debug, Clone, Copy)]
struct Cols {
    /// The widest state symbol of the active set, plus a space.
    state: u16,
    name: u16,
    branch: u16,
    sync: u16,
    activity: u16,
}

impl Cols {
    /// `widest` holds the widest name and the widest activity text of all the rows (not only
    /// the visible ones, so the columns do not jump while scrolling); `flags` whether any row
    /// has a ⚡ or ⛔ to show, the only case their columns take room.
    fn new(inner: u16, styles: &Styles, widest: (u16, u16), flags: bool) -> Self {
        let flags = if flags {
            u16::from(styles.symbol(SymbolToken::Conflict).width)
                + 1
                + u16::from(styles.symbol(SymbolToken::Blocked).width)
                + 1
        } else {
            0
        };
        let state = [
            SymbolToken::AgentActive,
            SymbolToken::AgentIdle,
            SymbolToken::AgentDone,
            SymbolToken::Warning,
        ]
        .iter()
        .map(|t| u16::from(styles.symbol(*t).width))
        .max()
        .unwrap_or(1)
            + 1;
        let base = MARK + state + 1 + CHANGES + flags;
        let fixed = base + NAME;
        // A cell keeps one blank column, so a text needs its width plus one.
        let wanted = (widest.1 + 1).clamp(ACTIVITY, ACTIVITY_MAX);
        let activity = if inner >= fixed + SYNC + wanted + 12 {
            wanted
        } else if inner >= fixed + SYNC + ACTIVITY + 12 {
            ACTIVITY
        } else {
            0
        };
        let sync = if inner >= fixed + SYNC + activity + 10 {
            SYNC
        } else {
            0
        };
        let room = inner.saturating_sub(base + sync + activity + BRANCH_MIN);
        let name = (widest.0 + 1).clamp(NAME, NAME_MAX).min(room.max(NAME));
        let branch = inner.saturating_sub(base + name + sync + activity);
        Self {
            state,
            name,
            branch,
            sync,
            activity,
        }
    }
}

impl AgentRowModel {
    fn state_symbol(&self) -> Option<(SymbolToken, ColorToken)> {
        Some(match self.state {
            AgentState::Active => (SymbolToken::AgentActive, ColorToken::AgentStateActive),
            AgentState::Idle => (SymbolToken::AgentIdle, ColorToken::AgentStateIdle),
            AgentState::Done => (SymbolToken::AgentDone, ColorToken::AgentStateDone),
            AgentState::Unavailable => (SymbolToken::Warning, ColorToken::StatusWarning),
            AgentState::NoAgent => (SymbolToken::AgentDone, ColorToken::TextMuted),
            AgentState::Unknown => return None,
        })
    }

    fn paint(
        &self,
        pen: &mut Pen<'_>,
        cols: Cols,
        row: Style,
        focus: Option<Style>,
        styles: &Styles,
    ) {
        let g = styles.glyphs;
        match focus {
            Some(f) => pen.cell(g.focus, MARK, f),
            None => pen.gap(MARK),
        };
        let start = pen.x;
        if let Some((sym, token)) = self.state_symbol() {
            pen.symbol(styles.symbol(sym), styles.fg(token).patch(row));
        }
        pen.to(start + cols.state);
        let muted = styles.fg(ColorToken::TextMuted).patch(row);
        let name = if matches!(self.state, AgentState::NoAgent | AgentState::Unknown) {
            muted
        } else {
            styles
                .agent(self.color_index)
                .patch(row)
                .add_modifier(Modifier::BOLD)
        };
        pen.cell(self.name.as_str(), cols.name, name).gap(1);

        // A row without an agent is muted whole, so it recedes without relying on color alone
        // (the ○ and the name say it too).
        let text = if self.state == AgentState::NoAgent {
            muted
        } else {
            styles.fg(ColorToken::TextDefault).patch(row)
        };
        let middle = cols.branch + CHANGES + cols.sync + cols.activity;
        let start = pen.x;
        if let Some(op) = &self.operation {
            let warn = styles.fg(ColorToken::StatusWarning).patch(row);
            pen.safe(&op.label, warn)
                .gap(1)
                .safe(&op.elapsed, muted)
                .gap(2);
            op.cancel.paint(pen, styles);
        } else if self.state == AgentState::Unavailable {
            pen.safe(
                &self.state_label,
                styles.fg(ColorToken::StatusWarning).patch(row),
            );
        } else {
            // The tag keeps its room at the end of the branch column while the branch keeps
            // `BRANCH_MIN`; otherwise it is left out.
            let tag = (self.tag.as_ref()).map(|t| {
                let label = format!("{}{}", g.separator.trim_start(), t.as_str());
                let w = width(&label) + 1;
                (label, w)
            });
            let tag = tag.filter(|(_, w)| cols.branch >= BRANCH_MIN + w);
            let (b, style) = match &self.branch {
                Branch::Named(b) => (b, text),
                Branch::Detached(b) => (b, styles.fg(ColorToken::StatusWarning).patch(row)),
            };
            match &tag {
                // The tag right after the branch, the rest of the column blank.
                Some((label, w)) => {
                    let branch = (cols.branch - w).min(width(b.as_str()) + 1);
                    pen.cell(b.as_str(), branch, style)
                        .cell(label, *w, muted)
                        .gap(cols.branch - branch - w);
                }
                None => {
                    pen.cell(b.as_str(), cols.branch, style);
                }
            }
            let changes = format!("{}{}", g.changes, self.changes);
            pen.cell(
                &changes,
                CHANGES,
                if self.changes > 0 {
                    styles.fg(ColorToken::GitModified).patch(row)
                } else {
                    muted
                },
            );
            if cols.sync > 0 {
                match &self.sync {
                    Sync::Known { ahead, behind } => {
                        let s = format!("{}{ahead} {}{behind}", g.ahead, g.behind);
                        pen.cell(&s, cols.sync, text)
                    }
                    Sync::Unknown(reason) => pen.cell(reason.as_str(), cols.sync, muted),
                };
            }
            if cols.activity > 0 {
                pen.cell(self.activity.as_str(), cols.activity, muted);
            }
        }
        pen.to(start + middle);
        if self.conflict {
            pen.symbol(
                styles.symbol(SymbolToken::Conflict),
                styles.fg(ColorToken::StatusWarning).patch(row),
            )
            .gap(1);
        }
        if self.blocked {
            pen.symbol(
                styles.symbol(SymbolToken::Blocked),
                styles.fg(ColorToken::StatusDanger).patch(row),
            );
        }
    }
}

impl Component for AgentListModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let border = styles.fg(ColorToken::TextMuted);
        let inner = panel(buf, area, &self.title, None, self.focused, border, styles);
        if inner.height == 0 {
            return;
        }
        let widest = self.rows.iter().fold((0, 0), |(n, a), r| {
            (
                n.max(width(r.name.as_str())),
                a.max(width(r.activity.as_str())),
            )
        });
        let flags = self.rows.iter().any(|r| r.conflict || r.blocked);
        let cols = Cols::new(inner.width, styles, widest, flags);
        let head = styles
            .fg(ColorToken::TextMuted)
            .add_modifier(Modifier::BOLD);
        let mut pen = Pen::new(buf, inner, inner.y, styles);
        pen.gap(MARK + cols.state)
            .cell(self.columns.agent.as_str(), cols.name, head)
            .gap(1)
            .cell(self.columns.branch.as_str(), cols.branch, head)
            .cell(self.columns.changes.as_str(), CHANGES, head);
        if cols.sync > 0 {
            pen.cell(self.columns.sync.as_str(), cols.sync, head);
        }
        if cols.activity > 0 {
            pen.cell(self.columns.activity.as_str(), cols.activity, head);
        }

        if self.rows.is_empty() {
            if inner.height > 1 {
                Pen::new(buf, inner, inner.y + 1, styles)
                    .gap(MARK + cols.state)
                    .safe(&self.empty, styles.fg(ColorToken::TextMuted));
            }
            return;
        }
        let visible = usize::from(inner.height - 1);
        for (i, row) in self.rows.iter().enumerate().skip(self.offset).take(visible) {
            let y = inner.y + 1 + u16::try_from(i - self.offset).unwrap_or(0);
            let selected = self.selected == Some(i);
            let line = Rect::new(inner.x, y, inner.width, 1);
            let row_style = if selected {
                let s = styles.bg(ColorToken::BgSelected);
                buf.set_style(line, s);
                s
            } else {
                Style::default()
            };
            let focus = (selected && self.focused).then(|| styles.fg(ColorToken::FocusDefault));
            row.paint(
                &mut Pen::new(buf, line, y, styles),
                cols,
                row_style,
                focus,
                styles,
            );
        }
    }
}
