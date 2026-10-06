//! ConfirmPrompt: confirmation of an irreversible action. It says what is lost and stays on
//! **No** by default (DSYS-GRP-001 § 3; US-CKP-013 to 020, 023, 024).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel, paragraph, width, wrapped_rows};

/// The selected answer. The default is [`Choice::No`]: Enter on a fresh prompt does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Choice {
    #[default]
    No,
    Yes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmModel {
    pub title: SafeText,
    pub question: SafeText,
    /// What is lost, one item per line (files, commits, the worktree…).
    pub losses: Vec<SafeText>,
    /// How to get it back, when a snapshot keeps it (⟲).
    pub recovery: Option<SafeText>,
    /// The verb of the action ("Discard worktree"), never a bare "OK".
    pub yes: SafeText,
    pub no: SafeText,
    pub choice: Choice,
}

impl ConfirmModel {
    /// A prompt on **No**.
    pub fn new(title: SafeText, question: SafeText, yes: SafeText, no: SafeText) -> Self {
        Self {
            title,
            question,
            losses: Vec::new(),
            recovery: None,
            yes,
            no,
            choice: Choice::default(),
        }
    }
}

impl Component for ConfirmModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let danger = styles.fg(ColorToken::StatusDanger);
        let inner = panel(
            buf,
            area,
            &self.title,
            Some(SymbolToken::Warning),
            true,
            danger,
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
        let text = styles.fg(ColorToken::TextDefault);
        let mut y = paragraph(
            buf,
            inner,
            inner.y,
            0,
            self.question.as_str(),
            text.add_modifier(Modifier::BOLD),
            styles,
        );
        for loss in &self.losses {
            if y >= inner.bottom() {
                break;
            }
            Pen::new(buf, inner, y, styles)
                .gap(1)
                .symbol(styles.symbol(SymbolToken::Error), danger)
                .gap(1)
                .safe(loss, text);
            y += 1;
        }
        if let Some(recovery) = &self.recovery
            && y < inner.bottom()
        {
            let ok = styles.fg(ColorToken::StatusSuccess);
            Pen::new(buf, inner, y, styles)
                .gap(1)
                .symbol(styles.symbol(SymbolToken::Undo), ok)
                .gap(1)
                .safe(recovery, ok);
        }

        // Buttons, right-aligned: the selected one has the focus marker and the focus style.
        let last = inner.bottom().saturating_sub(1);
        let button = |label: &SafeText| {
            width(g.focus) + 1 + width(g.button[0]) + width(label.as_str()) + width(g.button[1]) + 2
        };
        let total = button(&self.no) + button(&self.yes);
        let mut pen = Pen::new(buf, inner, last, styles);
        pen.to(inner.right().saturating_sub(total));
        for (choice, label) in [(Choice::No, &self.no), (Choice::Yes, &self.yes)] {
            let selected = self.choice == choice;
            let style = if selected {
                styles
                    .fg(ColorToken::FocusDefault)
                    .add_modifier(Modifier::BOLD)
            } else {
                styles.fg(ColorToken::TextMuted)
            };
            if selected {
                pen.text(g.focus, style).gap(1);
            } else {
                pen.gap(width(g.focus) + 1);
            }
            pen.text(g.button[0], style)
                .safe(label, style)
                .text(g.button[1], style)
                .gap(2);
        }
    }

    fn height(&self, width: u16) -> u16 {
        let w = width.saturating_sub(4);
        let rows = wrapped_rows(self.question.as_str(), w)
            + u16::try_from(self.losses.len()).unwrap_or(u16::MAX)
            + u16::from(self.recovery.is_some())
            + 2;
        rows + 2
    }
}
