//! Text that is safe to paint in a terminal (SEC-12, ADR-CKP-003 § 8).
//!
//! Widgets only take [`SafeText`]: a widget that would paint untrusted text does not compile.
//! This type only has the constructor for text compiled into the binary. The privileged
//! constructor that `present::sanitize` and the typed catalog need belongs to INF-CKP-001,
//! which may move the type to `present` and re-export it from here (TS-CKP-005, Dev Spec § 3).

use std::borrow::Cow;

/// Text already free of control, bidi and invisible characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafeText(Cow<'static, str>);

impl SafeText {
    /// Text compiled into the binary: catalog literals, the gallery and tests. Only those may
    /// call it (checked by `trusted_is_confined` in the widget tests).
    ///
    /// # Panics
    ///
    /// When the literal has a character the sanitizer would escape, so a bad literal fails in
    /// the first test that paints it.
    pub fn trusted(text: &'static str) -> Self {
        assert!(
            text.chars().all(is_paintable),
            "SafeText::trusted with a non-paintable character: {text:?}"
        );
        Self(Cow::Borrowed(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A character the sanitizer lets through unchanged (ADR-CKP-003 § 8 categories).
fn is_paintable(c: char) -> bool {
    let cp = c as u32;
    !(c.is_control()
        || cp == 0x061C
        || (0x200B..=0x200F).contains(&cp)
        || (0x2028..=0x202E).contains(&cp)
        || (0x2060..=0x2069).contains(&cp)
        || cp == 0xFEFF
        || (0xE0000..=0xE007F).contains(&cp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_keeps_plain_text() {
        assert_eq!(SafeText::trusted("main · ⚡").as_str(), "main · ⚡");
    }

    #[test]
    #[should_panic(expected = "non-paintable")]
    fn trusted_rejects_escape_sequences() {
        SafeText::trusted("\u{1b}]52;c;payload\u{7}");
    }

    #[test]
    #[should_panic(expected = "non-paintable")]
    fn trusted_rejects_bidi_controls() {
        SafeText::trusted("main\u{202E}");
    }
}
