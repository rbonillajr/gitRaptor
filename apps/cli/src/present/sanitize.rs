//! The single sanitizer of the TUI (SEC-12, ADR-CKP-003 § 8).
//!
//! Untrusted text never reaches a widget raw: it becomes a [`SafeText`],
//! which can only be built by sanitizing. Control characters are made
//! visible as escapes instead of being emitted or silently dropped, so an
//! OSC 52 sequence or a title change shows and does not act
//! (BR-CKP-VAL-002).

use std::fmt;

use gitraptor_api::display::visible;
use gitraptor_api::untrusted::UntrustedText;

pub use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS};

/// Marks the cut of a capped text (kept for the tests of this file).
#[cfg(test)]
const ELLIPSIS: char = '…';

/// Text that is safe to paint: no C0, C1, DEL, bidi, zero-width, line
/// separator or Tags character survives, and it is capped. Every
/// constructor sanitizes; there is no way to wrap a raw string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafeText(String);

impl SafeText {
    /// Free text or a path, one line.
    pub fn text(raw: &str) -> Self {
        Self(visible(raw, TEXT_MAX_CHARS))
    }

    /// A name, capped at [`NAME_MAX_CHARS`].
    pub fn name(raw: &str) -> Self {
        Self(visible(raw, NAME_MAX_CHARS))
    }

    /// Untrusted text of the contract (N6) as free text.
    pub fn from_untrusted<const MAX: usize>(text: &UntrustedText<MAX>) -> Self {
        Self::text(text.raw())
    }

    /// An untrusted name of the contract (N6).
    pub fn name_from_untrusted<const MAX: usize>(text: &UntrustedText<MAX>) -> Self {
        Self::name(text.raw())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SafeText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::display::must_escape;

    fn clean(text: &str) -> bool {
        !text.chars().any(must_escape)
    }

    #[test]
    fn escapes_are_visible_and_inert() {
        let corpus = [
            (
                "\u{1b}]52;c;cm0gLXJmIH4=\u{7}",
                "\\x1b]52;c;cm0gLXJmIH4=\\x07",
            ),
            ("\u{1b}]0;owned\u{7}", "\\x1b]0;owned\\x07"),
            ("\u{1b}[2J", "\\x1b[2J"),
            ("a\u{9b}31m", "a\\u{9b}31m"),
            ("main\u{202e}txt.exe", "main\\u{202e}txt.exe"),
            ("x\u{061c}y", "x\\u{61c}y"),
            ("one\u{2028}two", "one\\u{2028}two"),
            ("in\u{2063}visible", "in\\u{2063}visible"),
            ("tag\u{e0041}", "tag\\u{e0041}"),
            ("line\nbreak\ttab", "line\\x0abreak\\x09tab"),
            ("del\u{7f}", "del\\x7f"),
        ];
        for (raw, shown) in corpus {
            let safe = SafeText::text(raw);
            assert_eq!(safe.as_str(), shown, "{raw:?}");
            assert!(!safe.as_str().contains('\u{1b}'));
            assert!(clean(safe.as_str()));
        }
    }

    #[test]
    fn plain_text_is_untouched() {
        assert_eq!(SafeText::name("feat/ñandú-⚡").as_str(), "feat/ñandú-⚡");
    }

    #[test]
    fn names_are_capped_at_100_characters() {
        let long = "b".repeat(300);
        let name = SafeText::name(&long);
        assert_eq!(name.as_str().chars().count(), NAME_MAX_CHARS + 1);
        assert!(name.as_str().ends_with(ELLIPSIS));
        let exact = "c".repeat(NAME_MAX_CHARS);
        assert_eq!(SafeText::name(&exact).as_str(), exact);
    }

    #[test]
    fn combining_marks_are_capped_per_character() {
        let zalgo = format!("a{}b", "\u{0301}".repeat(50));
        assert_eq!(SafeText::name(&zalgo).as_str(), "a\u{0301}\u{0301}b");
    }

    /// Fuzzing with a fixed seed: whatever the input, the output has no
    /// character that acts on a terminal (V4).
    #[test]
    fn fuzzed_output_is_always_clean() {
        const POOL: &[char] = &[
            'a',
            'Z',
            '/',
            '-',
            ' ',
            'ñ',
            '⚡',
            '\u{1b}',
            '\u{7}',
            '\u{0}',
            '\n',
            '\r',
            '\t',
            '\u{7f}',
            '\u{80}',
            '\u{9b}',
            '\u{9c}',
            '\u{9d}',
            '\u{9f}',
            '\u{061c}',
            '\u{200b}',
            '\u{200e}',
            '\u{202a}',
            '\u{202e}',
            '\u{2028}',
            '\u{2029}',
            '\u{2060}',
            '\u{2064}',
            '\u{2066}',
            '\u{2069}',
            '\u{feff}',
            '\u{e0000}',
            '\u{e0041}',
            '\u{e007f}',
            '\u{0301}',
            '[',
            ']',
            ';',
            '\\',
        ];
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let len = (next() % 64) as usize;
            let raw: String = (0..len)
                .map(|_| POOL[(next() % POOL.len() as u64) as usize])
                .collect();
            let text = SafeText::text(&raw);
            let name = SafeText::name(&raw);
            assert!(clean(text.as_str()), "{raw:?} -> {text:?}");
            assert!(clean(name.as_str()), "{raw:?} -> {name:?}");
        }
    }
}
