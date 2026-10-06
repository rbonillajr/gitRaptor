//! From the theme to ratatui: styles, glyphs and the drawing helpers every widget shares
//! (ADR-CKP-003 § 7 and § 10, TS-CKP-005).
//!
//! Widgets never write a color or a glyph literal. Semantic symbols come from the theme
//! ([`SymbolToken`]); structural glyphs (borders, graph lanes, ellipsis, focus marker) come from
//! [`Glyphs`], with an ASCII set picked by the same [`SymbolSet`] (DSYS-GRP-001, 2026-10-05
//! amendment of TS-CKP-005).

use gitraptor_theme::{Ansi16, Attrs, Color, ColorToken, Glyph, SymbolSet, SymbolToken, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color as RColor, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::model::SafeText;

/// Structural glyphs: they draw shapes and carry no meaning of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// Panel border: horizontal, vertical and corners (top-left, top-right, bottom-left,
    /// bottom-right).
    pub border: [&'static str; 6],
    /// Border of the focused panel: a heavier shape, so focus does not rely on color.
    pub border_focused: [&'static str; 6],
    /// Marks the focused row and the selected choice.
    pub focus: &'static str,
    pub ellipsis: &'static str,
    pub separator: &'static str,
    pub ahead: &'static str,
    pub behind: &'static str,
    pub changes: &'static str,
    pub bullet: &'static str,
    /// Graph: trunk, branch-off, last branch-off, edge and commit node.
    pub lane_trunk: &'static str,
    pub lane_fork: &'static str,
    pub lane_last: &'static str,
    pub lane_edge: &'static str,
    pub commit: &'static str,
    /// Diff gutter and other vertical rules.
    pub rule: &'static str,
    /// Diff line signs (diff syntax, the same in both sets).
    pub added: &'static str,
    pub removed: &'static str,
    /// Between the two agents of a conflict.
    pub versus: &'static str,
    /// Around a button and around a reason.
    pub button: [&'static str; 2],
    pub paren: [&'static str; 2],
}

pub const UNICODE_GLYPHS: Glyphs = Glyphs {
    border: ["─", "│", "┌", "┐", "└", "┘"],
    border_focused: ["━", "┃", "┏", "┓", "┗", "┛"],
    focus: "›",
    ellipsis: "…",
    separator: " · ",
    ahead: "↑",
    behind: "↓",
    changes: "~",
    bullet: "•",
    lane_trunk: "│",
    lane_fork: "├",
    lane_last: "└",
    lane_edge: "─",
    commit: "•",
    rule: "│",
    added: "+",
    removed: "-",
    versus: "↔",
    button: ["[", "]"],
    paren: ["(", ")"],
};

pub const ASCII_GLYPHS: Glyphs = Glyphs {
    border: ["-", "|", "+", "+", "+", "+"],
    border_focused: ["=", "#", "#", "#", "#", "#"],
    focus: ">",
    ellipsis: "...",
    separator: " | ",
    ahead: "+",
    behind: "-",
    changes: "~",
    bullet: "-",
    lane_trunk: "|",
    lane_fork: "|",
    lane_last: "`",
    lane_edge: "-",
    commit: "o",
    rule: "|",
    added: "+",
    removed: "-",
    versus: "<->",
    button: ["[", "]"],
    paren: ["(", ")"],
};

/// The active theme mapped to ratatui. Cheap to copy; every widget borrows one.
#[derive(Debug, Clone, Copy)]
pub struct Styles {
    theme: Theme,
    pub glyphs: &'static Glyphs,
}

impl Styles {
    pub fn new(theme: Theme) -> Self {
        let glyphs = match theme.symbol_set() {
            SymbolSet::Unicode => &UNICODE_GLYPHS,
            SymbolSet::Ascii => &ASCII_GLYPHS,
        };
        Self { theme, glyphs }
    }

    /// A token as the text color, with its attributes (the only ones without color).
    pub fn fg(&self, token: ColorToken) -> Style {
        let s = self.theme.style(token);
        let style = Style::default().add_modifier(modifier(s.attrs));
        match s.color {
            Some(c) => style.fg(color(c)),
            None => style,
        }
    }

    /// A token as the background color.
    pub fn bg(&self, token: ColorToken) -> Style {
        let s = self.theme.style(token);
        let style = Style::default().add_modifier(modifier(s.attrs));
        match s.color {
            Some(c) => style.bg(color(c)),
            None => style,
        }
    }

    /// Default text over the default ground. The normal set inherits both from the terminal;
    /// high contrast paints its own.
    pub fn base(&self) -> Style {
        let fg = self.theme.style(ColorToken::TextDefault).color;
        let bg = self.theme.style(ColorToken::BgDefault).color;
        let mut style = Style::default();
        if let Some(c) = fg {
            style = style.fg(color(c));
        }
        if let Some(c) = bg {
            style = style.bg(color(c));
        }
        style
    }

    pub fn symbol(&self, token: SymbolToken) -> Glyph {
        self.theme.symbol(token)
    }

    /// The color of the agent at a 0-based position (reused from the ninth on).
    pub fn agent(&self, index: usize) -> Style {
        self.fg(gitraptor_theme::agent_color(index))
    }
}

fn modifier(attrs: Attrs) -> Modifier {
    let mut m = Modifier::empty();
    if attrs.contains(Attrs::BOLD) {
        m |= Modifier::BOLD;
    }
    if attrs.contains(Attrs::DIM) {
        m |= Modifier::DIM;
    }
    if attrs.contains(Attrs::REVERSE) {
        m |= Modifier::REVERSED;
    }
    if attrs.contains(Attrs::UNDERLINE) {
        m |= Modifier::UNDERLINED;
    }
    m
}

fn color(c: Color) -> RColor {
    match c {
        Color::Rgb(rgb) => RColor::Rgb(rgb.0, rgb.1, rgb.2),
        Color::Indexed(i) => RColor::Indexed(i),
        Color::Ansi16(a) => match a {
            Ansi16::Black => RColor::Black,
            Ansi16::Red => RColor::Red,
            Ansi16::Green => RColor::Green,
            Ansi16::Yellow => RColor::Yellow,
            Ansi16::Blue => RColor::Blue,
            Ansi16::Magenta => RColor::Magenta,
            Ansi16::Cyan => RColor::Cyan,
            Ansi16::White => RColor::Gray,
            Ansi16::BrightBlack => RColor::DarkGray,
            Ansi16::BrightRed => RColor::LightRed,
            Ansi16::BrightGreen => RColor::LightGreen,
            Ansi16::BrightYellow => RColor::LightYellow,
            Ansi16::BrightBlue => RColor::LightBlue,
            Ansi16::BrightMagenta => RColor::LightMagenta,
            Ansi16::BrightCyan => RColor::LightCyan,
            Ansi16::BrightWhite => RColor::White,
        },
    }
}

/// Display width in columns.
pub fn width(text: &str) -> u16 {
    u16::try_from(text.width()).unwrap_or(u16::MAX)
}

/// Writes along one line, left to right, never past `right`.
pub struct Pen<'b> {
    buf: &'b mut Buffer,
    pub x: u16,
    y: u16,
    right: u16,
    glyphs: &'static Glyphs,
}

impl<'b> Pen<'b> {
    /// A pen over the row `y` of `area`.
    pub fn new(buf: &'b mut Buffer, area: Rect, y: u16, styles: &Styles) -> Self {
        Self {
            buf,
            x: area.x,
            y,
            right: area.right(),
            glyphs: styles.glyphs,
        }
    }

    pub fn remaining(&self) -> u16 {
        self.right.saturating_sub(self.x)
    }

    /// Text cut with an ellipsis when it does not fit.
    pub fn text(&mut self, text: &str, style: Style) -> &mut Self {
        let room = self.remaining();
        self.x += put(self.buf, self.x, self.y, room, text, style, self.glyphs);
        self
    }

    pub fn safe(&mut self, text: &SafeText, style: Style) -> &mut Self {
        self.text(text.as_str(), style)
    }

    /// A table cell of `max` columns: the text cut to leave one blank column before the next
    /// cell, then padded to exactly `max`.
    pub fn cell(&mut self, text: &str, max: u16, style: Style) -> &mut Self {
        let room = self.remaining().min(max);
        let used = put(
            self.buf,
            self.x,
            self.y,
            room.saturating_sub(1),
            text,
            style,
            self.glyphs,
        );
        self.x += used;
        self.gap(room - used)
    }

    /// A theme symbol padded to its declared width. Never cut: if it does not fit, nothing is
    /// written.
    pub fn symbol(&mut self, glyph: Glyph, style: Style) -> &mut Self {
        if u16::from(glyph.width) <= self.remaining() {
            self.x += symbol(self.buf, self.x, self.y, glyph, style);
        }
        self
    }

    /// Blank columns painted with `style` (inside a border or a bar, where a plain gap would
    /// leave what was there).
    pub fn space(&mut self, n: u16, style: Style) -> &mut Self {
        let n = n.min(self.remaining());
        for col in self.x..self.x + n {
            self.buf[(col, self.y)].set_symbol(" ").set_style(style);
        }
        self.x += n;
        self
    }

    pub fn gap(&mut self, n: u16) -> &mut Self {
        self.x = self.x.saturating_add(n).min(self.right);
        self
    }

    /// Moves to an absolute column (clamped to the line).
    pub fn to(&mut self, x: u16) -> &mut Self {
        self.x = x.clamp(self.x, self.right);
        self
    }
}

/// Writes `text` in at most `max` columns, ending in an ellipsis when cut. Returns the columns
/// used.
pub fn put(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max: u16,
    text: &str,
    style: Style,
    glyphs: &Glyphs,
) -> u16 {
    if max == 0 {
        return 0;
    }
    let full = width(text);
    if full <= max {
        let end = buf.set_stringn(x, y, text, usize::from(max), style).0;
        return end.saturating_sub(x);
    }
    let ell = width(glyphs.ellipsis);
    if max <= ell {
        let end = buf.set_stringn(x, y, text, usize::from(max), style).0;
        return end.saturating_sub(x);
    }
    let mut cut = String::new();
    let mut used = 0u16;
    for c in text.chars() {
        let w = width(c.encode_utf8(&mut [0; 4]));
        if used + w > max - ell {
            break;
        }
        used += w;
        cut.push(c);
    }
    cut.push_str(glyphs.ellipsis);
    let end = buf.set_stringn(x, y, &cut, usize::from(max), style).0;
    end.saturating_sub(x)
}

/// Writes a symbol and pads it to the width the theme declares (ADR-CKP-003 § 7).
pub fn symbol(buf: &mut Buffer, x: u16, y: u16, glyph: Glyph, style: Style) -> u16 {
    let declared = u16::from(glyph.width);
    let end = buf
        .set_stringn(x, y, glyph.text, usize::from(declared), style)
        .0;
    for col in end..x + declared {
        if col < buf.area.right() {
            buf[(col, y)].set_symbol(" ").set_style(style);
        }
    }
    declared
}

/// Paints the ground of `area`.
pub fn fill(buf: &mut Buffer, area: Rect, styles: &Styles) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_style(styles.base());
        }
    }
}

/// A bordered panel with a title. Focus shows as a heavier border and the focus marker before
/// the title, so it never depends on color alone. Returns the inner area.
pub fn panel(
    buf: &mut Buffer,
    area: Rect,
    title: &SafeText,
    symbol: Option<SymbolToken>,
    focused: bool,
    border: Style,
    styles: &Styles,
) -> Rect {
    fill(buf, area, styles);
    if area.width < 2 || area.height < 2 {
        return Rect::default();
    }
    let g = styles.glyphs;
    let [h, v, tl, tr, bl, br] = if focused { g.border_focused } else { g.border };
    let border = if focused {
        styles.fg(ColorToken::FocusDefault)
    } else {
        border
    };
    let (left, right, top, bottom) = (area.x, area.right() - 1, area.y, area.bottom() - 1);
    for x in left + 1..right {
        buf[(x, top)].set_symbol(h).set_style(border);
        buf[(x, bottom)].set_symbol(h).set_style(border);
    }
    for y in top + 1..bottom {
        buf[(left, y)].set_symbol(v).set_style(border);
        buf[(right, y)].set_symbol(v).set_style(border);
    }
    buf[(left, top)].set_symbol(tl).set_style(border);
    buf[(right, top)].set_symbol(tr).set_style(border);
    buf[(left, bottom)].set_symbol(bl).set_style(border);
    buf[(right, bottom)].set_symbol(br).set_style(border);

    let title_area = Rect::new(left + 1, top, area.width.saturating_sub(2), 1);
    let mut pen = Pen::new(buf, title_area, top, styles);
    pen.gap(1).space(1, border);
    if focused {
        pen.text(g.focus, border).space(1, border);
    }
    if let Some(token) = symbol {
        pen.symbol(styles.symbol(token), border).space(1, border);
    }
    pen.safe(title, border.add_modifier(Modifier::BOLD))
        .space(1, border);
    Rect::new(
        left + 1,
        top + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

/// Splits `text` into lines of at most `max` columns, at spaces when it can.
pub fn wrap(text: &str, max: u16) -> Vec<String> {
    let max = max.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if width(&candidate) <= max {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        // A word longer than a line is split by columns.
        let mut chunk = String::new();
        for c in word.chars() {
            let mut tmp = [0; 4];
            if width(&chunk) + width(c.encode_utf8(&mut tmp)) > max {
                lines.push(std::mem::take(&mut chunk));
            }
            chunk.push(c);
        }
        line = chunk;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Writes `text` wrapped inside `area` from row `y`, with a hanging `indent`. Returns the row
/// after the last one written.
pub fn paragraph(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    indent: u16,
    text: &str,
    style: Style,
    styles: &Styles,
) -> u16 {
    let mut y = y;
    for line in wrap(text, area.width.saturating_sub(indent)) {
        if y >= area.bottom() {
            break;
        }
        Pen::new(buf, area, y, styles)
            .gap(indent)
            .text(&line, style);
        y += 1;
    }
    y
}

/// Rows `text` takes wrapped at `max` columns.
pub fn wrapped_rows(text: &str, max: u16) -> u16 {
    u16::try_from(wrap(text, max).len()).unwrap_or(u16::MAX)
}

/// Scroll offset that keeps `selected` visible, moving as little as possible from `prev`
/// (ADR-CKP-003: the offset lives in the model so the view does not jump).
pub fn follow(prev: usize, selected: usize, height: usize) -> usize {
    if height == 0 {
        return selected;
    }
    if selected < prev {
        selected
    } else if selected >= prev + height {
        selected + 1 - height
    } else {
        prev
    }
}

/// The area of a modal overlay (ConfirmPrompt, PolicyBanner, help): centered, at most
/// `width` × `height`, never outside `area`.
pub fn modal_area(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_theme::{ColorMode, Contrast};

    fn styles(mode: ColorMode, symbols: SymbolSet) -> Styles {
        Styles::new(Theme::new(mode, Contrast::Normal, symbols))
    }

    #[test]
    fn follow_scrolls_only_when_the_selection_leaves_the_view() {
        assert_eq!(follow(0, 3, 5), 0);
        assert_eq!(follow(0, 5, 5), 1);
        assert_eq!(follow(4, 2, 5), 2);
        assert_eq!(follow(2, 4, 5), 2);
        assert_eq!(follow(7, 0, 0), 0);
    }

    #[test]
    fn wrap_breaks_at_spaces_and_splits_long_words() {
        assert_eq!(
            wrap("rebase claude-1 onto main", 12),
            ["rebase", "claude-1", "onto main"]
        );
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 4), [""]);
    }

    #[test]
    fn modal_area_is_centered_and_clipped() {
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(modal_area(area, 40, 10), Rect::new(20, 7, 40, 10));
        assert_eq!(modal_area(area, 100, 30), area);
    }

    #[test]
    fn put_cuts_with_the_ellipsis_of_the_symbol_set() {
        let s = styles(ColorMode::NoColor, SymbolSet::Ascii);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let used = put(
            &mut buf,
            0,
            0,
            8,
            "feature/long-name",
            Style::default(),
            s.glyphs,
        );
        assert_eq!(used, 8);
        let line: String = (0..10).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(line, "featu...  ");
    }

    #[test]
    fn symbols_take_their_declared_width() {
        let s = styles(ColorMode::TrueColor, SymbolSet::Unicode);
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        let glyph = s.symbol(SymbolToken::Warning);
        assert_eq!(symbol(&mut buf, 0, 0, glyph, Style::default()), 2);
        assert_eq!(buf[(0, 0)].symbol(), "⚠");
        assert_eq!(buf[(1, 0)].symbol(), " ");
    }

    #[test]
    fn no_color_carries_meaning_with_attributes_only() {
        let s = styles(ColorMode::NoColor, SymbolSet::Unicode);
        let focus = s.fg(ColorToken::FocusDefault);
        assert_eq!(focus.fg, None);
        assert!(
            focus
                .add_modifier
                .contains(Modifier::BOLD | Modifier::REVERSED)
        );
    }
}
