//! Text that comes from a repo or an agent (SEC-12, ADR-GRP-005 § 5).
//!
//! Paths, branch names, declared agent names and diagnostics may carry
//! terminal escapes (OSC 52 writes the clipboard, OSC 0 the window title,
//! OSC 8 hyperlinks) or prompt-injection text. On the wire the marking is
//! explicit, `{"untrusted": "..."}`, so every client sees it, the MCP server
//! included. A terminal only ever prints [`Untrusted::sanitized`].

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};

/// Upper bound of any untrusted text in the contract, in bytes: paths and
/// free text ([`Untrusted`]).
pub const MAX_UNTRUSTED_BYTES: usize = 4096;

/// Upper bound of untrusted names, in bytes: branches, worktree and agent
/// names ([`UntrustedName`], ADR-CKP-003 § 4 N6).
pub const MAX_UNTRUSTED_NAME_BYTES: usize = 1024;

/// Untrusted paths and free text, bounded at [`MAX_UNTRUSTED_BYTES`].
pub type Untrusted = UntrustedText<MAX_UNTRUSTED_BYTES>;

/// Untrusted names, bounded at [`MAX_UNTRUSTED_NAME_BYTES`].
pub type UntrustedName = UntrustedText<MAX_UNTRUSTED_NAME_BYTES>;

/// Untrusted text bounded at `MAX` bytes, its own type per field class so a
/// widget cannot take it as a plain string (N6). Longer text is cut at `MAX`
/// and marked `truncated`, when built and when decoded, so a client never
/// holds more than the bound; text that was not valid UTF-8 (a path or a
/// ref) is converted lossily and marked `lossy`. The daemon never
/// sanitizes: clients do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UntrustedText<const MAX: usize> {
    untrusted: String,
    #[serde(skip_serializing_if = "is_false")]
    truncated: bool,
    #[serde(skip_serializing_if = "is_false")]
    lossy: bool,
}

/// The wire form, decoded before the bound is applied.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    untrusted: String,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    lossy: bool,
}

impl<'de, const MAX: usize> Deserialize<'de> for UntrustedText<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Wire {
            mut untrusted,
            truncated,
            lossy,
        } = Wire::deserialize(deserializer)?;
        let cut = truncate(&mut untrusted, MAX);
        Ok(Self {
            untrusted,
            truncated: truncated || cut,
            lossy,
        })
    }
}

impl<const MAX: usize> JsonSchema for UntrustedText<MAX> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("Untrusted{MAX}"))
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "properties": {
                "untrusted": { "type": "string", "maxLength": MAX },
                "truncated": { "type": "boolean" },
                "lossy": { "type": "boolean" }
            },
            "required": ["untrusted"],
            "additionalProperties": false
        })
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl<const MAX: usize> UntrustedText<MAX> {
    /// The bound of this class, in bytes.
    pub const MAX_BYTES: usize = MAX;

    pub fn new(text: impl Into<String>) -> Self {
        let mut text = text.into();
        let truncated = truncate(&mut text, MAX);
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

    /// The same text as another class (cut to its bound if longer).
    pub fn into_class<const OTHER: usize>(self) -> UntrustedText<OTHER> {
        let mut text = self.untrusted;
        let cut = truncate(&mut text, OTHER);
        UntrustedText {
            untrusted: text,
            truncated: self.truncated || cut,
            lossy: self.lossy,
        }
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

    /// The same text cut to `max` characters.
    pub fn capped_chars(&self, max: usize) -> Self {
        let mut text = self.untrusted.clone();
        let cut = match text.char_indices().nth(max) {
            Some((end, _)) => {
                text.truncate(end);
                true
            }
            None => false,
        };
        Self {
            untrusted: text,
            truncated: self.truncated || cut,
            lossy: self.lossy,
        }
    }

    /// The text as a name in an MCP response: at most
    /// [`MAX_MCP_NAME_CHARS`](crate::mcp_view::MAX_MCP_NAME_CHARS)
    /// characters (ADR-MCP-001 § 6). Escaping is [`crate::mcp_view::for_mcp`].
    pub fn mcp_name(&self) -> Self {
        self.capped_chars(crate::mcp_view::MAX_MCP_NAME_CHARS)
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

/// Bidi overrides and isolates, marks, zero-width and other format characters
/// (Unicode category Cf) and the line and paragraph separators (Zl, Zp): they
/// make a terminal show text different from what it is, or break a line where
/// the reader does not expect it (M-05). Also the variation selectors, the
/// combining grapheme joiner and the Hangul fillers: invisible, they can
/// carry hidden bytes to a model (US-MCP-005).
fn is_invisible_control(c: char) -> bool {
    matches!(
        c,
        '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{34f}'
            | '\u{115f}'
            | '\u{1160}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{ffa0}'
            | '\u{e0100}'..='\u{e01ef}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
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
        let mcp = text.mcp_name();
        assert_eq!(
            mcp.raw().chars().count(),
            crate::mcp_view::MAX_MCP_NAME_CHARS
        );
        assert!(mcp.raw().chars().all(|c| c == 'é'));
        assert!(
            !UntrustedName::new("é".repeat(100))
                .mcp_name()
                .is_truncated()
        );
        assert!(
            serde_json::to_string(&mcp)
                .unwrap()
                .contains(r#""truncated":true"#)
        );
        assert!(!Untrusted::new("short").is_truncated());
    }

    /// N6: the decoder applies each field class's bound, so a client never
    /// holds more than it, whatever the daemon sent.
    #[test]
    fn decoder_enforces_the_field_bound() {
        let long = "a".repeat(MAX_UNTRUSTED_BYTES + 10);
        let wire = serde_json::json!({ "untrusted": long });
        let name: UntrustedName = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(name.raw().len(), MAX_UNTRUSTED_NAME_BYTES);
        assert!(name.is_truncated());
        let path: Untrusted = serde_json::from_value(wire).unwrap();
        assert_eq!(path.raw().len(), MAX_UNTRUSTED_BYTES);
        assert!(path.is_truncated());
        let short: UntrustedName =
            serde_json::from_value(serde_json::json!({ "untrusted": "main" })).unwrap();
        assert!(!short.is_truncated());
        assert_eq!(
            serde_json::to_string(&short).unwrap(),
            r#"{"untrusted":"main"}"#
        );
        assert!(UntrustedName::new("x".repeat(2000)).is_truncated());
    }

    /// N6: each class declares its bound in the schema.
    #[test]
    fn schema_declares_each_bound() {
        let name = serde_json::to_value(schemars::schema_for!(UntrustedName)).unwrap();
        assert_eq!(
            name["properties"]["untrusted"]["maxLength"],
            MAX_UNTRUSTED_NAME_BYTES
        );
        let path = serde_json::to_value(schemars::schema_for!(Untrusted)).unwrap();
        assert_eq!(
            path["properties"]["untrusted"]["maxLength"],
            MAX_UNTRUSTED_BYTES
        );
        assert_eq!(name["additionalProperties"], false);
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

    #[test]
    fn line_separators_and_format_characters_are_neutralized() {
        // M-05 (US-GRD-001): Zl, Zp and Cf never reach a terminal.
        let clean = sanitize("a\u{2028}b\u{2029}c\u{61c}d\u{ad}e\u{e0041}f");
        assert_eq!(clean, "a\u{FFFD}b\u{FFFD}c\u{FFFD}d\u{FFFD}e\u{FFFD}f");
        assert_eq!(sanitize("caf\u{e9} \u{4e2d}"), "caf\u{e9} \u{4e2d}");
        // US-MCP-005: variation selectors, the grapheme joiner and the Hangul
        // fillers hide bytes inside visible text.
        let hidden = sanitize("a\u{fe0f}b\u{e0100}c\u{34f}d\u{3164}e\u{ffa0}f\u{115f}g");
        assert_eq!(hidden.chars().filter(|&c| c == '\u{FFFD}').count(), 6);
    }
}
