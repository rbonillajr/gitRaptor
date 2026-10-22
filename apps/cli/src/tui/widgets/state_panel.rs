//! StatePanel: what happened and what to do, when a state takes the place of the data it would
//! otherwise show (engine without Git, no engine, no repos). Neutral on purpose: it says
//! nothing about policies and it does not go away on its own, unlike the banner and the toast
//! (DSYS-GRP-001 § 3).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel, wrap};

/// How serious the state is. Without color, the symbol and the title still say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTone {
    Info,
    Warning,
    Error,
}

impl PanelTone {
    fn tokens(self) -> (SymbolToken, ColorToken) {
        match self {
            Self::Info => (SymbolToken::Info, ColorToken::StatusInfo),
            Self::Warning => (SymbolToken::Warning, ColorToken::StatusWarning),
            Self::Error => (SymbolToken::Error, ColorToken::StatusDanger),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatePanelModel {
    pub tone: PanelTone,
    pub title: SafeText,
    /// "What happened → what to do", with the command, as one catalog sentence.
    pub message: SafeText,
}

impl Component for StatePanelModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let (symbol, color) = self.tone.tokens();
        let inner = panel(
            buf,
            area,
            &self.title,
            Some(symbol),
            false,
            styles.fg(color),
            styles,
        );
        if inner.height == 0 || inner.width < 3 {
            return;
        }
        let inner = Rect::new(inner.x + 1, inner.y, inner.width - 2, inner.height);
        let style = styles.fg(ColorToken::TextDefault);
        // Folded first, so the wrap measures what is painted.
        let message = styles.glyphs.fold(self.message.as_str());
        let lines = wrap(&message, inner.width);
        let rows = usize::from(inner.height);
        for (i, line) in lines.iter().take(rows).enumerate() {
            let y = inner.y + u16::try_from(i).unwrap_or(0);
            let mut pen = Pen::new(buf, inner, y, styles);
            if i + 1 == rows && lines.len() > rows {
                // Out of height: the rest is cut with the ellipsis of the symbol set.
                let rest = lines[i..].join(" ");
                pen.text(&rest, style);
            } else {
                pen.text(line, style);
            }
        }
    }
}
