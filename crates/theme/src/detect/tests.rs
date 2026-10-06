use super::*;
use std::cell::Cell;
use std::collections::HashMap;

fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| map.get(name).cloned()
}

/// A query that must not run.
fn never() -> Option<Rgb> {
    panic!("the terminal was queried")
}

#[test]
fn parses_the_osc11_replies_terminals_send() {
    let cases: [(&[u8], Rgb); 7] = [
        // xterm, iTerm2, kitty, WezTerm: 4 digits per channel, ST or BEL.
        (b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\", Rgb(255, 255, 255)),
        (b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07", Rgb(0x1e, 0x1e, 0x1e)),
        // Solarized dark from a 2-digit terminal.
        (b"\x1b]11;rgb:00/2b/36\x07", Rgb(0x00, 0x2b, 0x36)),
        // 1 and 3 digits are scaled, not truncated.
        (b"\x1b]11;rgb:f/8/0\x07", Rgb(255, 136, 0)),
        (b"\x1b]11;rgb:fff/000/800\x07", Rgb(255, 0, 128)),
        (
            b"\x1b]11;rgba:fbfb/fbfb/fbfb/ffff\x1b\\",
            Rgb(0xfb, 0xfb, 0xfb),
        ),
        (b"\x1b]11;#fdf6e3\x07", Rgb(0xfd, 0xf6, 0xe3)),
    ];
    for (reply, rgb) in cases {
        assert_eq!(
            parse_osc11_reply(reply),
            Some(rgb),
            "{}",
            String::from_utf8_lossy(reply)
        );
    }
}

#[test]
fn finds_the_reply_among_other_bytes_and_rejects_garbage() {
    // A key typed before the reply, and the DA1 answer after it.
    let mixed = b"j\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b[?62;22c";
    assert_eq!(parse_osc11_reply(mixed), Some(Rgb(255, 255, 255)));
    for bad in [
        &b""[..],
        b"\x1b[?62;22c",                   // only DA1: OSC 11 not supported
        b"\x1b]11;rgb:ffff/ffff/ffff",     // cut before the terminator
        b"\x1b]11;rgb:ffff/ffff\x07",      // two channels
        b"\x1b]11;rgb:fffff/0/0\x07",      // five digits
        b"\x1b]11;rgb:zz/00/00\x07",       // not hex
        b"\x1b]11;?\x07",                  // our own request echoed back
        b"\x1b]10;rgb:ffff/ffff/ffff\x07", // the foreground, not the background
        b"\x1b]11;#fff\x07",               // short form not accepted
    ] {
        assert_eq!(
            parse_osc11_reply(bad),
            None,
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn classifies_the_ground_by_luminance() {
    for light in [
        Rgb(255, 255, 255),
        Rgb(0xfb, 0xfb, 0xfb),
        Rgb(0xfd, 0xf6, 0xe3),
        Rgb(0xee, 0xe8, 0xd5),
    ] {
        assert_eq!(Background::from_rgb(light), Background::Light, "{light:?}");
    }
    for dark in [
        Rgb(0, 0, 0),
        Rgb(0x1e, 0x1e, 0x1e),
        Rgb(0x00, 0x2b, 0x36),
        Rgb(0x28, 0x2c, 0x34),
        Rgb(0x30, 0x0a, 0x24),
    ] {
        assert_eq!(Background::from_rgb(dark), Background::Dark, "{dark:?}");
    }
    // Around the crossover (≈ #767676 is 4.5:1 against both black and white).
    assert_eq!(
        Background::from_rgb(Rgb(0x80, 0x80, 0x80)),
        Background::Light
    );
    assert_eq!(
        Background::from_rgb(Rgb(0x70, 0x70, 0x70)),
        Background::Dark
    );
}

#[test]
fn reads_colorfgbg() {
    assert_eq!(Background::from_colorfgbg("15;0"), Some(Background::Dark));
    assert_eq!(Background::from_colorfgbg("0;15"), Some(Background::Light));
    assert_eq!(Background::from_colorfgbg("0;7"), Some(Background::Light));
    assert_eq!(Background::from_colorfgbg("7;8"), Some(Background::Dark));
    // rxvt's three-field form: the last field is the background.
    assert_eq!(
        Background::from_colorfgbg("0;default;15"),
        Some(Background::Light)
    );
    assert_eq!(
        Background::from_colorfgbg("15;default;0"),
        Some(Background::Dark)
    );
    for nothing in ["", "15;default", "15;16", "15;-1", "light", "15;"] {
        assert_eq!(Background::from_colorfgbg(nothing), None, "{nothing:?}");
    }
}

#[test]
fn parses_the_theme_choice() {
    assert_eq!("light".parse(), Ok(ThemeChoice::Light));
    assert_eq!(" Dark ".parse(), Ok(ThemeChoice::Dark));
    assert_eq!("high-contrast".parse(), Ok(ThemeChoice::HighContrast));
    assert_eq!("AUTO".parse(), Ok(ThemeChoice::Auto));
    for value in ThemeChoice::VALUES {
        assert!(value.parse::<ThemeChoice>().is_ok(), "{value}");
    }
    let err = "\x1b[31msolarized".parse::<ThemeChoice>().unwrap_err();
    assert!(err.to_string().contains("high-contrast"), "{err}");
    // Unchecked input is not echoed back (SEC-12).
    assert!(!err.to_string().contains("solarized"), "{err}");
}

#[test]
fn the_flag_wins_and_the_terminal_is_not_queried() {
    let env = env_of(&[(THEME_ENV, "dark"), ("COLORFGBG", "15;0")]);
    let d = resolve(Some(ThemeChoice::Light), false, &env, never);
    assert_eq!(
        (d.contrast, d.background, d.source),
        (Contrast::Normal, Background::Light, Source::Flag)
    );
    let d = resolve(Some(ThemeChoice::HighContrast), false, &env, never);
    assert_eq!((d.contrast, d.source), (Contrast::High, Source::Flag));
    let d = resolve(Some(ThemeChoice::Dark), false, env_of(&[]), never);
    assert_eq!((d.background, d.source), (Background::Dark, Source::Flag));
}

#[test]
fn the_variable_wins_over_detection() {
    let d = resolve(None, false, env_of(&[(THEME_ENV, "light")]), never);
    assert_eq!((d.background, d.source), (Background::Light, Source::Env));
    let d = resolve(None, false, env_of(&[(THEME_ENV, "high-contrast")]), never);
    assert_eq!((d.contrast, d.source), (Contrast::High, Source::Env));
    // `--theme auto` defers to the variable.
    let d = resolve(
        Some(ThemeChoice::Auto),
        false,
        env_of(&[(THEME_ENV, "light")]),
        never,
    );
    assert_eq!(d.source, Source::Env);
}

#[test]
fn an_osc11_reply_decides_when_nothing_is_explicit() {
    let white = || parse_osc11_reply(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
    // COLORFGBG says dark, the terminal says light: the terminal is the better witness.
    let d = resolve(None, false, env_of(&[("COLORFGBG", "15;0")]), white);
    assert_eq!(
        (d.contrast, d.background, d.source),
        (Contrast::Normal, Background::Light, Source::Osc11)
    );
    let solarized = || parse_osc11_reply(b"\x1b]11;rgb:0000/2b2b/3636\x07");
    let d = resolve(
        Some(ThemeChoice::Auto),
        false,
        env_of(&[(THEME_ENV, "auto")]),
        solarized,
    );
    assert_eq!((d.background, d.source), (Background::Dark, Source::Osc11));
}

#[test]
fn without_a_reply_colorfgbg_then_dark() {
    let silent = || None;
    let d = resolve(None, false, env_of(&[("COLORFGBG", "0;15")]), silent);
    assert_eq!(
        (d.background, d.source),
        (Background::Light, Source::ColorFgBg)
    );
    let d = resolve(None, false, env_of(&[("COLORFGBG", "15;default")]), silent);
    assert_eq!(
        (d.background, d.source),
        (Background::Dark, Source::Default)
    );
    let d = resolve(None, false, env_of(&[]), silent);
    assert_eq!(
        (d.contrast, d.background, d.source),
        (Contrast::Normal, Background::Dark, Source::Default)
    );
    // A terminal that ignores OSC 11 only answers DA1: same as silence.
    let da1_only = || parse_osc11_reply(b"\x1b[?62;22c");
    let d = resolve(None, false, env_of(&[]), da1_only);
    assert_eq!(d.source, Source::Default);
}

#[test]
fn no_color_and_dumb_terminals_are_not_queried() {
    for env in [
        env_of(&[("NO_COLOR", "1"), ("COLORFGBG", "0;15")]),
        env_of(&[("TERM", "dumb"), ("COLORFGBG", "0;15")]),
    ] {
        let d = resolve(None, false, env, never);
        assert_eq!(
            (d.background, d.source),
            (Background::Light, Source::ColorFgBg)
        );
    }
    // An empty NO_COLOR does not count (no-color.org).
    let asked = Cell::new(false);
    let d = resolve(None, false, env_of(&[("NO_COLOR", "")]), || {
        asked.set(true);
        None
    });
    assert!(asked.get());
    assert_eq!(d.source, Source::Default);
}

#[test]
fn an_invalid_variable_is_ignored_and_reported() {
    let d = resolve(None, false, env_of(&[(THEME_ENV, "solarized")]), || {
        parse_osc11_reply(b"\x1b]11;rgb:ffff/ffff/ffff\x07")
    });
    assert_eq!((d.background, d.source), (Background::Light, Source::Osc11));
    assert!(d.invalid_env);
    let d = resolve(None, false, env_of(&[(THEME_ENV, "  ")]), || None);
    assert!(!d.invalid_env);
}

#[test]
fn detects_the_end_of_the_da1_reply() {
    assert!(reply_complete(b"\x1b]11;rgb:0/0/0\x07\x1b[?62;22c"));
    assert!(reply_complete(b"\x1b[?1;2c"));
    assert!(!reply_complete(b"\x1b[?62;22"));
    assert!(!reply_complete(b"\x1b]11;rgb:cccc/cccc/cccc\x07"));
}

#[test]
fn the_query_asks_for_the_background_and_then_for_da1() {
    assert!(OSC11_QUERY.starts_with(b"\x1b]11;?\x1b\\"));
    assert!(OSC11_QUERY.ends_with(b"\x1b[c"));
}

#[test]
fn no_color_flag_skips_the_query() {
    let d = resolve(None, true, env_of(&[]), never);
    assert_eq!(
        (d.background, d.source),
        (Background::Dark, Source::Default)
    );
}

#[test]
fn the_color_depth_comes_from_the_environment() {
    let mode = |no_color, vars: &[(&str, &str)]| color_mode(no_color, env_of(vars));
    assert_eq!(
        mode(false, &[("COLORTERM", "truecolor")]),
        ColorMode::TrueColor
    );
    assert_eq!(mode(false, &[("COLORTERM", "24bit")]), ColorMode::TrueColor);
    assert_eq!(
        mode(false, &[("TERM", "xterm-256color")]),
        ColorMode::Ansi256
    );
    assert_eq!(mode(false, &[("TERM", "xterm")]), ColorMode::Ansi16);
    assert_eq!(mode(false, &[]), ColorMode::Ansi16);
    // No color wins over everything, and an empty value is absent.
    assert_eq!(
        mode(true, &[("COLORTERM", "truecolor")]),
        ColorMode::NoColor
    );
    assert_eq!(
        mode(false, &[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]),
        ColorMode::NoColor
    );
    assert_eq!(
        mode(false, &[("NO_COLOR", ""), ("COLORTERM", "truecolor")]),
        ColorMode::TrueColor
    );
    assert_eq!(mode(false, &[("TERM", "dumb")]), ColorMode::NoColor);
}

#[test]
fn the_symbol_set_follows_the_flag_and_the_locale() {
    let set = |ascii, vars: &[(&str, &str)]| symbol_set(ascii, env_of(vars));
    assert_eq!(set(false, &[("LANG", "es_ES.UTF-8")]), SymbolSet::Unicode);
    assert_eq!(set(false, &[("LANG", "en_US.utf8")]), SymbolSet::Unicode);
    assert_eq!(set(false, &[("LANG", "C")]), SymbolSet::Ascii);
    assert_eq!(
        set(false, &[("LC_ALL", "POSIX"), ("LANG", "en_US.UTF-8")]),
        SymbolSet::Ascii
    );
    assert_eq!(
        set(false, &[("LC_ALL", ""), ("LC_CTYPE", "en_US.ISO8859-1")]),
        SymbolSet::Ascii
    );
    assert_eq!(set(false, &[]), SymbolSet::Unicode);
    assert_eq!(set(true, &[("LANG", "en_US.UTF-8")]), SymbolSet::Ascii);
}
