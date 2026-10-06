//! Snapshots of every component and state in every theme mode, plus the accessibility and
//! boundary checks of the library (the TUI-wide ones are in `tests/tui_boundaries.rs`) (TS-CKP-005, DSYS-GRP-001 § 6, ADR-CKP-003 V1 and V5).
//!
//! One snapshot per story. The text is painted once per symbol set: the test fails if a color
//! mode changes the text, so the color runs are the only difference between modes and they are
//! listed per mode. The first story of each component, and every Layout story, also pin the
//! ASCII set in every color mode.
//!
//! The palette is provisional (DSYS-GRP-001 § 8): when the tokens change, regenerate with
//! `INSTA_UPDATE=always cargo test -p gitraptor-cli --bin raptor tui::widgets` and review the
//! diff; no widget changes.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::tui::gallery::stories::{Story, stories};
use crate::tui::gallery::{Mode, buffer_text};
use crate::tui::style::Styles;
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};

/// Every color mode of the gallery: all modes but the ASCII one, which is a symbol set. A new
/// theme variant (e.g. the light-terminal one) added to [`Mode::ALL`] lands here by itself.
fn color_modes() -> Vec<Mode> {
    Mode::ALL
        .into_iter()
        .filter(|m| *m != Mode::Ascii)
        .collect()
}

fn color(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(i) => format!("@{i}"),
        other => format!("{other:?}"),
    }
}

/// The styled runs of a buffer: `row:from-to fg bg modifiers`, default cells left out.
pub(crate) fn style_runs(buf: &Buffer) -> String {
    let mut out = String::new();
    let area = buf.area;
    for y in area.top()..area.bottom() {
        let mut x = area.left();
        while x < area.right() {
            let key = |x: u16| {
                let c = &buf[(x, y)];
                (c.fg, c.bg, c.modifier)
            };
            let k = key(x);
            let start = x;
            while x < area.right() && key(x) == k {
                x += 1;
            }
            let (fg, bg, m) = k;
            if fg == Color::Reset && bg == Color::Reset && m.is_empty() {
                continue;
            }
            let mut run = format!("{y}:{start}-{}", x - 1);
            if fg != Color::Reset {
                let _ = write!(run, " fg={}", color(fg));
            }
            if bg != Color::Reset {
                let _ = write!(run, " bg={}", color(bg));
            }
            if m != Modifier::empty() {
                let _ = write!(run, " {m:?}");
            }
            out.push_str(&run);
            out.push('\n');
        }
    }
    out
}

fn with_symbols(mode: Mode, symbols: SymbolSet) -> Styles {
    Styles::new(mode.theme_with(symbols))
}

fn snapshot(story: &Story, full_ascii: bool) -> String {
    let mut out = String::new();
    for symbols in [SymbolSet::Unicode, SymbolSet::Ascii] {
        let set = match symbols {
            SymbolSet::Unicode => "unicode",
            SymbolSet::Ascii => "ascii",
        };
        let reference = buffer_text(&story.render(&with_symbols(Mode::TrueColor, symbols)));
        let _ = writeln!(out, "=== text ({set}) ===");
        for line in &reference {
            let _ = writeln!(out, "{line}");
        }
        let all = color_modes();
        let modes = if symbols == SymbolSet::Unicode || full_ascii {
            &all[..]
        } else {
            &all[..1]
        };
        for mode in modes {
            let buf = story.render(&with_symbols(*mode, symbols));
            assert_eq!(
                buffer_text(&buf),
                reference,
                "{} / {}: the {} mode changed the text",
                story.component,
                story.state,
                mode.name()
            );
            let _ = writeln!(out, "=== styles ({}, {set}) ===", mode.name());
            out.push_str(&style_runs(&buf));
        }
    }
    out
}

fn slug(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[test]
fn every_component_state_and_mode() {
    let all = stories();
    let mut first: HashMap<&str, bool> = HashMap::new();
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("snapshots");
    settings.bind(|| {
        for story in &all {
            let representative = first.insert(story.component, true).is_none();
            let full_ascii = representative || story.component == "Layout";
            let name = format!("{}__{}", slug(story.component), slug(story.state));
            insta::assert_snapshot!(name, snapshot(story, full_ascii));
        }
    });
}

/// Without color, two states of a component never look the same: state is never carried by
/// color alone (NFR-09, DSYS-GRP-001 § 6).
#[test]
fn states_differ_without_color() {
    let all = stories();
    for symbols in [SymbolSet::Unicode, SymbolSet::Ascii] {
        let styles = with_symbols(Mode::NoColor, symbols);
        let mut seen: HashMap<(&str, Vec<String>), &str> = HashMap::new();
        for story in &all {
            let text = buffer_text(&story.render(&styles));
            if let Some(other) = seen.insert((story.component, text), story.state) {
                panic!(
                    "{}: '{}' and '{}' look the same without color",
                    story.component, other, story.state
                );
            }
        }
    }
}

/// The ASCII set paints only ASCII (the fallback really falls back).
#[test]
fn the_ascii_set_paints_only_ascii() {
    let styles = with_symbols(Mode::NoColor, SymbolSet::Ascii);
    for story in stories() {
        for line in buffer_text(&story.render(&styles)) {
            assert!(
                line.is_ascii(),
                "{} / {}: {line}",
                story.component,
                story.state
            );
        }
    }
}

fn story(component: &str, state: &str) -> Story {
    stories()
        .into_iter()
        .find(|s| s.component == component && s.state == state)
        .expect("story")
}

fn text(component: &str, state: &str, mode: Mode) -> String {
    buffer_text(&story(component, state).render(&Styles::new(mode.theme()))).join("\n")
}

#[test]
fn focus_has_a_shape_not_only_a_color() {
    let focused = text("AgentList", "mixed states", Mode::NoColor);
    let unfocused = text("AgentList", "unfocused, scrolled", Mode::NoColor);
    assert!(focused.contains("┏━ › Agents"), "{focused}");
    assert!(focused.contains("› ●"), "{focused}");
    assert!(unfocused.contains("┌─ Agents"), "{unfocused}");
    assert!(!unfocused.contains('›'));
    let ascii = text("AgentList", "mixed states", Mode::Ascii);
    assert!(ascii.contains("#= > Agents"), "{ascii}");
}

#[test]
fn confirm_prompt_starts_on_no() {
    use crate::model::SafeText;
    use crate::tui::widgets::confirm::{Choice, ConfirmModel};
    let t = SafeText::text;
    let m = ConfirmModel::new(t("Discard"), t("Discard?"), t("Discard"), t("No"));
    assert_eq!(m.choice, Choice::No);
    let painted = text("ConfirmPrompt", "default No", Mode::NoColor);
    assert!(painted.contains("› [No]"), "{painted}");
    assert!(!painted.contains("› [Discard"), "{painted}");
}

#[test]
fn key_hints_keep_help_whatever_the_width() {
    let narrow = text("KeyHints", "narrow", Mode::TrueColor);
    assert!(narrow.trim_end().ends_with("? help"), "{narrow}");
    let layout = text("Layout", "80x24", Mode::TrueColor);
    let last = layout.lines().last().unwrap();
    assert!(
        last.contains("q quit") && last.ends_with("? help"),
        "{last}"
    );
}

#[test]
fn the_header_always_shows_the_alert_counts() {
    for state in [
        "live",
        "reconnecting",
        "engine unavailable",
        "acting as agent",
    ] {
        let line = text("StatusBar", state, Mode::Ascii);
        assert!(line.trim_end().ends_with("[c] 2  [blocked] 1"), "{line}");
    }
}

#[test]
fn below_the_minimum_only_the_size_message_and_quit_are_painted() {
    let small = text("Layout", "79x24 too small", Mode::TrueColor);
    assert!(small.contains("needs at least 80x24"), "{small}");
    assert!(small.contains("q quit"));
    assert!(!small.contains("Agents"));
}

#[test]
fn the_ninth_agent_reuses_the_first_color_and_keeps_its_name() {
    let s = story("AgentList", "ninth agent reuses color");
    let styles = Styles::new(Theme::new(
        ColorMode::TrueColor,
        Contrast::Normal,
        SymbolSet::Unicode,
    ));
    let buf = s.render(&styles);
    let lines = buffer_text(&buf);
    let find = |name: &str| {
        let y = lines.iter().position(|l| l.contains(name)).expect(name);
        let x = lines[y].find(name).unwrap();
        // Column of the name: count display cells before it.
        let col = lines[y][..x].chars().count();
        buf[(u16::try_from(col).unwrap(), u16::try_from(y).unwrap())].fg
    };
    assert_eq!(find("claude-1 "), find("claude-9"));
    assert_ne!(find("claude-1 "), find("claude-2"));
}

// --- Boundaries (ADR-CKP-003 V5, TS-CKP-005) ------------------------------------------------

fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// The code before its test module.
fn production(path: &Path) -> String {
    // CRLF on a Windows checkout.
    let code = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    match code.find("#[cfg(test)]\nmod tests {") {
        Some(i) => code[..i].to_owned(),
        None => code,
    }
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Widgets import no channel types nor the catalog, and write no color, glyph or user-facing
/// literal.
#[test]
fn widgets_are_pure_and_literal_free() {
    let dir = src().join("tui/widgets");
    for path in sources(&dir) {
        let rel = path.file_name().unwrap().to_string_lossy().into_owned();
        if rel == "tests.rs" {
            continue;
        }
        let code = production(&path);
        // The engine, Git and policy layers are checked for the whole TUI in
        // tests/tui_boundaries.rs; widgets also stay off the contract and the catalog.
        for forbidden in ["gitraptor_api", "crate::i18n", "Color::", "Rgb("] {
            assert!(!code.contains(forbidden), "{rel} uses {forbidden}");
        }
        for (n, line) in code.lines().enumerate() {
            let line = line.trim_start();
            if line.starts_with("//") {
                continue;
            }
            assert!(
                line.is_ascii(),
                "{rel}:{}: non-ASCII outside comments",
                n + 1
            );
            for literal in line.split('"').skip(1).step_by(2) {
                let bare: String = literal
                    .split('{')
                    .map(|part| part.split_once('}').map_or(part, |(_, rest)| rest))
                    .collect();
                assert!(
                    !bare.chars().any(|c| c.is_alphabetic()),
                    "{rel}:{}: user-facing literal {literal:?}",
                    n + 1
                );
            }
        }
    }
}
