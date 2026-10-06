//! Which set to use: the `--theme` override, `GITRAPTOR_THEME`, and otherwise the terminal's
//! background, asked with OSC 11 and with `COLORFGBG` as the fallback (TS-CKP-004, 2026-10-05
//! amendment).
//!
//! Everything here is pure: the bytes to send and how to read the reply. Writing them to the
//! terminal in raw mode belongs to the TUI (`apps/cli`, module `term`), which asks **before** it
//! starts its own event reader, so the reply is not taken for a key press (ADR-CKP-003 § 10 and
//! § 12; the crate stays without dependencies).

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::{Contrast, Rgb, relative_luminance};

/// Environment variable with the same values as `--theme`.
pub const THEME_ENV: &str = "GITRAPTOR_THEME";

/// What to write to the terminal: the OSC 11 background request, then a device-attributes
/// request (DA1). Every terminal answers DA1 and in order, so a DA1 reply without an OSC 11 one
/// before it means the terminal does not support OSC 11, and the wait ends at once
/// ([`reply_complete`]).
pub const OSC11_QUERY: &[u8] = b"\x1b]11;?\x1b\\\x1b[c";

/// How long to wait for [`OSC11_QUERY`] when the terminal answers nothing at all.
pub const QUERY_TIMEOUT: Duration = Duration::from_millis(200);

/// Upper bound of the bytes read while waiting: a well-formed reply is under 64.
pub const QUERY_MAX_REPLY: usize = 1024;

/// The background the normal set is drawn on. The TUI inherits the terminal's ground, so the
/// painted colors have to fit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Background {
    #[default]
    Dark,
    Light,
}

impl Background {
    /// Light when the color contrasts more with black than with white (WCAG relative luminance
    /// above ≈ 0.179).
    pub fn from_rgb(rgb: Rgb) -> Self {
        // (L + 0.05) / 0.05 = 1.05 / (L + 0.05)  ⇔  L = √(1.05 · 0.05) − 0.05
        let threshold = (1.05_f64 * 0.05).sqrt() - 0.05;
        if relative_luminance(rgb) > threshold {
            Background::Light
        } else {
            Background::Dark
        }
    }

    /// From `COLORFGBG` (`"fg;bg"` or `"fg;default;bg"`, rxvt convention): the last field is the
    /// background's ANSI index. 7 and 9..=15 are light; 0..=6 and 8 are dark. `None` when the
    /// value says nothing usable (`default`, out of range, empty).
    pub fn from_colorfgbg(value: &str) -> Option<Self> {
        let bg: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
        match bg {
            7 | 9..=15 => Some(Background::Light),
            0..=6 | 8 => Some(Background::Dark),
            _ => None,
        }
    }
}

/// The value of `--theme` and of [`THEME_ENV`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeChoice {
    /// Detect the background (the default).
    #[default]
    Auto,
    Dark,
    Light,
    HighContrast,
}

impl ThemeChoice {
    /// Accepted spellings, for help texts and errors.
    pub const VALUES: [&'static str; 4] = ["auto", "dark", "light", "high-contrast"];
}

/// A `--theme` or [`THEME_ENV`] value that is none of [`ThemeChoice::VALUES`]. It does not keep
/// the value: it is unchecked input and must not be echoed to the terminal unsanitized (SEC-12,
/// ADR-CKP-003 § 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseThemeChoiceError;

impl fmt::Display for ParseThemeChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown theme (expected one of: {})",
            ThemeChoice::VALUES.join(", ")
        )
    }
}

impl std::error::Error for ParseThemeChoiceError {}

impl FromStr for ThemeChoice {
    type Err = ParseThemeChoiceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(ThemeChoice::Auto),
            "dark" => Ok(ThemeChoice::Dark),
            "light" => Ok(ThemeChoice::Light),
            "high-contrast" => Ok(ThemeChoice::HighContrast),
            _ => Err(ParseThemeChoiceError),
        }
    }
}

/// Where the resolved set came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// `--theme`.
    Flag,
    /// [`THEME_ENV`].
    Env,
    /// The terminal answered OSC 11.
    Osc11,
    /// `COLORFGBG`.
    ColorFgBg,
    /// Nothing answered: dark is assumed.
    Default,
}

/// The outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub contrast: Contrast,
    /// Ignored by the high-contrast set, which paints its own ground.
    pub background: Background,
    pub source: Source,
    /// [`THEME_ENV`] had a value that is not a theme and was ignored, so the TUI can say so
    /// (without repeating the value).
    pub invalid_env: bool,
}

impl Detection {
    fn from_choice(choice: ThemeChoice, source: Source) -> Option<Self> {
        let (contrast, background) = match choice {
            ThemeChoice::Auto => return None,
            ThemeChoice::Dark => (Contrast::Normal, Background::Dark),
            ThemeChoice::Light => (Contrast::Normal, Background::Light),
            ThemeChoice::HighContrast => (Contrast::High, Background::Dark),
        };
        Some(Self {
            contrast,
            background,
            source,
            invalid_env: false,
        })
    }
}

/// Resolves the set: `--theme` (`flag`) > [`THEME_ENV`] > OSC 11 (`query`) > `COLORFGBG` >
/// dark. `COLORFGBG` goes after OSC 11 because it often goes stale through tmux and ssh.
///
/// `no_color` is `--no-color`; `NO_COLOR` and `TERM=dumb` are read from `env`
/// (`std::env::var(name).ok()` in production). `query` asks the terminal for its background
/// ([`OSC11_QUERY`] and [`parse_osc11_reply`]); it is not called when a choice is explicit or
/// when no color is painted, because then the terminal is better left alone.
pub fn resolve(
    flag: Option<ThemeChoice>,
    no_color: bool,
    env: impl Fn(&str) -> Option<String>,
    query: impl FnOnce() -> Option<Rgb>,
) -> Detection {
    if let Some(d) = flag.and_then(|c| Detection::from_choice(c, Source::Flag)) {
        return d;
    }
    let mut invalid_env = false;
    if let Some(value) = env(THEME_ENV).filter(|v| !v.trim().is_empty()) {
        match value.parse::<ThemeChoice>() {
            Ok(choice) => {
                if let Some(d) = Detection::from_choice(choice, Source::Env) {
                    return d;
                }
            }
            Err(ParseThemeChoiceError) => invalid_env = true,
        }
    }
    let colorless = no_color
        || env("NO_COLOR").is_some_and(|v| !v.is_empty())
        || env("TERM").is_some_and(|t| t == "dumb");
    let (background, source) = if let Some(rgb) = (!colorless).then(query).flatten() {
        (Background::from_rgb(rgb), Source::Osc11)
    } else if let Some(bg) = env("COLORFGBG").and_then(|v| Background::from_colorfgbg(&v)) {
        (bg, Source::ColorFgBg)
    } else {
        (Background::Dark, Source::Default)
    };
    Detection {
        contrast: Contrast::Normal,
        background,
        source,
        invalid_env,
    }
}

/// Finds an OSC 11 reply (`ESC ] 11 ; rgb:RRRR/GGGG/BBBB` ended by BEL or `ESC \`) anywhere in
/// `bytes` and returns its color. Each channel has 1 to 4 hex digits and is scaled to 8 bits;
/// `rgba:` (alpha ignored) and `#rrggbb` are accepted too.
pub fn parse_osc11_reply(bytes: &[u8]) -> Option<Rgb> {
    const START: &[u8] = b"\x1b]11;";
    let at = bytes.windows(START.len()).position(|w| w == START)?;
    let body = &bytes[at + START.len()..];
    let end = body.iter().position(|&b| b == 0x07 || b == 0x1b)?;
    let spec = std::str::from_utf8(&body[..end]).ok()?.trim();
    if let Some(hex) = spec.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        return Some(Rgb(channel(0)?, channel(2)?, channel(4)?));
    }
    let channels = spec
        .strip_prefix("rgb:")
        .or_else(|| spec.strip_prefix("rgba:"))?;
    let mut parts = channels.split('/').map(scale_channel);
    let rgb = Rgb(parts.next()??, parts.next()??, parts.next()??);
    Some(rgb)
}

/// `h`, `hh`, `hhh` or `hhhh` hex digits to 0..=255.
fn scale_channel(digits: &str) -> Option<u8> {
    if digits.is_empty() || digits.len() > 4 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    let max = (1u32 << (4 * digits.len())) - 1;
    // Rounded; at most 255 because value ≤ max.
    u8::try_from((value * 255 + max / 2) / max).ok()
}

/// Whether the reply to [`OSC11_QUERY`] is complete: it contains the end of the
/// device-attributes reply (`ESC [ ? … c`), which the terminal sends after answering (or
/// ignoring) the OSC 11 request before it.
pub fn reply_complete(bytes: &[u8]) -> bool {
    bytes
        .windows(3)
        .position(|w| w == b"\x1b[?")
        .is_some_and(|at| bytes[at + 3..].contains(&b'c'))
}

#[cfg(test)]
mod tests;
