//! RepoPicker: the observed repos to choose from when the TUI starts outside all of them
//! (US-CKP-001, dogfooding 2026-10-06). One row per repo with its name and path; the selected
//! one carries the focus marker and the selection background.

use gitraptor_theme::ColorToken;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel, width};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoChoiceModel {
    pub name: SafeText,
    pub path: SafeText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPickerModel {
    pub title: SafeText,
    /// What to do, above the list ("This folder is not in an observed repo: choose one").
    pub prompt: SafeText,
    pub repos: Vec<RepoChoiceModel>,
    pub selected: usize,
    /// First visible row; follows the selection ([`follow`](crate::tui::style::follow)).
    pub offset: usize,
}

impl Component for RepoPickerModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let border = styles.fg(ColorToken::TextMuted);
        let inner = panel(buf, area, &self.title, None, true, border, styles);
        if inner.height == 0 {
            return;
        }
        Pen::new(buf, inner, inner.y, styles)
            .gap(1)
            .safe(&self.prompt, styles.fg(ColorToken::TextMuted));
        let marker = width(styles.glyphs.focus);
        let name_width = self
            .repos
            .iter()
            .map(|r| width(r.name.as_str()))
            .max()
            .unwrap_or(0)
            .saturating_add(2);
        let visible = usize::from(inner.height.saturating_sub(2));
        for (i, repo) in self
            .repos
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(visible)
        {
            let y = inner.y + 2 + u16::try_from(i - self.offset).unwrap_or(0);
            let line = Rect::new(inner.x, y, inner.width, 1);
            let selected = i == self.selected;
            let row = if selected {
                let s = styles.bg(ColorToken::BgSelected);
                buf.set_style(line, s);
                s
            } else {
                Style::default()
            };
            let mut pen = Pen::new(buf, line, y, styles);
            pen.gap(1);
            if selected {
                pen.text(
                    styles.glyphs.focus,
                    styles.fg(ColorToken::FocusDefault).patch(row),
                );
            } else {
                pen.gap(marker);
            }
            pen.gap(1)
                .cell(
                    repo.name.as_str(),
                    name_width,
                    styles
                        .fg(ColorToken::TextDefault)
                        .patch(row)
                        .add_modifier(Modifier::BOLD),
                )
                .safe(&repo.path, styles.fg(ColorToken::TextMuted).patch(row));
        }
    }
}
