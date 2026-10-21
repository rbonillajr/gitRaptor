//! The whole screen under the constraints of the story: a terminal below the minimum, no
//! color, ASCII, both languages and the `?` help. Unit twins of `tests/tui_screens.rs`, with
//! fixed times and no waiting.

use gitraptor_api::messages::{SessionStateView, UnavailableReason, WorktreeStatus, WorktreeView};
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::crossterm::event::KeyCode;
use ratatui::style::Color;

use super::tests::{
    asking, discovering, key, lines, model_with, outside, ready, render, screen, session, shop,
    shop_model, worktree,
};
use crate::model::{ConnEvent, Model, Msg, Notice, ObserveFailure, Size};
use crate::present::i18n::Lang;
use crate::tui::update::update;
use crate::tui::widgets::tests::style_runs;

fn themed(model: Model, mode: ColorMode, symbols: SymbolSet) -> Model {
    model.with_theme(Theme::new(mode, Contrast::Normal, symbols))
}

fn unavailable(path: &str) -> WorktreeView {
    worktree(
        path,
        false,
        WorktreeStatus::Unavailable {
            reason: UnavailableReason::Missing,
        },
    )
}

/// The lines of the screen, trimmed and joined by a space: a wrapped message reads as one.
fn flat(model: &Model, width: u16, height: u16) -> String {
    lines(&render(model, width, height))
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn snapshot(name: &str, model: &Model, width: u16, height: u16) {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| {
        let buffer = render(model, width, height);
        let snap = format!(
            "{}\n=== styles ===\n{}",
            lines(&buffer).join("\n"),
            style_runs(&buffer)
        );
        insta::assert_snapshot!(name, snap);
    });
}

fn only_ascii_or_letters(painted: &str) -> Option<char> {
    painted
        .chars()
        .find(|c| !(c.is_ascii() || c.is_alphabetic() || matches!(c, '¿' | '¡')))
}

/// Below 80×24 only the size message; widening the terminal brings the fleet back without
/// restarting.
#[test]
fn too_small_then_resized_without_restart() {
    for (lang, message, name) in [
        (
            Lang::En,
            "Terminal too small (70×20). Resize it to at least 80×24.",
            "too_small_70x20_en",
        ),
        (
            Lang::Es,
            "Terminal demasiado pequeña (70×20). Amplía la terminal a 80×24 como mínimo.",
            "too_small_70x20_es",
        ),
    ] {
        let mut model = shop_model(lang);
        let small = flat(&model, 70, 20);
        assert!(small.contains(message), "{small}");
        assert!(!small.contains("feat-pagos"), "{small}");
        assert!(!small.contains("GitRaptor"), "no header: {small}");
        snapshot(name, &model, 70, 20);
        update(
            &mut model,
            Msg::Resize(Size {
                width: 80,
                height: 24,
            }),
        );
        let big = screen(&model, 80, 24);
        assert!(big.contains("feat-pagos"), "{big}");
        assert!(!big.contains(&message[..14]), "{big}");
    }
}

/// Every state reads by its symbol and its words, and no cell has a color.
#[test]
fn every_state_reads_without_color() {
    for (symbols, rows) in [
        (
            SymbolSet::Unicode,
            [
                ("●", "claude-1"),
                ("◐", "claude-2"),
                ("○", "feat-docs"),
                ("⚠", "feat-gone"),
            ],
        ),
        (
            SymbolSet::Ascii,
            [
                ("*", "claude-1"),
                ("~", "claude-2"),
                ("o", "feat-docs"),
                ("[!]", "feat-gone"),
            ],
        ),
    ] {
        let mut worktrees = shop();
        worktrees.push(worktree(
            "/w/shop/feat-idle",
            false,
            ready("feat-idle", 0, 0, 0),
        ));
        worktrees.push(unavailable("/w/shop/feat-gone"));
        let model = themed(
            model_with(
                Lang::En,
                worktrees,
                Some(vec![
                    session(
                        "s1",
                        "/w/shop/feat-pagos",
                        "claude-1",
                        SessionStateView::Active,
                    ),
                    session(
                        "s2",
                        "/w/shop/feat-idle",
                        "claude-2",
                        SessionStateView::Inactive,
                    ),
                ]),
            ),
            ColorMode::NoColor,
            symbols,
        );
        let buffer = render(&model, 100, 30);
        let painted = lines(&buffer);
        for (symbol, name) in rows {
            let line = painted.iter().find(|l| l.contains(name)).expect(name);
            assert!(line.contains(symbol), "{symbols:?} {name}: {line}");
        }
        let all = painted.join("\n");
        for word in ["No agent", "folder missing"] {
            assert!(all.contains(word), "{word}\n{all}");
        }
        for cell in buffer.content() {
            assert_eq!(cell.fg, Color::Reset, "{symbols:?}: {:?}", cell.symbol());
            assert_eq!(cell.bg, Color::Reset, "{symbols:?}: {:?}", cell.symbol());
        }
        let name = match symbols {
            SymbolSet::Unicode => "fleet_states_100x30_unicode",
            SymbolSet::Ascii => "fleet_states_100x30_ascii",
        };
        snapshot(name, &model, 100, 30);
    }
}

/// The bar ends with the meaning of the states that are on screen.
#[test]
fn the_bar_explains_the_states_on_screen() {
    for (symbols, active, idle, glyph) in [
        (
            SymbolSet::Unicode,
            "● agente activo",
            "◐ agente inactivo",
            "◐",
        ),
        (
            SymbolSet::Ascii,
            "* agente activo",
            "~ agente inactivo",
            "~",
        ),
    ] {
        let main = || worktree("/w/shop", true, ready("main", 0, 0, 0));
        let pagos = || worktree("/w/shop/feat-pagos", false, ready("feat-pagos", 2, 0, 0));
        let sleepy = || worktree("/w/shop/feat-idle", false, ready("feat-idle", 0, 0, 0));
        let active_sessions = || {
            vec![
                session("s0", "/w/shop", "claude-0", SessionStateView::Active),
                session(
                    "s1",
                    "/w/shop/feat-pagos",
                    "claude-1",
                    SessionStateView::Active,
                ),
            ]
        };
        let mut all = active_sessions();
        all.push(session(
            "s2",
            "/w/shop/feat-idle",
            "claude-2",
            SessionStateView::Inactive,
        ));
        let both = themed(
            model_with(Lang::Es, vec![main(), pagos(), sleepy()], Some(all)),
            ColorMode::NoColor,
            symbols,
        );
        let shown = lines(&render(&both, 120, 30));
        let bar = shown.iter().rev().find(|l| !l.trim().is_empty()).unwrap();
        assert!(bar.contains(active) && bar.contains(idle), "{bar}");
        assert!(bar.find(active) < bar.find(idle), "{bar}");
        let no_idle = themed(
            model_with(Lang::Es, vec![main(), pagos()], Some(active_sessions())),
            ColorMode::NoColor,
            symbols,
        );
        let shown = screen(&no_idle, 120, 30);
        assert!(shown.contains(active), "{shown}");
        assert!(!shown.contains("agente inactivo"), "{shown}");
        assert!(
            symbols == SymbolSet::Ascii || !shown.contains(glyph),
            "{shown}"
        );
    }
}

/// Nine agents, nine rows told apart by name (BR-CKP-EDGE-006).
#[test]
fn nine_agents_are_told_apart_by_name() {
    let mut worktrees = vec![worktree("/w/shop", true, ready("main", 0, 0, 0))];
    let mut sessions = Vec::new();
    for n in 1..=9 {
        let path = format!("/w/shop/feat-{n}");
        worktrees.push(worktree(&path, false, ready(&format!("feat-{n}"), 0, 0, 0)));
        sessions.push(session(
            &format!("s{n}"),
            &path,
            &format!("claude-{n}"),
            SessionStateView::Active,
        ));
    }
    let model = themed(
        model_with(Lang::En, worktrees, Some(sessions)),
        ColorMode::NoColor,
        SymbolSet::Unicode,
    );
    let painted = lines(&render(&model, 100, 30));
    let found: Vec<&String> = (1..=9)
        .map(|n| {
            painted
                .iter()
                .find(|l| l.contains(&format!("claude-{n} ")))
                .unwrap_or_else(|| panic!("claude-{n} is missing:\n{}", painted.join("\n")))
        })
        .collect();
    for (i, a) in found.iter().enumerate() {
        for b in &found[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

fn choosing(lang: Lang) -> Model {
    let mut model = outside(lang, &["shop", "blog"]);
    update(&mut model, Msg::Conn(ConnEvent::Unlocated));
    model
}

fn failed(lang: Lang) -> Model {
    let mut model = asking(lang);
    update(&mut model, key(KeyCode::Char('y')));
    update(
        &mut model,
        Msg::Conn(ConnEvent::ObserveFailed(ObserveFailure::NotTrusted)),
    );
    model
}

fn with_help(mut model: Model) -> Model {
    update(&mut model, key(KeyCode::Char('?')));
    model
}

/// In ASCII nothing but ASCII, letters and the Spanish inverted marks reaches the terminal,
/// on every screen.
#[test]
fn ascii_screens_paint_only_ascii() {
    // The header of the failed observation carries its notice.
    assert_eq!(
        failed(Lang::En).ui.notice,
        Some(Notice::ObserveFailed(ObserveFailure::NotTrusted))
    );
    for lang in [Lang::En, Lang::Es] {
        let screens: Vec<(&str, Model, u16, u16)> = vec![
            ("fleet", shop_model(lang), 100, 30),
            ("picker", choosing(lang), 100, 30),
            ("question", asking(lang), 100, 30),
            ("discovered", discovering(lang), 100, 30),
            ("help", with_help(shop_model(lang)), 100, 30),
            ("help picker", with_help(choosing(lang)), 100, 30),
            ("too small", shop_model(lang), 70, 20),
            ("observe failed", failed(lang), 100, 30),
        ];
        for (name, model, width, height) in screens {
            let model = themed(model, ColorMode::NoColor, SymbolSet::Ascii);
            let painted = screen(&model, width, height);
            if let Some(c) = only_ascii_or_letters(&painted) {
                panic!("{name} in {lang:?} paints {c:?}:\n{painted}");
            }
        }
    }
}

#[test]
fn the_screen_speaks_the_language() {
    let es = screen(&shop_model(Lang::Es), 100, 30);
    for word in ["Flota", "salir", "ayuda"] {
        assert!(es.contains(word), "{word}\n{es}");
    }
    for word in ["Fleet", "quit", "help"] {
        assert!(!es.contains(word), "{word}\n{es}");
    }
    let en = screen(&shop_model(Lang::En), 100, 30);
    for word in ["Fleet", "quit", "help"] {
        assert!(en.contains(word), "{word}\n{en}");
    }
    for word in ["Flota", "salir", "ayuda"] {
        assert!(!en.contains(word), "{word}\n{en}");
    }
}

/// `?` lists the keys of the panel it is opened in.
#[test]
fn question_mark_shows_the_keys_of_the_panel() {
    let open = |model: Model| screen(&with_help(model), 80, 24);
    let fleet = open(shop_model(Lang::Es));
    for token in ["Siempre", "Ctrl-Z", "q", "r", "?", "Esc"] {
        assert!(fleet.contains(token), "{token}\n{fleet}");
    }
    assert!(!fleet.contains("Enter"), "{fleet}");
    // The last symbol of the legend and the way out fit in 80×24.
    assert!(fleet.contains("aviso") && fleet.contains("Esc"), "{fleet}");
    let picker = open(choosing(Lang::Es));
    for token in ["↑ k", "↓ j", "Enter", "Esc", "aviso"] {
        assert!(picker.contains(token), "{token}\n{picker}");
    }
    let question = open(asking(Lang::Es));
    for token in ["s y", "n", "Esc", "aviso"] {
        assert!(question.contains(token), "{token}\n{question}");
    }
    snapshot(
        "help_fleet_80x24_es",
        &with_help(shop_model(Lang::Es)),
        80,
        24,
    );
    snapshot(
        "help_picker_80x24_es",
        &with_help(choosing(Lang::Es)),
        80,
        24,
    );
    snapshot(
        "help_question_80x24_es",
        &with_help(asking(Lang::Es)),
        80,
        24,
    );
    snapshot(
        "help_fleet_80x24_en",
        &with_help(shop_model(Lang::En)),
        80,
        24,
    );
}
