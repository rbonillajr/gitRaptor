use super::*;
use std::collections::HashSet;

const ALL_MODES: [ColorMode; 4] = [
    ColorMode::TrueColor,
    ColorMode::Ansi256,
    ColorMode::Ansi16,
    ColorMode::NoColor,
];
/// The three value sets: normal on a dark and on a light terminal, and high contrast.
const ALL_SETS: [(Contrast, Background); 3] = [
    (Contrast::Normal, Background::Dark),
    (Contrast::Normal, Background::Light),
    (Contrast::High, Background::Dark),
];

/// WCAG 2.1 AA for text: gate of the high-contrast set and of the normal sets on typical grounds.
const WCAG_AA: f64 = 4.5;

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
fn every_color_token_has_three_depths_in_every_set() {
    let mut names = HashSet::new();
    for (i, token) in ColorToken::ALL.into_iter().enumerate() {
        assert_eq!(token as usize, i, "{} out of order", token.name());
        assert!(names.insert(token.name()), "{} repeated", token.name());
        for (contrast, background) in ALL_SETS {
            let v = token.values(contrast, background);
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
    for (contrast, background) in ALL_SETS {
        for symbols in [SymbolSet::Unicode, SymbolSet::Ascii] {
            let theme =
                Theme::new(ColorMode::NoColor, contrast, symbols).with_background(background);
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
    for (contrast, background) in ALL_SETS {
        let theme =
            |mode| Theme::new(mode, contrast, SymbolSet::Unicode).with_background(background);
        for token in ColorToken::ALL {
            let v = token.values(contrast, background);
            let expect = |mode| {
                let style = theme(mode).style(token);
                assert!(style.attrs.is_empty());
                style.color
            };
            if token.inherits(contrast) {
                for mode in ALL_MODES {
                    assert_eq!(theme(mode).style(token).color, None);
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
    // ADR-CKP-003 § 10: agnostic of the TUI library. Dev-dependencies (tests) are allowed. The
    // OSC 11 query is pure here; its terminal I/O lives in apps/cli (2026-10-05 amendment).
    let manifest = include_str!("../Cargo.toml");
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line.ends_with("dependencies]") && !line.contains("dev-dependencies");
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
        let (f, b) = (
            fg.values(Contrast::High, Background::Dark),
            bg.values(Contrast::High, Background::Dark),
        );
        for (depth, ratio) in [
            ("truecolor", contrast_ratio(f.rgb, b.rgb)),
            ("256", contrast_ratio(f.ansi256_rgb, b.ansi256_rgb)),
        ] {
            if ratio < WCAG_AA {
                failures.push(format!(
                    "{} on {} ({depth}): {ratio:.2}",
                    fg.name(),
                    bg.name()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "below {WCAG_AA}:1: {failures:#?}");
}

/// Typical terminal grounds the normal sets are measured on (TS-CKP-004, 2026-10-05 amendment):
/// the TUI inherits the ground, so `bg.default` alone says nothing.
const DARK_GROUNDS: [(&str, Rgb); 2] = [
    ("#1e1e1e", Rgb(0x1e, 0x1e, 0x1e)),
    ("Solarized dark", Rgb(0x00, 0x2b, 0x36)),
];
const LIGHT_GROUNDS: [(&str, Rgb); 1] = [("white", Rgb(0xff, 0xff, 0xff))];
/// Only reported: a common light ground the values were not designed against.
const LIGHT_GROUNDS_REPORTED: [(&str, Rgb); 1] = [("Solarized light", Rgb(0xfd, 0xf6, 0xe3))];

fn grounds(background: Background) -> &'static [(&'static str, Rgb)] {
    match background {
        Background::Dark => &DARK_GROUNDS,
        Background::Light => &LIGHT_GROUNDS,
    }
}

/// What the normal-set gate measures, as `(fg, ground name, fg rgb, ground rgb)` at a depth.
/// - `text.default` and `text.muted` on each typical ground of the variant;
/// - `text.default` on `bg.selected` and `bg.highlight`;
/// - on a light terminal, also every other foreground on white (designed for 4.5:1) and
///   `text.inverse` on the colors it is printed over.
fn normal_gate_pairs(background: Background, depth: Depth) -> Vec<(String, Rgb, Rgb)> {
    let v = |t: ColorToken| depth(t.values(Contrast::Normal, background));
    let mut pairs = Vec::new();
    for (name, ground) in grounds(background) {
        for fg in [ColorToken::TextDefault, ColorToken::TextMuted] {
            pairs.push((format!("{} on {name}", fg.name()), v(fg), *ground));
        }
    }
    for ground in [ColorToken::BgSelected, ColorToken::BgHighlight] {
        pairs.push((
            format!("text.default on {}", ground.name()),
            v(ColorToken::TextDefault),
            v(ground),
        ));
    }
    if background == Background::Light {
        let (name, white) = LIGHT_GROUNDS[0];
        for fg in ColorToken::ALL
            .into_iter()
            .filter(|t| t.role() == Role::Foreground && !matches!(t, ColorToken::TextInverse))
        {
            pairs.push((format!("{} on {name}", fg.name()), v(fg), white));
        }
        for ground in INVERSE_GROUNDS {
            pairs.push((
                format!("text.inverse on {}", ground.name()),
                v(ColorToken::TextInverse),
                v(ground),
            ));
        }
    }
    pairs
}

const INVERSE_GROUNDS: [ColorToken; 6] = [
    ColorToken::AccentDefault,
    ColorToken::FocusDefault,
    ColorToken::StatusSuccess,
    ColorToken::StatusWarning,
    ColorToken::StatusDanger,
    ColorToken::StatusInfo,
];

/// Picks the RGB measured at a depth.
type Depth = fn(Values) -> Rgb;

const DEPTHS: [(&str, Depth); 2] = [("truecolor", |v| v.rgb), ("256", |v| v.ansi256_rgb)];

#[test]
fn normal_sets_meet_wcag_aa_on_typical_grounds() {
    let mut failures = Vec::new();
    for background in [Background::Dark, Background::Light] {
        for (depth, pick) in DEPTHS {
            for (what, fg, bg) in normal_gate_pairs(background, pick) {
                let ratio = contrast_ratio(fg, bg);
                if ratio < WCAG_AA {
                    failures.push(format!("{background:?} {what} ({depth}): {ratio:.2}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "below {WCAG_AA}:1: {failures:#?}");
}

#[test]
fn status_and_git_colors_keep_distinct_256_indices_in_each_variant() {
    // Tokens on different primitives must not collapse into one 256-color index; agents may
    // (they are told apart by name and symbol), and so may tokens sharing a primitive.
    const TOKENS: [ColorToken; 9] = [
        ColorToken::StatusSuccess,
        ColorToken::StatusWarning,
        ColorToken::StatusDanger,
        ColorToken::StatusInfo,
        ColorToken::GitAdded,
        ColorToken::GitRemoved,
        ColorToken::GitModified,
        ColorToken::GitConflict,
        ColorToken::GitBranchBase,
    ];
    for background in [Background::Dark, Background::Light] {
        let v = |t: ColorToken| t.values(Contrast::Normal, background);
        for (i, a) in TOKENS.iter().enumerate() {
            for b in &TOKENS[i + 1..] {
                if v(*a).rgb != v(*b).rgb {
                    assert_ne!(
                        v(*a).ansi256,
                        v(*b).ansi256,
                        "{background:?}: {} and {} share a 256 index",
                        a.name(),
                        b.name()
                    );
                }
            }
        }
    }
}

#[test]
fn light_values_differ_from_dark_where_the_brief_found_them_unreadable() {
    // The yellow focus and agent.4 were unreadable on a light terminal.
    let white = Rgb(255, 255, 255);
    for token in [ColorToken::FocusDefault, ColorToken::Agent4] {
        let dark = token.values(Contrast::Normal, Background::Dark).rgb;
        let light = token.values(Contrast::Normal, Background::Light).rgb;
        assert!(contrast_ratio(dark, white) < 2.0, "{}", token.name());
        assert!(contrast_ratio(light, white) >= WCAG_AA, "{}", token.name());
    }
    // Option A, decided by Rene Bonilla (2026-10-05).
    let rgb = |t: ColorToken, b| t.values(Contrast::Normal, b).rgb;
    assert_eq!(
        rgb(ColorToken::AccentDefault, Background::Dark),
        Rgb(0x2d, 0xc2, 0xad)
    );
    assert_eq!(
        rgb(ColorToken::AccentDefault, Background::Light),
        Rgb(0x0f, 0x7d, 0x70)
    );
    assert_eq!(
        rgb(ColorToken::FocusDefault, Background::Light),
        Rgb(0x8a, 0x5a, 0x00)
    );
    assert_eq!(
        rgb(ColorToken::BgSelected, Background::Dark),
        Rgb(0x30, 0x30, 0x30)
    );
    assert_eq!(
        rgb(ColorToken::BgSelected, Background::Light),
        Rgb(0xe8, 0xe8, 0xe8)
    );
    assert_eq!(
        rgb(ColorToken::TextMuted, Background::Light),
        Rgb(0x6b, 0x6b, 0x6b)
    );
}

#[test]
fn a_light_theme_paints_the_light_values_and_high_contrast_ignores_the_background() {
    let light = Theme::new(ColorMode::TrueColor, Contrast::Normal, SymbolSet::Unicode)
        .with_background(Background::Light);
    assert_eq!(light.background(), Background::Light);
    assert_eq!(
        light.style(ColorToken::AccentDefault).color,
        Some(Color::Rgb(Rgb(0x0f, 0x7d, 0x70)))
    );
    // Text and ground are still inherited.
    assert_eq!(light.style(ColorToken::TextDefault).color, None);
    assert_eq!(light.style(ColorToken::BgDefault).color, None);
    for token in ColorToken::ALL {
        let high = Theme::new(ColorMode::TrueColor, Contrast::High, SymbolSet::Unicode);
        assert_eq!(
            high.style(token),
            high.with_background(Background::Light).style(token),
            "{}",
            token.name()
        );
    }
}

/// The contrast report of TS-CKP-004 (no gate beyond the tests above): every foreground of each
/// normal variant on its typical grounds and on its selection, in truecolor and 256, and the
/// 16-color collisions of the agent palette. Run with
/// `cargo test -p gitraptor-theme contrast_report -- --nocapture`.
#[test]
fn contrast_report() {
    let foregrounds: Vec<_> = ColorToken::ALL
        .into_iter()
        .filter(|t| t.role() == Role::Foreground && *t != ColorToken::TextInverse)
        .collect();
    for background in [Background::Dark, Background::Light] {
        let v = |t: ColorToken| t.values(Contrast::Normal, background);
        let mut columns: Vec<(String, Values)> = grounds(background)
            .iter()
            .map(|(n, rgb)| ((*n).to_owned(), plain(*rgb)))
            .collect();
        if background == Background::Light {
            columns.extend(
                LIGHT_GROUNDS_REPORTED
                    .iter()
                    .map(|(n, rgb)| (format!("{n} (info)"), plain(*rgb))),
            );
        }
        columns.push(("bg.selected".to_owned(), v(ColorToken::BgSelected)));
        columns.push(("bg.highlight".to_owned(), v(ColorToken::BgHighlight)));
        println!("\n{background:?} terminal, truecolor / 256 (* = below {WCAG_AA}):");
        let header: Vec<_> = columns.iter().map(|(n, _)| format!("{n:>24}")).collect();
        println!("  {:<18}{}", "", header.join(""));
        for fg in &foregrounds {
            let cells: Vec<_> = columns
                .iter()
                .map(|(_, g)| {
                    let (tc, c256) = (
                        contrast_ratio(v(*fg).rgb, g.rgb),
                        contrast_ratio(v(*fg).ansi256_rgb, g.ansi256_rgb),
                    );
                    assert!(tc.is_finite() && c256.is_finite());
                    let flag = |r: f64| if r < WCAG_AA { '*' } else { ' ' };
                    format!(
                        "{:>24}",
                        format!("{tc:.1}{}/{c256:.1}{}", flag(tc), flag(c256))
                    )
                })
                .collect();
            println!("  {:<18}{}", fg.name(), cells.join(""));
        }
        let inverse: Vec<_> = INVERSE_GROUNDS
            .iter()
            .map(|g| {
                format!(
                    "{} {:.1}",
                    g.name(),
                    contrast_ratio(v(ColorToken::TextInverse).rgb, v(*g).rgb)
                )
            })
            .collect();
        println!("  text.inverse on: {}", inverse.join(", "));
        let mut seen: Vec<(Ansi16, Vec<&str>)> = Vec::new();
        for token in AGENT_COLORS {
            let ansi = v(token).ansi16;
            match seen.iter_mut().find(|(a, _)| *a == ansi) {
                Some((_, names)) => names.push(token.name()),
                None => seen.push((ansi, vec![token.name()])),
            }
        }
        println!("  agent colors sharing a 16-color slot (name and symbol tell them apart):");
        for (ansi, names) in seen.iter().filter(|(_, n)| n.len() > 1) {
            println!("    {ansi:?}: {}", names.join(", "));
        }
    }
}

/// A ground given as RGB, measured as is at both depths (it is the terminal's own color).
fn plain(rgb: Rgb) -> Values {
    Values {
        rgb,
        ansi256: 16,
        ansi256_rgb: rgb,
        ansi16: Ansi16::Black,
    }
}
