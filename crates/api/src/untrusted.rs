//! Text that comes from a repo or an agent (SEC-12, ADR-GRP-005 § 5).
//!
//! Paths, branch names, declared agent names and diagnostics may carry
//! terminal escapes (OSC 52 writes the clipboard, OSC 0 the window title,
//! OSC 8 hyperlinks) or prompt-injection text. On the wire the marking is
//! explicit, `{"untrusted": "..."}`, so every client sees it, the MCP server
//! included. A terminal only ever prints [`Untrusted::sanitized`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Upper bound of any untrusted text in the contract, in bytes.
pub const MAX_UNTRUSTED_BYTES: usize = 4096;

/// Bound of untrusted text in responses for `raptor-mcp` (SEC-12).
pub const MAX_MCP_UNTRUSTED_BYTES: usize = 256;

/// Untrusted text. Longer text is cut at [`MAX_UNTRUSTED_BYTES`] and marked
/// `truncated`; text that was not valid UTF-8 (a path or a ref) is converted
/// lossily and marked `lossy`. The daemon never sanitizes: clients do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Untrusted {
    untrusted: String,
    #[serde(default, skip_serializing_if = "is_false")]
    truncated: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    lossy: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Untrusted {
    pub fn new(text: impl Into<String>) -> Self {
        let mut text = text.into();
        let truncated = truncate(&mut text, MAX_UNTRUSTED_BYTES);
        Self {
            untrusted: text,
            truncated,
            lossy: false,
        }
    }

    /// Text from the OS (a path, a process name) that may not be UTF-8.
    pub fn from_os(text: &std::ffi::OsStr) -> Self {
        let lossy = text.to_str().is_none();
        let mut value = Self::new(text.to_string_lossy());
        value.lossy = lossy;
        value
    }

    /// The same text cut to `max` bytes (at a character boundary).
    pub fn capped(&self, max: usize) -> Self {
        let mut text = self.untrusted.clone();
        let cut = truncate(&mut text, max);
        Self {
            untrusted: text,
            truncated: self.truncated || cut,
            lossy: self.lossy,
        }
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn is_lossy(&self) -> bool {
        self.lossy
    }

    /// The raw text. Never print it to a terminal: use [`Self::sanitized`].
    pub fn raw(&self) -> &str {
        &self.untrusted
    }

    /// The text without escape sequences, control characters or bidi
    /// overrides, safe to print to a terminal.
    pub fn sanitized(&self) -> String {
        sanitize(&self.untrusted)
    }
}

fn truncate(text: &mut String, max: usize) -> bool {
    if text.len() <= max {
        return false;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    true
}

/// Removes ANSI/OSC/DCS escape sequences and replaces any other control
/// character (C0, DEL, C1), bidi control and zero-width character with
/// U+FFFD.
pub fn sanitize(text: &str) -> String {
    const REPLACEMENT: char = '\u{FFFD}';
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // CSI: parameters up to a final byte in 0x40..=0x7E.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC, DCS, SOS, PM, APC: a string up to BEL or ST (ESC \).
                Some(']' | 'P' | 'X' | '^' | '_') => skip_string(&mut chars),
                _ => {}
            },
            // 8-bit CSI and OSC/DCS/SOS/PM/APC introducers.
            '\u{9b}' => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            '\u{9d}' | '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => skip_string(&mut chars),
            c if c.is_control() || is_invisible_control(c) => out.push(REPLACEMENT),
            c => out.push(c),
        }
    }
    out
}

fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(c) = chars.next() {
        match c {
            '\u{07}' | '\u{9c}' => return,
            '\u{1b}' if chars.peek() == Some(&'\\') => {
                chars.next();
                return;
            }
            _ => {}
        }
    }
}

/// Bidi overrides and isolates, marks and zero-width characters: they make
/// a terminal show text different from what it is.
fn is_invisible_control(c: char) -> bool {
    matches!(
        c,
        '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2060}'
            | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_form_is_explicitly_marked() {
        let text = Untrusted::new("feat/x");
        assert_eq!(
            serde_json::to_string(&text).unwrap(),
            r#"{"untrusted":"feat/x"}"#
        );
        assert!(serde_json::from_str::<Untrusted>(r#""feat/x""#).is_err());
        assert!(serde_json::from_str::<Untrusted>(r#"{"untrusted":"a","x":1}"#).is_err());
    }

    #[test]
    fn osc_and_csi_escapes_are_removed() {
        let clipboard = "main\u{1b}]52;c;ZXZpbA==\u{07}";
        assert_eq!(sanitize(clipboard), "main");
        let title = "a\u{1b}]0;pwned\u{1b}\\b";
        assert_eq!(sanitize(title), "ab");
        let link = "\u{1b}]8;;https://evil\u{1b}\\x\u{1b}]8;;\u{1b}\\";
        assert_eq!(sanitize(link), "x");
        assert_eq!(sanitize("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(sanitize("a\u{9b}2Jb"), "ab");
        assert_eq!(sanitize("a\u{9d}0;t\u{9c}b"), "ab");
    }

    #[test]
    fn control_and_bidi_characters_are_replaced() {
        assert_eq!(sanitize("a\nb\tc\u{7f}"), "a\u{FFFD}b\u{FFFD}c\u{FFFD}");
        assert_eq!(sanitize("x\u{202e}y"), "x\u{FFFD}y");
        assert_eq!(sanitize("a\u{200b}b\u{feff}"), "a\u{FFFD}b\u{FFFD}");
        assert_eq!(sanitize("ñandú/日本"), "ñandú/日本");
    }

    #[test]
    fn length_is_bounded_at_a_char_boundary() {
        let long = "é".repeat(MAX_UNTRUSTED_BYTES);
        let text = Untrusted::new(long);
        assert!(text.raw().len() <= MAX_UNTRUSTED_BYTES);
        assert!(text.is_truncated());
        let mcp = text.capped(MAX_MCP_UNTRUSTED_BYTES);
        assert!(mcp.raw().len() <= MAX_MCP_UNTRUSTED_BYTES);
        assert!(mcp.raw().chars().all(|c| c == 'é'));
        assert!(
            serde_json::to_string(&mcp)
                .unwrap()
                .contains(r#""truncated":true"#)
        );
        assert!(!Untrusted::new("short").is_truncated());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_text_is_marked_lossy() {
        use std::os::unix::ffi::OsStrExt;
        let raw = std::ffi::OsStr::from_bytes(b"feat/\xff");
        let text = Untrusted::from_os(raw);
        assert!(text.is_lossy());
        assert_eq!(text.raw(), "feat/\u{FFFD}");
        assert!(!Untrusted::from_os(std::ffi::OsStr::new("ok")).is_lossy());
    }
}
