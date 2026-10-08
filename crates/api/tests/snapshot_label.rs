//! The manual snapshot label as the daemon and `raptor-mcp` validate it (L1 of the security
//! conditions of US-MCP-008): hidden or odd characters are refused, ordinary text in any
//! language is not. Dedicated file: the criterion's test must not live in a production file.

use gitraptor_api::catalog::check_snapshot_label;

/// Characters a terminal or a model reads differently from a person (L1), one per family.
const REFUSED: &[(&str, char)] = &[
    ("soft hyphen", '\u{00ad}'),
    ("combining grapheme joiner", '\u{034f}'),
    ("hangul choseong filler", '\u{115f}'),
    ("hangul jungseong filler", '\u{1160}'),
    ("khmer inherent vowel", '\u{17b4}'),
    ("khmer inherent vowel aa", '\u{17b5}'),
    ("mongolian free variation selector", '\u{180b}'),
    ("mongolian vowel separator", '\u{180e}'),
    ("mongolian free variation selector 4", '\u{180f}'),
    ("hangul filler", '\u{3164}'),
    ("halfwidth hangul filler", '\u{ffa0}'),
    ("variation selector 1", '\u{fe00}'),
    ("variation selector 16", '\u{fe0f}'),
    ("variation selector 17", '\u{e0100}'),
    ("variation selector 256", '\u{e01ef}'),
    ("specials block start", '\u{fff0}'),
    ("specials block end", '\u{fffb}'),
    ("private use (bmp start)", '\u{e000}'),
    ("private use (bmp end)", '\u{f8ff}'),
    ("private use (plane 15)", '\u{f0000}'),
    ("private use (plane 16 end)", '\u{10ffff}'),
    ("non-character fdd0", '\u{fdd0}'),
    ("non-character fdef", '\u{fdef}'),
    ("non-character fffe", '\u{fffe}'),
    ("non-character ffff", '\u{ffff}'),
    ("non-character 1fffe", '\u{1fffe}'),
    ("non-character 10fffe", '\u{10fffe}'),
];

#[test]
fn snapshot_labels_refuse_hidden_and_odd_characters() {
    // Ordinary labels, in several languages, are accepted (so a refusal below is the
    // character's, not the validator refusing everything).
    for ok in [
        "antes de migrar",
        "before the migration",
        "añadí café — listo",
        "リファクタリング前",
        "e\u{0301}\u{0301}\u{0301}\u{0301}", // a base with four combining marks
        "x",
    ] {
        assert!(check_snapshot_label(ok).is_ok(), "{ok:?} must be valid");
    }

    for (what, c) in REFUSED {
        for label in [
            format!("antes{c}de migrar"),
            format!("{c}"),
            format!("a{c}"),
        ] {
            assert!(
                check_snapshot_label(&label).is_err(),
                "{what} (U+{:04X}) must be refused in {label:?}",
                *c as u32
            );
        }
    }

    // A label that starts with a combining mark, or piles more than four of them.
    assert!(check_snapshot_label("\u{0301}antes").is_err());
    assert!(
        check_snapshot_label("\u{20dd}antes").is_err(),
        "enclosing mark first"
    );
    assert!(
        check_snapshot_label("e\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}").is_err(),
        "five combining marks in a row"
    );

    // Only spaces, or spaces at either end.
    for bad in ["   ", " ", " antes", "antes ", "  antes de migrar  "] {
        assert!(check_snapshot_label(bad).is_err(), "{bad:?}");
    }

    // The base rules still hold: empty, over 64 characters, controls.
    assert!(check_snapshot_label("").is_err());
    assert!(check_snapshot_label(&"x".repeat(64)).is_ok());
    assert!(check_snapshot_label(&"x".repeat(65)).is_err());
    assert!(check_snapshot_label("a\u{1b}[31m").is_err());
}
