//! PolicyBanner: an action blocked by a policy, or waiting for approval (DSYS-GRP-001 § 3;
//! US-CKP-019, 020, 023). A modal overlay: the caller places it with `modal_area`.

use gitraptor_theme::{ColorToken, SymbolToken};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::Component;
use super::key_hints::KeyHint;
use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, panel, paragraph, wrapped_rows};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyKind {
    Blocked,
    /// Waiting for approval, with a countdown (US-CKP-023).
    Pending,
    /// The approval window closed: no actions left.
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyBannerModel {
    pub kind: PolicyKind,
    pub title: SafeText,
    /// "Rule" and the rule that decided.
    pub rule_label: SafeText,
    pub rule: SafeText,
    pub reason: SafeText,
    /// What is allowed instead (DSYS-GRP-001 § 4: what happened → why → what to do).
    pub alternative: Option<SafeText>,
    /// Time left to decide, as text ("expires in 4:32").
    pub countdown: Option<SafeText>,
    pub actions: Vec<KeyHint>,
}

impl PolicyBannerModel {
    fn look(&self) -> (SymbolToken, ColorToken) {
        match self.kind {
            PolicyKind::Blocked => (SymbolToken::Blocked, ColorToken::StatusDanger),
            PolicyKind::Pending => (SymbolToken::Warning, ColorToken::StatusWarning),
            PolicyKind::Expired => (SymbolToken::Error, ColorToken::TextMuted),
        }
    }
}

impl Component for PolicyBannerModel {
    fn render(&self, area: Rect, buf: &mut Buffer, styles: &Styles) {
        let (sym, token) = self.look();
        let accent = styles.fg(token);
        let inner = panel(buf, area, &self.title, Some(sym), true, accent, styles);
        if inner.height == 0 {
            return;
        }
        let inner = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        let text = styles.fg(ColorToken::TextDefault);
        let muted = styles.fg(ColorToken::TextMuted);

        let mut pen = Pen::new(buf, inner, inner.y, styles);
        pen.safe(&self.rule_label, muted)
            .gap(1)
            .safe(&self.rule, accent.add_modifier(Modifier::BOLD));
        let mut y = paragraph(
            buf,
            inner,
            inner.y + 1,
            0,
            self.reason.as_str(),
            text,
            styles,
        );
        if let Some(alt) = &self.alternative {
            y = paragraph(
                buf,
                inner,
                y,
                0,
                alt.as_str(),
                styles.fg(ColorToken::StatusInfo),
                styles,
            );
        }
        if let Some(countdown) = &self.countdown
            && y < inner.bottom()
        {
            Pen::new(buf, inner, y, styles)
                .symbol(
                    styles.symbol(SymbolToken::Info),
                    styles.fg(ColorToken::StatusInfo),
                )
                .gap(1)
                .safe(countdown, styles.fg(ColorToken::StatusWarning));
        }
        let last = inner.bottom().saturating_sub(1);
        if !self.actions.is_empty() && last > inner.y {
            let mut pen = Pen::new(buf, inner, last, styles);
            for (i, hint) in self.actions.iter().enumerate() {
                if i > 0 {
                    pen.gap(2);
                }
                if !hint.paint(&mut pen, styles) {
                    break;
                }
            }
        }
    }

    fn height(&self, width: u16) -> u16 {
        let w = width.saturating_sub(4);
        let mut rows = 1 + wrapped_rows(self.reason.as_str(), w);
        if let Some(alt) = &self.alternative {
            rows += wrapped_rows(alt.as_str(), w);
        }
        rows += u16::from(self.countdown.is_some());
        if !self.actions.is_empty() {
            rows += 2;
        }
        rows + 2
    }
}
