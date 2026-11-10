//! The full-screen diff view (DS-US-CKP-012, D12): a scrollable list of section headers, file
//! headers and diff lines, from rows that already hold sanitized text. Nothing is named after an
//! agent (D8). Colors are semantic tokens only.

use gitraptor_theme::ColorToken;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Say;
use crate::model::{DiffFailure, SafeText};
use crate::present::diff::{DiffRow, FileHead, SectionKind, clock};
use crate::present::diff_text::DiffText;
use crate::tui::diff::DiffState;
use crate::tui::style::{Pen, Styles, panel};
use crate::tui::widgets::diff_view::paint_line;

/// Paints the view over `area`.
pub(super) fn paint(state: &DiffState, say: Say, styles: &Styles, frame: &mut Frame, area: Rect) {
    let (title, offset) = match state {
        DiffState::Loading { worktree, .. } | DiffState::Failed { worktree, .. } => (
            say.diff(DiffText::TitlePending {
                worktree: worktree.as_str(),
            }),
            0,
        ),
        DiffState::Ready {
            worktree,
            at_utc_ms,
            offset,
            ..
        } => (
            say.diff(DiffText::Title {
                worktree: worktree.as_str(),
                clock: &clock(*at_utc_ms),
            }),
            *offset,
        ),
    };
    let muted = styles.fg(ColorToken::TextMuted);
    let buf = frame.buffer_mut();
    let inner = panel(buf, area, &title, None, true, muted, styles);
    if inner.height == 0 {
        return;
    }
    let message = |text: DiffText<'_>| say.diff(text);
    let state_line = match state {
        DiffState::Loading { .. } => Some(message(DiffText::Loading)),
        DiffState::Failed { failure, .. } => Some(message(match failure {
            DiffFailure::Busy => DiffText::Busy,
            DiffFailure::Unavailable(reason) => DiffText::Unavailable(*reason),
            DiffFailure::Lost => DiffText::Lost,
            // A cancelled answer closes the view; it is never shown.
            DiffFailure::Refused | DiffFailure::Cancelled => DiffText::Refused,
        })),
        DiffState::Ready { .. } => None,
    };
    if let Some(line) = state_line {
        Pen::new(buf, inner, inner.y, styles)
            .gap(1)
            .safe(&line, muted);
        return;
    }
    let DiffState::Ready { lines, .. } = state else {
        return;
    };
    let shown = lines.iter().skip(offset).take(usize::from(inner.height));
    for (y, row) in (inner.y..).zip(shown) {
        match row {
            DiffRow::Section { kind, files } => {
                let text = match kind {
                    SectionKind::Committed => DiffText::Committed(*files),
                    SectionKind::Uncommitted => DiffText::Uncommitted(*files),
                };
                let bold = styles
                    .fg(ColorToken::TextDefault)
                    .add_modifier(Modifier::BOLD);
                Pen::new(buf, inner, y, styles)
                    .gap(1)
                    .safe(&say.diff(text), bold);
            }
            DiffRow::Gap(reason) => {
                let text = say.diff(DiffText::Gap(*reason));
                Pen::new(buf, inner, y, styles).gap(1).safe(&text, muted);
            }
            DiffRow::Cut(cut) => {
                let text = say.diff(DiffText::Cut(*cut));
                Pen::new(buf, inner, y, styles).gap(1).safe(&text, muted);
            }
            DiffRow::File(head) => file_header(buf, inner, y, head, say, styles),
            DiffRow::Line(line) => paint_line(buf, inner, y, line, styles),
        }
    }
}

/// `path  change  +a −r  notice`.
fn file_header(
    buf: &mut ratatui::buffer::Buffer,
    inner: Rect,
    y: u16,
    head: &FileHead,
    say: Say,
    styles: &Styles,
) {
    let g = styles.glyphs;
    let muted = styles.fg(ColorToken::TextMuted);
    let mut pen = Pen::new(buf, inner, y, styles);
    pen.gap(1)
        .safe(&head.path, styles.fg(ColorToken::TextDefault))
        .gap(2)
        .safe(&say.diff(DiffText::Change(head.change)), muted)
        .gap(2)
        .text(
            &format!("{}{}", g.added, head.added),
            styles.fg(ColorToken::GitAdded),
        )
        .gap(1)
        .text(
            &format!("{}{}", g.removed, head.removed),
            styles.fg(ColorToken::GitRemoved),
        );
    if let Some(note) = head.note {
        let note: SafeText = say.diff(DiffText::Note(note));
        pen.gap(2).safe(&note, muted);
    }
}
