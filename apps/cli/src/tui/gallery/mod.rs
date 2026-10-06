//! `raptor ui gallery`: every component in every state, with sample data, browsable with the
//! keyboard. The terminal's Storybook (TS-CKP-005). A hidden developer tool: it opens no channel
//! and writes nothing; its keys are its own, not the app keymap.

pub mod stories;

use std::io::{self, Write};

use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::model::SafeText;
use crate::tui::style::{Pen, Styles, fill};
use crate::tui::widgets::key_hints::{KeyHint, KeyHintsModel};
use crate::tui::widgets::themed;
use stories::{Story, stories};

/// The theme modes every component is shown and pinned in. A new theme variant of
/// `crates/theme` (the light-terminal one) is one more value here: the gallery, `--dump` and
/// every snapshot pick it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    #[value(name = "truecolor")]
    TrueColor,
    #[value(name = "256")]
    Ansi256,
    #[value(name = "16")]
    Ansi16,
    #[value(name = "no-color")]
    NoColor,
    #[value(name = "high-contrast")]
    HighContrast,
    /// Truecolor with the ASCII symbol set.
    #[value(name = "ascii")]
    Ascii,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::TrueColor,
        Mode::Ansi256,
        Mode::Ansi16,
        Mode::NoColor,
        Mode::HighContrast,
        Mode::Ascii,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Mode::TrueColor => "truecolor",
            Mode::Ansi256 => "256",
            Mode::Ansi16 => "16",
            Mode::NoColor => "no-color",
            Mode::HighContrast => "high-contrast",
            Mode::Ascii => "ascii",
        }
    }

    pub fn theme(self) -> Theme {
        self.theme_with(match self {
            Mode::Ascii => SymbolSet::Ascii,
            _ => SymbolSet::Unicode,
        })
    }

    /// The theme of this mode with a given symbol set. Every setting of a mode (depth, contrast
    /// and, when it lands, the terminal background) lives here, so the snapshots that cross
    /// symbol sets with modes keep all of it.
    pub fn theme_with(self, symbols: SymbolSet) -> Theme {
        let (mode, contrast) = match self {
            Mode::TrueColor | Mode::Ascii => (ColorMode::TrueColor, Contrast::Normal),
            Mode::Ansi256 => (ColorMode::Ansi256, Contrast::Normal),
            Mode::Ansi16 => (ColorMode::Ansi16, Contrast::Normal),
            Mode::NoColor => (ColorMode::NoColor, Contrast::Normal),
            Mode::HighContrast => (ColorMode::TrueColor, Contrast::High),
        };
        Theme::new(mode, contrast, symbols)
    }
}

/// Plain text of a buffer, one line per row, trailing spaces trimmed.
pub fn buffer_text(buf: &Buffer) -> Vec<String> {
    let area = buf.area;
    (area.top()..area.bottom())
        .map(|y| {
            // A wide glyph hides the cells it covers, as in a terminal.
            let mut line = String::new();
            let mut x = area.left();
            while x < area.right() {
                let symbol = buf[(x, y)].symbol();
                line.push_str(symbol);
                x += crate::tui::style::width(symbol).max(1);
            }
            line.trim_end().to_owned()
        })
        .collect()
}

/// `--dump`: every story in one mode, as plain text (the capture for a PR and for CI).
pub fn dump(mode: Mode, out: &mut impl Write) -> io::Result<()> {
    let styles = Styles::new(mode.theme());
    for story in stories() {
        writeln!(
            out,
            "== {} / {} ({}x{}, {}) ==",
            story.component,
            story.state,
            story.width,
            story.height,
            mode.name()
        )?;
        for line in buffer_text(&story.render(&styles)) {
            writeln!(out, "{line}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// Where the gallery is: which component, which of its states, which mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub component: usize,
    pub state: usize,
    pub mode: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    NextComponent,
    PrevComponent,
    NextState,
    PrevState,
    NextMode,
    Quit,
}

/// Story indexes grouped by component, in order.
fn groups(all: &[Story]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, s) in all.iter().enumerate() {
        match groups.last_mut() {
            Some(g) if all[g[0]].component == s.component => g.push(i),
            _ => groups.push(vec![i]),
        }
    }
    groups
}

impl Cursor {
    /// Applies a key; returns false on quit.
    pub fn on_key(&mut self, key: Key, groups: &[Vec<usize>]) -> bool {
        let n = groups.len();
        match key {
            Key::NextComponent => {
                self.component = (self.component + 1) % n;
                self.state = 0;
            }
            Key::PrevComponent => {
                self.component = (self.component + n - 1) % n;
                self.state = 0;
            }
            Key::NextState => self.state = (self.state + 1) % groups[self.component].len(),
            Key::PrevState => {
                let m = groups[self.component].len();
                self.state = (self.state + m - 1) % m;
            }
            Key::NextMode => self.mode = (self.mode + 1) % Mode::ALL.len(),
            Key::Quit => return false,
        }
        true
    }
}

fn key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => Key::NextComponent,
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => Key::PrevComponent,
        KeyCode::Down | KeyCode::Char('j') => Key::NextState,
        KeyCode::Up | KeyCode::Char('k') => Key::PrevState,
        KeyCode::Char('m') => Key::NextMode,
        KeyCode::Char('q') | KeyCode::Esc => Key::Quit,
        _ => return None,
    })
}

/// Paints the gallery frame: a title line, the story and the gallery's own key hints.
pub fn paint(cursor: Cursor, all: &[Story], area: Rect, buf: &mut Buffer) {
    let groups = groups(all);
    let mode = Mode::ALL[cursor.mode];
    let styles = Styles::new(mode.theme());
    fill(buf, area, &styles);
    let story = &all[groups[cursor.component][cursor.state]];

    let sep = styles.glyphs.separator;
    let title = format!(
        "{} ({}/{}){sep}{} ({}/{}){sep}mode {}{sep}{}x{}",
        story.component,
        cursor.component + 1,
        groups.len(),
        story.state,
        cursor.state + 1,
        groups[cursor.component].len(),
        mode.name(),
        story.width,
        story.height
    );
    let bold = styles.base().add_modifier(Modifier::BOLD);
    Pen::new(buf, area, area.y, &styles)
        .gap(1)
        .text(&title, bold);

    let body = Rect::new(
        area.x + 1,
        area.y + 2,
        area.width.saturating_sub(2),
        area.height.saturating_sub(3),
    );
    let rendered = story.render(&styles);
    for y in 0..story.height.min(body.height) {
        for x in 0..story.width.min(body.width) {
            buf[(body.x + x, body.y + y)] = rendered[(x, y)].clone();
        }
    }

    let hints = KeyHintsModel {
        hints: vec![
            KeyHint::new(SafeText::text("Tab/h/l"), SafeText::text("component")),
            KeyHint::new(SafeText::text("j/k"), SafeText::text("state")),
            KeyHint::new(SafeText::text("m"), SafeText::text("mode")),
        ],
        help: KeyHint::new(SafeText::text("q"), SafeText::text("quit")),
    };
    let last = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
    themed(&hints, &styles).render(last, buf);
}

/// The interactive gallery. `ratatui::init` installs the panic hook that restores the terminal.
pub fn run(mode: Mode) -> io::Result<()> {
    let all = stories();
    let groups = groups(&all);
    let mut cursor = Cursor {
        component: 0,
        state: 0,
        mode: Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0),
    };
    let mut terminal = ratatui::init();
    let result = (|| -> io::Result<()> {
        loop {
            terminal.draw(|frame| paint(cursor, &all, frame.area(), frame.buffer_mut()))?;
            if let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
                && let Some(key) = key(k.code)
                && !cursor.on_key(key, &groups)
            {
                return Ok(());
            }
        }
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_wraps_and_resets_the_state() {
        let all = stories();
        let groups = groups(&all);
        let mut c = Cursor {
            component: 0,
            state: 2,
            mode: 0,
        };
        assert!(c.on_key(Key::PrevComponent, &groups));
        assert_eq!((c.component, c.state), (groups.len() - 1, 0));
        assert!(c.on_key(Key::NextComponent, &groups));
        assert_eq!(c.component, 0);
        assert!(c.on_key(Key::PrevState, &groups));
        assert_eq!(c.state, groups[0].len() - 1);
        for _ in 0..Mode::ALL.len() {
            c.on_key(Key::NextMode, &groups);
        }
        assert_eq!(c.mode, 0);
        assert!(!c.on_key(Key::Quit, &groups));
    }

    #[test]
    fn the_gallery_covers_the_ten_components() {
        let all = stories();
        let names: Vec<&str> = groups(&all).iter().map(|g| all[g[0]].component).collect();
        for component in [
            "Layout",
            "StatusBar",
            "AgentList",
            "GraphLanes",
            "DiffView",
            "TimelineList",
            "ConflictAlert",
            "PolicyBanner",
            "ConfirmPrompt",
            "Notification",
            "KeyHints",
        ] {
            assert!(names.contains(&component), "{component} missing");
        }
    }

    #[test]
    fn the_frame_paints_title_story_and_hints() {
        let all = stories();
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        let cursor = Cursor {
            component: 6,
            state: 0,
            mode: 5,
        };
        paint(cursor, &all, area, &mut buf);
        let text = buffer_text(&buf).join("\n");
        assert!(text.contains("ConflictAlert (7/11)"), "{text}");
        assert!(text.contains("mode ascii"));
        assert!(text.contains("[c] Conflict predicted"));
        assert!(text.contains("q quit"));
    }

    #[test]
    fn dump_lists_every_story() {
        let mut out = Vec::new();
        dump(Mode::Ascii, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.matches("== ").count(), stories().len());
        assert!(out.is_ascii(), "the ASCII mode paints only ASCII");
    }
}
