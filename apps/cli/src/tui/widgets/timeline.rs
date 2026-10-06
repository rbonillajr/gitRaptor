//! TimelineList: the Time Machine snapshots with who, when and what, and the undo / restore
//! actions (DSYS-GRP-001 § 3; US-CKP-021, 016).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::Component;
use super::key_hints::KeyHint;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub when: SafeText,
    pub actor: SafeText,
    /// Agent color of the actor; `None` for the developer or an unknown actor.
    pub actor_color: Option<usize>,
    pub summary: SafeText,
    /// The entry can be undone (⟲).
    pub undoable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineNotice {
    /// Old snapshots will be purged (US-CKP-021).
    Purge(SafeText),
    /// Undo is not available now, with the reason (US-CKP-016).
    UndoDisabled(SafeText),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineModel {
    pub title: SafeText,
    pub entries: Vec<TimelineEntry>,
    pub selected: Option<usize>,
    pub offset: usize,
    pub focused: bool,
    pub empty: SafeText,
    pub notice: Option<TimelineNotice>,
    /// Actions on the selected entry (undo, restore), always visible at the bottom.
    pub actions: Vec<KeyHint>,
}

const MARK: u16 = 2;
const WHEN: u16 = 7;
const ACTOR: u16 = 12;

impl Component for TimelineModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let border = styles.fg(ColorToken::TextMuted);
        let inner = panel(buf, area, &self.title, None, self.focused, border, styles);
        if inner.height == 0 {
            return;
        }
        let muted = styles.fg(ColorToken::TextMuted);
        let mut bottom = inner.bottom();

        // Actions, then the notice, from the bottom up: they never scroll away.
        if !self.actions.is_empty() && bottom > inner.y {
            bottom -= 1;
            let mut pen = Pen::new(buf, inner, bottom, styles);
            pen.gap(1);
            for (i, hint) in self.actions.iter().enumerate() {
                if i > 0 {
                    pen.gap(2);
                }
                if !hint.paint(&mut pen, styles) {
                    break;
                }
            }
        }
        if let Some(notice) = &self.notice
            && bottom > inner.y
        {
            bottom -= 1;
            let (sym, token, text) = match notice {
                TimelineNotice::Purge(t) => (SymbolToken::Warning, ColorToken::StatusWarning, t),
                TimelineNotice::UndoDisabled(t) => (SymbolToken::Info, ColorToken::StatusInfo, t),
            };
            let style = styles.fg(token);
            Pen::new(buf, inner, bottom, styles)
                .gap(1)
                .symbol(styles.symbol(sym), style)
                .gap(1)
                .safe(text, style);
        }

        if self.entries.is_empty() {
            Pen::new(buf, inner, inner.y, styles)
                .gap(MARK)
                .safe(&self.empty, muted);
            return;
        }
        let undo = styles.symbol(SymbolToken::Undo);
        let rows = usize::from(bottom - inner.y);
        for (i, entry) in self.entries.iter().enumerate().skip(self.offset).take(rows) {
            let y = inner.y + u16::try_from(i - self.offset).unwrap_or(0);
            let line = Rect::new(inner.x, y, inner.width, 1);
            let selected = self.selected == Some(i);
            let row = if selected {
                let s = styles.bg(ColorToken::BgSelected);
                buf.set_style(line, s);
                s
            } else {
                Style::default()
            };
            let mut pen = Pen::new(buf, line, y, styles);
            if selected && self.focused {
                pen.cell(
                    styles.glyphs.focus,
                    MARK,
                    styles.fg(ColorToken::FocusDefault),
                );
            } else {
                pen.gap(MARK);
            }
            let start = pen.x;
            if entry.undoable {
                pen.symbol(undo, styles.fg(ColorToken::StatusSuccess).patch(row));
            }
            pen.to(start + u16::from(undo.width) + 1);
            pen.cell(entry.when.as_str(), WHEN, muted.patch(row));
            let actor = entry
                .actor_color
                .map_or(styles.fg(ColorToken::TextDefault), |i| styles.agent(i));
            pen.cell(entry.actor.as_str(), ACTOR, actor.patch(row))
                .safe(
                    &entry.summary,
                    styles.fg(ColorToken::TextDefault).patch(row),
                );
        }
    }
}
