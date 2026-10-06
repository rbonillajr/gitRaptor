//! DiffView: the diff of an agent or a commit (DSYS-GRP-001 § 3; US-CKP-012, 016).

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::Component;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Hunk,
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: SafeText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffBody {
    Lines(Vec<DiffLine>),
    /// A binary file: no lines, a message.
    Binary(SafeText),
    /// No changes.
    Empty(SafeText),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffModel {
    /// The file path.
    pub title: SafeText,
    /// Lines added and removed, shown in the header.
    pub stats: Option<(u32, u32)>,
    pub body: DiffBody,
    pub offset: usize,
    pub focused: bool,
}

const NUMBER: u16 = 5;

impl Component for DiffModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let border = styles.fg(ColorToken::TextMuted);
        let inner = panel(buf, area, &self.title, None, self.focused, border, styles);
        if inner.height == 0 {
            return;
        }
        let g = styles.glyphs;
        let muted = styles.fg(ColorToken::TextMuted);
        let added = styles.fg(ColorToken::GitAdded);
        let removed = styles.fg(ColorToken::GitRemoved);
        let mut y = inner.y;
        if let Some((a, r)) = self.stats {
            let mut pen = Pen::new(buf, inner, y, styles);
            pen.gap(1)
                .text(&format!("{}{a}", g.added), added)
                .gap(1)
                .text(&format!("{}{r}", g.removed), removed);
            y += 1;
        }
        let lines = match &self.body {
            DiffBody::Lines(lines) => lines,
            DiffBody::Binary(message) | DiffBody::Empty(message) => {
                if y < inner.bottom() {
                    Pen::new(buf, inner, y, styles)
                        .gap(1)
                        .symbol(
                            styles.symbol(SymbolToken::Info),
                            styles.fg(ColorToken::StatusInfo),
                        )
                        .gap(1)
                        .safe(message, muted);
                }
                return;
            }
        };
        let rows = usize::from(inner.bottom() - y);
        for line in lines.iter().skip(self.offset).take(rows) {
            let mut pen = Pen::new(buf, inner, y, styles);
            let number = |n: Option<u32>| n.map_or_else(String::new, |n| n.to_string());
            if line.kind == DiffLineKind::Hunk {
                pen.gap(NUMBER * 2 + 1)
                    .text(g.rule, muted)
                    .gap(1)
                    .safe(&line.text, styles.fg(ColorToken::StatusInfo));
            } else {
                let (sign, style) = match line.kind {
                    DiffLineKind::Added => (g.added, added),
                    DiffLineKind::Removed => (g.removed, removed),
                    _ => (" ", styles.fg(ColorToken::TextDefault)),
                };
                pen.text(&format!("{:>4} ", number(line.old)), muted)
                    .text(&format!("{:>4} ", number(line.new)), muted)
                    .gap(1)
                    .text(g.rule, muted)
                    .gap(1)
                    .text(sign, style)
                    .safe(&line.text, style);
            }
            y += 1;
        }
    }
}
