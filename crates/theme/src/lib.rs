//! TUI/CLI theme generated from `@gitraptor/tokens` (`packages/design-tokens`, ADR-GRP-003).
//!
//! Widgets ask for a meaning (a [`ColorToken`] or a [`SymbolToken`]) and the [`Theme`] resolves
//! its color, attributes, glyph and width for the active mode. The crate has no dependencies and
//! knows nothing about the TUI library: `apps/cli` maps [`Style`] to its own styles
//! (ADR-CKP-003 § 10). Detecting the mode (flags, `NO_COLOR`, `COLORTERM`, locale) is the TUI's job.

#[rustfmt::skip]
mod generated;

pub use generated::{ColorToken, SymbolToken};

/// An RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The 16 ANSI colors, as the terminal theme defines them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ansi16 {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl Ansi16 {
    /// The SGR palette index, 0..=15.
    pub const fn index(self) -> u8 {
        self as u8
    }
}

/// Text attributes. Without color they carry the meaning (`NO_COLOR`, ADR-CKP-003 § 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attrs(u8);

impl Attrs {
    pub const NONE: Attrs = Attrs(0);
    pub const BOLD: Attrs = Attrs(1);
    pub const DIM: Attrs = Attrs(1 << 1);
    pub const REVERSE: Attrs = Attrs(1 << 2);
    pub const UNDERLINE: Attrs = Attrs(1 << 3);

    pub const fn union(self, other: Attrs) -> Attrs {
        Attrs(self.0 | other.0)
    }

    pub const fn contains(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Whether a token is meant as text or as a background. Advisory: [`Style`] has no role, the
/// widget applies the color where it belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Foreground,
    Background,
}

/// One color at the three depths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Values {
    /// Truecolor value.
    pub rgb: Rgb,
    /// xterm 256-color index, always in `16..=255` (derived at build time unless overridden).
    pub ansi256: u8,
    /// The RGB the xterm palette gives `ansi256`, to measure contrast at that depth.
    pub ansi256_rgb: Rgb,
    /// 16-color fallback.
    pub ansi16: Ansi16,
}

/// Generated data of a semantic color token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ColorSpec {
    pub(crate) role: Role,
    /// In the normal set, keep the terminal default (the values are only a contrast reference).
    pub(crate) inherit: bool,
    pub(crate) no_color: Attrs,
    pub(crate) normal: Values,
    pub(crate) high_contrast: Values,
}

/// Generated data of a symbol token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SymbolSpec {
    pub(crate) glyph: &'static str,
    pub(crate) glyph_width: u8,
    pub(crate) ascii: &'static str,
}

/// Color depth of the terminal, or no color at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorMode {
    TrueColor,
    Ansi256,
    Ansi16,
    /// `NO_COLOR` or `--no-color`: attributes only. Wins over [`Contrast::High`].
    NoColor,
}

/// The semantic set in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Contrast {
    /// Inherits the terminal's text and background.
    Normal,
    /// `--theme high-contrast`: paints its own ground, so nothing is inherited.
    High,
}

/// Unicode glyphs or their ASCII fallback (`--ascii`, non-UTF-8 locale).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolSet {
    Unicode,
    Ascii,
}

/// A resolved color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    Rgb(Rgb),
    /// xterm 256-color index.
    Indexed(u8),
    Ansi16(Ansi16),
}

/// What a widget applies for a token: a color (`None` = terminal default) and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    pub color: Option<Color>,
    pub attrs: Attrs,
}

/// A resolved symbol. The layout reserves `width` columns and pads the text up to it, whatever
/// the terminal actually draws (ADR-CKP-003 § 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Glyph {
    pub text: &'static str,
    pub width: u8,
}

/// The active theme: a color mode, a semantic set and a symbol set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Theme {
    mode: ColorMode,
    contrast: Contrast,
    symbols: SymbolSet,
}

impl Theme {
    pub const fn new(mode: ColorMode, contrast: Contrast, symbols: SymbolSet) -> Self {
        Self {
            mode,
            contrast,
            symbols,
        }
    }

    pub const fn mode(&self) -> ColorMode {
        self.mode
    }

    pub const fn contrast(&self) -> Contrast {
        self.contrast
    }

    pub const fn symbol_set(&self) -> SymbolSet {
        self.symbols
    }

    /// Color and attributes of a semantic token in this theme.
    pub fn style(&self, token: ColorToken) -> Style {
        let spec = token.spec();
        if self.mode == ColorMode::NoColor {
            return Style {
                color: None,
                attrs: spec.no_color,
            };
        }
        let color = (!token.inherits(self.contrast)).then(|| {
            let v = token.values(self.contrast);
            match self.mode {
                ColorMode::TrueColor => Color::Rgb(v.rgb),
                ColorMode::Ansi256 => Color::Indexed(v.ansi256),
                ColorMode::Ansi16 | ColorMode::NoColor => Color::Ansi16(v.ansi16),
            }
        });
        Style {
            color,
            attrs: Attrs::NONE,
        }
    }

    /// Glyph and display width of a symbol in this theme.
    pub fn symbol(&self, symbol: SymbolToken) -> Glyph {
        let spec = symbol.spec();
        match self.symbols {
            SymbolSet::Unicode => Glyph {
                text: spec.glyph,
                width: spec.glyph_width,
            },
            SymbolSet::Ascii => Glyph {
                text: spec.ascii,
                // Fallbacks are short ASCII (checked by tests), so the length fits in a u8.
                width: spec.ascii.len() as u8,
            },
        }
    }
}

impl ColorToken {
    pub const fn role(self) -> Role {
        self.spec().role
    }

    /// The values of the token in a semantic set.
    pub const fn values(self, contrast: Contrast) -> Values {
        match contrast {
            Contrast::Normal => self.spec().normal,
            Contrast::High => self.spec().high_contrast,
        }
    }

    /// Whether the token keeps the terminal default in that set.
    pub const fn inherits(self, contrast: Contrast) -> bool {
        matches!(contrast, Contrast::Normal) && self.spec().inherit
    }

    /// Attributes used without color.
    pub const fn no_color_attrs(self) -> Attrs {
        self.spec().no_color
    }
}

/// The categorical agent colors, `agent.1` … `agent.8`.
pub const AGENT_COLORS: [ColorToken; 8] = [
    ColorToken::Agent1,
    ColorToken::Agent2,
    ColorToken::Agent3,
    ColorToken::Agent4,
    ColorToken::Agent5,
    ColorToken::Agent6,
    ColorToken::Agent7,
    ColorToken::Agent8,
];

/// Color of the agent at a 0-based position. From the ninth agent on the colors repeat, and the
/// name and the symbol tell agents apart (BR-CKP-EDGE-006).
pub const fn agent_color(index: usize) -> ColorToken {
    AGENT_COLORS[index % AGENT_COLORS.len()]
}

/// Whether the agent at a 0-based position shares its color with an earlier one.
pub const fn is_agent_color_reused(index: usize) -> bool {
    index >= AGENT_COLORS.len()
}

/// WCAG 2.1 contrast ratio between two colors, from 1.0 to 21.0.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// WCAG 2.1 relative luminance.
fn relative_luminance(Rgb(r, g, b): Rgb) -> f64 {
    let lin = |c: u8| {
        let v = f64::from(c) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

#[cfg(test)]
mod tests;
