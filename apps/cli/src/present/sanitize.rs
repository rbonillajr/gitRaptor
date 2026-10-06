//! The single sanitizer of the TUI (SEC-12, ADR-CKP-003 § 8).
//!
//! Untrusted text never reaches a widget raw: it becomes a [`SafeText`],
//! which can only be built by sanitizing. Control characters are made
//! visible as escapes instead of being emitted or silently dropped, so an
//! OSC 52 sequence or a title change shows and does not act
//! (BR-CKP-VAL-002).

use std::fmt;

use gitraptor_api::untrusted::UntrustedText;

/// Cap of names (branches, worktrees, agents, labels), in characters (L-03).
pub const NAME_MAX_CHARS: usize = 100;

/// Cap of free text and paths, in characters.
pub const TEXT_MAX_CHARS: usize = 1024;

/// Combining marks kept after one base character; the rest are dropped so a
/// "zalgo" name cannot paint over its neighbours.
const MAX_COMBINING: usize = 2;

/// Marks the cut of a capped text.
const ELLIPSIS: char = '…';

/// Text that is safe to paint: no C0, C1, DEL, bidi, zero-width, line
/// separator or Tags character survives, and it is capped. Every
/// constructor sanitizes; there is no way to wrap a raw string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafeText(String);

impl SafeText {
    /// Free text or a path, one line.
    pub fn text(raw: &str) -> Self {
        Self(sanitize(raw, TEXT_MAX_CHARS))
    }

    /// A name, capped at [`NAME_MAX_CHARS`].
    pub fn name(raw: &str) -> Self {
        Self(sanitize(raw, NAME_MAX_CHARS))
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

fn sanitize(raw: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_chars * 4));
    let mut kept = 0;
    let mut combining = 0;
    for c in raw.chars() {
        if is_combining(c) {
            combining += 1;
            if combining > MAX_COMBINING {
                continue;
            }
        } else {
            combining = 0;
        }
        if kept == max_chars {
            out.push(ELLIPSIS);
            break;
        }
        kept += 1;
        if must_escape(c) {
            push_escape(&mut out, c);
        } else {
            out.push(c);
        }
    }
    out
}

/// Characters that act on a terminal or show text different from what it
/// is (ADR-CKP-003 § 8, L-03).
pub(crate) fn must_escape(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            // Bidi controls.
            '\u{061c}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                // Zero-width and invisible.
                | '\u{200b}'..='\u{200f}'
                | '\u{2060}'..='\u{2064}'
                | '\u{feff}'
                // Line and paragraph separators in a one-line field.
                | '\u{2028}'
                | '\u{2029}'
                // Tags.
                | '\u{e0000}'..='\u{e007f}'
        )
}

/// `\x1b` for the 7-bit controls and DEL, `\u{9b}` for the rest.
fn push_escape(out: &mut String, c: char) {
    use fmt::Write as _;
    let code = u32::from(c);
    let _ = if code < 0x80 {
        write!(out, "\\x{code:02x}")
    } else {
        write!(out, "\\u{{{code:x}}}")
    };
}

/// Combining marks of the main blocks.
fn is_combining(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036f}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe20}'..='\u{fe2f}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
