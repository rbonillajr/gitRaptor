//! ObservePrompt: "Observe this repo? [y/N]" when the TUI opens inside a repo the engine does
//! not observe (US-CKP-025). Observing is not irreversible (`raptor repo retire` undoes it), so
//! it is a neutral panel with the ℹ symbol and the accent border, not a ConfirmPrompt: the
//! default (No) is in the question itself, as the content guide asks (DSYS-GRP-001 § 3).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel, paragraph};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservePromptModel {
    pub title: SafeText,
    /// Why it is asked ("GitRaptor does not observe the repo of this folder yet.").
    pub why: SafeText,
    pub name: SafeText,
    pub path: SafeText,
    /// The question with its default, or what is happening once answered.
    pub question: SafeText,
}

impl Component for ObservePromptModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let inner = panel(
            buf,
            area,
            &self.title,
            Some(SymbolToken::Info),
            true,
            styles.fg(ColorToken::AccentDefault),
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
        let muted = styles.fg(ColorToken::TextMuted);
        let text = styles.fg(ColorToken::TextDefault);
        let mut y = paragraph(buf, inner, inner.y, 0, self.why.as_str(), muted, styles);
        y += 1;
        if y < inner.bottom() {
            Pen::new(buf, inner, y, styles)
                .safe(&self.name, text.add_modifier(Modifier::BOLD))
                .gap(2)
                .safe(&self.path, muted);
        }
        y += 2;
        paragraph(
            buf,
            inner,
            y,
            0,
            self.question.as_str(),
            text.add_modifier(Modifier::BOLD),
            styles,
        );
    }
}
