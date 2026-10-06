//! Notification (toast): feedback of an action, with "u: undo" when there is one
//! (DSYS-GRP-001 § 3; US-CKP-014 to 018, 021, 024).
//!
//! The widget is pure: the 5 s expiry and the history belong to the app update (INF-CKP-001).
//! It paints the toasts it is given, newest at the bottom, anchored to the bottom right.

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::Component;
use super::key_hints::KeyHint;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, fill, width};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastModel {
    pub kind: ToastKind,
    pub message: SafeText,
    pub action: Option<KeyHint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastStackModel {
    pub toasts: Vec<ToastModel>,
}

const MAX_WIDTH: u16 = 60;

impl ToastModel {
    fn look(&self) -> (SymbolToken, ColorToken) {
        match self.kind {
            ToastKind::Success => (SymbolToken::Success, ColorToken::StatusSuccess),
            ToastKind::Info => (SymbolToken::Info, ColorToken::StatusInfo),
            ToastKind::Warning => (SymbolToken::Warning, ColorToken::StatusWarning),
            ToastKind::Error => (SymbolToken::Error, ColorToken::StatusDanger),
        }
    }

    fn columns(&self, styles: &Styles) -> u16 {
        let (sym, _) = self.look();
        let action = self.action.as_ref().map_or(0, |a| {
            2 + width(a.keys.as_str()) + 1 + width(a.label.as_str())
        });
        u16::from(styles.symbol(sym).width) + 1 + width(self.message.as_str()) + action + 4
    }

    fn paint(&self, buf: &mut Buffer, area: Rect, styles: &Styles) {
        fill(buf, area, styles);
        let (sym, token) = self.look();
        let accent = styles.fg(token);
        // A one-line card with a rule on the left in the kind's color.
        let rule = Rect::new(area.x, area.y, 1, area.height);
        for y in rule.top()..rule.bottom() {
            buf[(rule.x, y)]
                .set_symbol(styles.glyphs.rule)
                .set_style(accent);
        }
        let line = Rect::new(area.x + 2, area.y, area.width.saturating_sub(3), 1);
        let mut pen = Pen::new(buf, line, area.y, styles);
        pen.symbol(styles.symbol(sym), accent).gap(1);
        let action = self.action.as_ref().map_or(0, |a| {
            2 + width(a.keys.as_str()) + 1 + width(a.label.as_str())
        });
        let room = pen.remaining().saturating_sub(action);
        pen.cell(
            self.message.as_str(),
            room,
            styles.fg(ColorToken::TextDefault),
        );
        if let Some(a) = &self.action {
            pen.gap(2);
            a.paint(&mut pen, styles);
        }
    }
}

impl ToastStackModel {
    /// Where the stack goes inside `area` (the body): bottom right, one row per toast.
    pub fn area(&self, area: Rect, styles: &Styles) -> Rect {
        let w = self
            .toasts
            .iter()
            .map(|t| t.columns(styles))
            .max()
            .unwrap_or(0)
            .min(MAX_WIDTH)
            .min(area.width);
        let h = self.height(w).min(area.height);
        Rect::new(area.right() - w, area.bottom() - h, w, h)
    }
}

impl Component for ToastStackModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let rows = usize::from(area.height);
        let skip = self.toasts.len().saturating_sub(rows);
        for (i, toast) in self.toasts.iter().skip(skip).enumerate() {
            let y = area.y + u16::try_from(i).unwrap_or(0);
            toast.paint(buf, Rect::new(area.x, y, area.width, 1), styles);
        }
    }

    fn height(&self, _width: u16) -> u16 {
        u16::try_from(self.toasts.len()).unwrap_or(u16::MAX)
    }
}
