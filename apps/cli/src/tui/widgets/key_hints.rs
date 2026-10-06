//! KeyHints (the bottom line, always visible) and Help (`?`) (DSYS-GRP-001 § 3).
//!
//! Both are fed from the single action ↔ keys table of `tui::keymap` (INF-CKP-001), so they
//! cannot diverge from the input.

use gitraptor_theme::ColorToken;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, fill, panel, width};

/// One action and its keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyHint {
    pub keys: SafeText,
    pub label: SafeText,
    /// Why the action is not available now. Shown as text, not only dimmed.
    pub disabled: Option<SafeText>,
}

impl KeyHint {
    pub fn new(keys: SafeText, label: SafeText) -> Self {
        Self {
            keys,
            label,
            disabled: None,
        }
    }

    pub fn disabled(mut self, reason: SafeText) -> Self {
        self.disabled = Some(reason);
        self
    }

    fn columns(&self, styles: &Styles) -> u16 {
        let g = styles.glyphs;
        let reason = self.disabled.as_ref().map_or(0, |r| {
            1 + width(g.paren[0]) + width(r.as_str()) + width(g.paren[1])
        });
        width(self.keys.as_str()) + 1 + width(self.label.as_str()) + reason
    }

    /// Paints the hint; returns false when it does not fit whole.
    pub(crate) fn paint(&self, pen: &mut Pen<'_>, styles: &Styles) -> bool {
        if self.columns(styles) > pen.remaining() {
            return false;
        }
        let (keys, label) = match self.disabled {
            None => (key_style(styles), styles.fg(ColorToken::TextDefault)),
            Some(_) => (
                styles.fg(ColorToken::TextMuted),
                styles.fg(ColorToken::TextMuted),
            ),
        };
        pen.safe(&self.keys, keys).gap(1).safe(&self.label, label);
        if let Some(reason) = &self.disabled {
            let g = styles.glyphs;
            pen.gap(1)
                .text(g.paren[0], label)
                .safe(reason, label)
                .text(g.paren[1], label);
        }
        true
    }
}

/// The one-line bar at the bottom of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyHintsModel {
    pub hints: Vec<KeyHint>,
    /// Always shown, at the right end, whatever the width.
    pub help: KeyHint,
}

impl Component for KeyHintsModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        fill(buf, area, styles);
        if area.height == 0 {
            return;
        }
        let help_width = self.help.columns(styles);
        let right = Rect::new(
            area.right().saturating_sub(help_width + 1),
            area.y,
            help_width.min(area.width),
            1,
        );
        self.help
            .paint(&mut Pen::new(buf, right, area.y, styles), styles);

        let left = Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(help_width + 3),
            1,
        );
        let mut pen = Pen::new(buf, left, area.y, styles);
        for (i, hint) in self.hints.iter().enumerate() {
            if i > 0 {
                if pen.remaining() < 2 {
                    break;
                }
                pen.gap(2);
            }
            if !hint.paint(&mut pen, styles) {
                break;
            }
        }
    }

    fn height(&self, _width: u16) -> u16 {
        1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpGroup {
    pub title: SafeText,
    pub hints: Vec<KeyHint>,
}

/// The `?` overlay: every action of the keymap, by group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpModel {
    pub title: SafeText,
    pub groups: Vec<HelpGroup>,
    /// A line under the groups (e.g. that `--plain` is read-only, R-CKP-10).
    pub note: Option<SafeText>,
    pub close: KeyHint,
}

impl HelpModel {
    fn key_column(&self) -> u16 {
        self.groups
            .iter()
            .flat_map(|g| &g.hints)
            .map(|h| width(h.keys.as_str()))
            .max()
            .unwrap_or(0)
    }

    fn rows(&self) -> u16 {
        let entries: usize = self.groups.iter().map(|g| g.hints.len() + 2).sum();
        u16::try_from(entries).unwrap_or(u16::MAX) + u16::from(self.note.is_some()) * 2 + 1
    }
}

impl Component for HelpModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let border = styles.fg(ColorToken::AccentDefault);
        let inner = panel(buf, area, &self.title, None, true, border, styles);
        let inner = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        let keys = self.key_column();
        let mut y = inner.y;
        let bottom = inner.bottom().saturating_sub(1);
        let heading = styles
            .fg(ColorToken::TextDefault)
            .add_modifier(Modifier::BOLD);
        'groups: for group in &self.groups {
            if y >= bottom {
                break;
            }
            Pen::new(buf, inner, y, styles).safe(&group.title, heading);
            y += 1;
            for hint in &group.hints {
                if y >= bottom {
                    break 'groups;
                }
                let mut pen = Pen::new(buf, inner, y, styles);
                pen.gap(2);
                let start = pen.x;
                let style = if hint.disabled.is_some() {
                    styles.fg(ColorToken::TextMuted)
                } else {
                    key_style(styles)
                };
                pen.safe(&hint.keys, style).to(start + keys + 2);
                let label = if hint.disabled.is_some() {
                    styles.fg(ColorToken::TextMuted)
                } else {
                    styles.fg(ColorToken::TextDefault)
                };
                pen.safe(&hint.label, label);
                if let Some(reason) = &hint.disabled {
                    let g = styles.glyphs;
                    pen.gap(1)
                        .text(g.paren[0], label)
                        .safe(reason, label)
                        .text(g.paren[1], label);
                }
                y += 1;
            }
            y += 1;
        }
        if let Some(note) = &self.note
            && y < bottom
        {
            Pen::new(buf, inner, y, styles).safe(note, styles.fg(ColorToken::TextMuted));
        }
        let mut pen = Pen::new(buf, inner, inner.bottom().saturating_sub(1), styles);
        pen.to(inner.right().saturating_sub(self.close.columns(styles)));
        self.close.paint(&mut pen, styles);
    }

    fn height(&self, _width: u16) -> u16 {
        self.rows() + 2
    }
}

/// Text style of a key.
fn key_style(styles: &Styles) -> Style {
    styles
        .fg(ColorToken::AccentDefault)
        .add_modifier(Modifier::BOLD)
}
