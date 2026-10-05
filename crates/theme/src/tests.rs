use super::*;
use std::collections::HashSet;

const ALL_MODES: [ColorMode; 4] = [
    ColorMode::TrueColor,
    ColorMode::Ansi256,
    ColorMode::Ansi16,
    ColorMode::NoColor,
];
const ALL_CONTRASTS: [Contrast; 2] = [Contrast::Normal, Contrast::High];

/// WCAG 2.1 AA for text. Gate of the high-contrast set until DSYS-GRP-001 § 8 sets another.
const MIN_HIGH_CONTRAST: f64 = 4.5;

fn xterm_rgb(index: u8) -> Rgb {
    const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
    if index >= 232 {
        let v = 8 + 10 * (index - 232);
        return Rgb(v, v, v);
    }
    let i = usize::from(index - 16);
    Rgb(CUBE[i / 36], CUBE[(i / 6) % 6], CUBE[i % 6])
}

#[test]
fn every_color_token_has_three_depths_in_both_sets() {
    let mut names = HashSet::new();
    for (i, token) in ColorToken::ALL.into_iter().enumerate() {
        assert_eq!(token as usize, i, "{} out of order", token.name());
        assert!(names.insert(token.name()), "{} repeated", token.name());
        for contrast in ALL_CONTRASTS {
            let v = token.values(contrast);
            assert!(
                v.ansi256 >= 16,
                "{}: index {} is user-remapped",
                token.name(),
                v.ansi256
            );
            assert_eq!(v.ansi256_rgb, xterm_rgb(v.ansi256), "{}", token.name());
            assert!(v.ansi16.index() <= 15);
        }
    }
}

#[test]
fn the_token_set_covers_the_design_system() {
    let names: HashSet<_> = ColorToken::ALL.iter().map(|t| t.name()).collect();
    for expected in [
        "text.default",
        "text.muted",
        "text.inverse",
        "bg.default",
        "bg.selected",
        "bg.highlight",
        "accent.default",
        "focus.default",
        "status.success",
        "status.warning",
        "status.danger",
        "status.info",
        "git.added",
        "git.removed",
        "git.modified",
        "git.conflict",
        "git.branch.base",
        "agent.state.active",
        "agent.state.idle",
        "agent.state.done",
    ] {
        assert!(names.contains(expected), "missing {expected}");
    }
    for n in 1..=8 {
        assert!(
            names.contains(format!("agent.{n}").as_str()),
            "missing agent.{n}"
        );
    }
}

#[test]
fn every_symbol_has_glyph_ascii_fallback_and_width() {
    for symbol in SymbolToken::ALL {
        let spec = symbol.spec();
        let name = symbol.name();
        assert!(
            !spec.glyph.is_ascii(),
            "{name}: the glyph is the Unicode form"
        );
        assert!(
            (1..=2).contains(&spec.glyph_width),
            "{name}: width {}",
            spec.glyph_width
        );
        assert!(!spec.ascii.is_empty(), "{name}: empty fallback");
        assert!(
            spec.ascii.bytes().all(|b| b.is_ascii_graphic()),
            "{name}: fallback {:?} is not printable ASCII",
            spec.ascii
        );
    }
}

#[test]
fn declared_symbol_width_is_at_least_the_unicode_width() {
    use unicode_width::UnicodeWidthStr;
    for symbol in SymbolToken::ALL {
        let spec = symbol.spec();
        assert!(
            usize::from(spec.glyph_width) >= spec.glyph.width(),
            "{}: declared {} < unicode-width {}",
            symbol.name(),
            spec.glyph_width,
            spec.glyph.width()
        );
    }
}

#[test]
fn wide_symbols_reserve_two_columns() {
    let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode);
    for symbol in [
        SymbolToken::Conflict,
        SymbolToken::Blocked,
        SymbolToken::Warning,
    ] {
        assert_eq!(theme.symbol(symbol).width, 2, "{}", symbol.name());
    }
}

#[test]
fn ascii_set_returns_the_fallback_and_its_length() {
    let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Ascii);
    for symbol in SymbolToken::ALL {
        let glyph = theme.symbol(symbol);
        assert!(glyph.text.is_ascii(), "{}", symbol.name());
        assert_eq!(
            usize::from(glyph.width),
            glyph.text.len(),
            "{}",
            symbol.name()
        );
    }
    assert_eq!(theme.symbol(SymbolToken::Blocked).text, "[blocked]");
    assert_eq!(theme.symbol(SymbolToken::Conflict).text, "[c]");
}

#[test]
fn no_color_never_yields_a_color_and_keeps_the_meaning_in_attributes() {
    for contrast in ALL_CONTRASTS {
        for symbols in [SymbolSet::Unicode, SymbolSet::Ascii] {
            let theme = Theme::new(ColorMode::NoColor, contrast, symbols);
            for token in ColorToken::ALL {
                let style = theme.style(token);
                assert_eq!(
                    style.color,
                    None,
                    "{} has a color under NO_COLOR",
                    token.name()
                );
                assert_eq!(style.attrs, token.no_color_attrs());
            }
            // ADR-CKP-003 § 10: bold and reverse carry what matters.
            assert!(
                theme
                    .style(ColorToken::StatusDanger)
                    .attrs
                    .contains(Attrs::BOLD)
            );
            assert!(
                theme
                    .style(ColorToken::StatusWarning)
                    .attrs
                    .contains(Attrs::BOLD)
            );
            assert!(
                theme
                    .style(ColorToken::BgSelected)
                    .attrs
                    .contains(Attrs::REVERSE)
            );
            let focus = theme.style(ColorToken::FocusDefault).attrs;
            assert!(focus.contains(Attrs::REVERSE.union(Attrs::BOLD)));
        }
    }
}

#[test]
fn dim_and_underline_only_where_losing_them_loses_no_information() {
    for token in ColorToken::ALL {
        let attrs = token.no_color_attrs();
        if attrs.contains(Attrs::DIM) || attrs.contains(Attrs::UNDERLINE) {
            assert!(
                matches!(
                    token,
                    ColorToken::TextMuted | ColorToken::AgentStateIdle | ColorToken::AgentStateDone
                ),
                "{} relies on dim/underline",
                token.name()
            );
        }
    }
}

#[test]
fn each_mode_yields_its_depth() {
    for contrast in ALL_CONTRASTS {
        for token in ColorToken::ALL {
            let v = token.values(contrast);
            let expect = |mode| {
                let style = Theme::new(mode, contrast, SymbolSet::Unicode).style(token);
                assert!(style.attrs.is_empty());
                style.color
            };
            if token.inherits(contrast) {
                for mode in ALL_MODES {
                    assert_eq!(
                        Theme::new(mode, contrast, SymbolSet::Unicode)
                            .style(token)
                            .color,
                        None
                    );
                }
                continue;
            }
            assert_eq!(expect(ColorMode::TrueColor), Some(Color::Rgb(v.rgb)));
            assert_eq!(expect(ColorMode::Ansi256), Some(Color::Indexed(v.ansi256)));
            assert_eq!(expect(ColorMode::Ansi16), Some(Color::Ansi16(v.ansi16)));
        }
    }
}

#[test]
fn normal_set_inherits_the_terminal_text_and_ground_and_high_contrast_paints_them() {
    for token in [ColorToken::TextDefault, ColorToken::BgDefault] {
        assert!(token.inherits(Contrast::Normal), "{}", token.name());
        assert!(!token.inherits(Contrast::High), "{}", token.name());
    }
    let high = Theme::new(ColorMode::TrueColor, Contrast::High, SymbolSet::Unicode);
    assert_eq!(
        high.style(ColorToken::BgDefault).color,
        Some(Color::Rgb(Rgb(0, 0, 0)))
    );
    assert_eq!(ColorToken::BgDefault.role(), Role::Background);
    assert_eq!(ColorToken::TextDefault.role(), Role::Foreground);
}

#[test]
fn ninth_agent_reuses_the_first_color() {
    let distinct: HashSet<_> = (0..8).map(agent_color).collect();
    assert_eq!(distinct.len(), 8);
    assert!((0..8).all(|i| !is_agent_color_reused(i)));
    assert_eq!(agent_color(8), agent_color(0));
    assert_eq!(agent_color(8), ColorToken::Agent1);
    assert!(is_agent_color_reused(8));
    assert_eq!(agent_color(17), ColorToken::Agent2);
    // The symbol does not depend on the position: name and symbol tell reused colors apart.
    let theme = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode);
    assert_eq!(theme.symbol(SymbolToken::AgentActive).text, "●");
    // agent.state.* are not part of the categorical cycle.
    for i in 0..64 {
        let c = agent_color(i);
        assert!(!matches!(
            c,
            ColorToken::AgentStateActive | ColorToken::AgentStateIdle | ColorToken::AgentStateDone
        ));
    }
}

#[test]
fn crate_has_no_normal_dependencies() {
    // ADR-CKP-003 § 10: agnostic of the TUI library. Dev-dependencies (tests) are allowed.
    let manifest = include_str!("../Cargo.toml");
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        assert!(
            !in_deps || line.is_empty() || line.starts_with('#'),
            "crates/theme must not depend on anything: {line}"
        );
    }
    assert!(!manifest.contains("ratatui") && !manifest.contains("crossterm"));
}

#[test]
fn contrast_ratio_matches_wcag_reference_values() {
    assert!((contrast_ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
    assert!((contrast_ratio(Rgb(10, 20, 30), Rgb(10, 20, 30)) - 1.0).abs() < 1e-9);
    // #777777 on white is the classic 4.48:1.
    assert!((contrast_ratio(Rgb(0x77, 0x77, 0x77), Rgb(255, 255, 255)) - 4.48).abs() < 0.01);
}

/// Pairs measured: every foreground on `bg.default`; normal and muted text on the selected and
/// highlighted grounds; inverse text on the colors it is printed over.
fn contrast_pairs() -> Vec<(ColorToken, ColorToken)> {
    let mut pairs: Vec<_> = ColorToken::ALL
        .into_iter()
        .filter(|t| t.role() == Role::Foreground && *t != ColorToken::TextInverse)
        .map(|t| (t, ColorToken::BgDefault))
        .collect();
    for ground in [ColorToken::BgSelected, ColorToken::BgHighlight] {
        pairs.push((ColorToken::TextDefault, ground));
        pairs.push((ColorToken::TextMuted, ground));
    }
    for ground in [
        ColorToken::AccentDefault,
        ColorToken::FocusDefault,
        ColorToken::StatusSuccess,
        ColorToken::StatusWarning,
        ColorToken::StatusDanger,
        ColorToken::StatusInfo,
    ] {
        pairs.push((ColorToken::TextInverse, ground));
    }
    pairs
}

#[test]
fn high_contrast_meets_wcag_aa_in_truecolor_and_256() {
    let mut failures = Vec::new();
    for (fg, bg) in contrast_pairs() {
        let (f, b) = (fg.values(Contrast::High), bg.values(Contrast::High));
        for (depth, ratio) in [
            ("truecolor", contrast_ratio(f.rgb, b.rgb)),
            ("256", contrast_ratio(f.ansi256_rgb, b.ansi256_rgb)),
        ] {
            if ratio < MIN_HIGH_CONTRAST {
                failures.push(format!(
                    "{} on {} ({depth}): {ratio:.2}",
                    fg.name(),
                    bg.name()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "below {MIN_HIGH_CONTRAST}:1: {failures:#?}"
    );
}

/// Report without gate (TS-CKP-004): the normal set over the assumed dark ground and the 16-color
/// collisions of the agent palette. Run with `--nocapture` to read it.
#[test]
fn report_normal_contrast_and_agent_collisions() {
    println!("normal set, assumed dark ground (DSYS-GRP-001 § 8.2):");
    for (fg, bg) in contrast_pairs() {
        let (f, b) = (fg.values(Contrast::Normal), bg.values(Contrast::Normal));
        let (tc, c256) = (
            contrast_ratio(f.rgb, b.rgb),
            contrast_ratio(f.ansi256_rgb, b.ansi256_rgb),
        );
        assert!(tc.is_finite() && c256.is_finite());
        let flag = if tc < MIN_HIGH_CONTRAST { "  < AA" } else { "" };
        println!(
            "  {:<20} on {:<14} {tc:>5.2} / 256: {c256:>5.2}{flag}",
            fg.name(),
            bg.name()
        );
    }
    let mut seen: Vec<(Ansi16, Vec<&str>)> = Vec::new();
    for token in AGENT_COLORS {
        let ansi = token.values(Contrast::Normal).ansi16;
        match seen.iter_mut().find(|(a, _)| *a == ansi) {
            Some((_, names)) => names.push(token.name()),
            None => seen.push((ansi, vec![token.name()])),
        }
    }
    println!("agent colors sharing a 16-color slot (name and symbol tell them apart):");
    for (ansi, names) in seen.iter().filter(|(_, n)| n.len() > 1) {
        println!("  {ansi:?}: {}", names.join(", "));
    }
}
