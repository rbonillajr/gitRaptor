//! The single table action ↔ keys (ADR-CKP-003 § 10). It feeds the input,
//! the key hints and the help at once, so they cannot diverge.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::present::i18n::Text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Reconnect now instead of waiting for the next attempt.
    Retry,
    /// Move the selection of a list.
    Up,
    Down,
    /// Open what is selected.
    Open,
    /// Hand the terminal back to the shell until `fg` (job control).
    Suspend,
    /// Answer yes to the question on screen ("Observe this repo? [y/N]"): `y`, or `s` for
    /// "sí", in both languages.
    Yes,
    /// Answer no (the default; Enter says no too).
    No,
    /// Leave the question for later: nothing is decided (`Esc`).
    Later,
}

/// A key as the table names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Enter,
    Esc,
}

impl Key {
    pub fn label(self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Ctrl(c) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            Self::Up => "↑".into(),
            Self::Down => "↓".into(),
            Self::Enter => "Enter".into(),
            Self::Esc => "Esc".into(),
        }
    }

    fn matches(self, event: &KeyEvent) -> bool {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        match (self, event.code) {
            (Self::Char(k), KeyCode::Char(c)) => !ctrl && k == c,
            (Self::Ctrl(k), KeyCode::Char(c)) => ctrl && k.eq_ignore_ascii_case(&c),
            (Self::Up, KeyCode::Up)
            | (Self::Down, KeyCode::Down)
            | (Self::Enter, KeyCode::Enter)
            | (Self::Esc, KeyCode::Esc) => !ctrl,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub action: Action,
    /// The first key is the one the hints show.
    pub keys: &'static [Key],
    pub hint: Text<'static>,
}

pub const BINDINGS: &[Binding] = &[
    Binding {
        action: Action::Quit,
        keys: &[Key::Char('q'), Key::Ctrl('c')],
        hint: Text::KeyQuit,
    },
    Binding {
        action: Action::Retry,
        keys: &[Key::Char('r')],
        hint: Text::KeyRetry,
    },
    Binding {
        action: Action::Up,
        keys: &[Key::Up, Key::Char('k')],
        hint: Text::KeyUp,
    },
    Binding {
        action: Action::Down,
        keys: &[Key::Down, Key::Char('j')],
        hint: Text::KeyDown,
    },
    Binding {
        action: Action::Open,
        keys: &[Key::Enter],
        hint: Text::KeyOpen,
    },
    Binding {
        action: Action::Suspend,
        keys: &[Key::Ctrl('z')],
        hint: Text::KeySuspend,
    },
    Binding {
        action: Action::Yes,
        keys: &[Key::Char('y'), Key::Char('s')],
        hint: Text::KeyObserve,
    },
    Binding {
        action: Action::No,
        keys: &[Key::Char('n')],
        hint: Text::KeyNo,
    },
    Binding {
        action: Action::Later,
        keys: &[Key::Esc],
        hint: Text::KeyLater,
    },
];

impl Action {
    /// Not in the key hints: the terminal convention every shell user already knows.
    pub fn is_hinted(self) -> bool {
        !matches!(self, Self::Quit | Self::Suspend)
    }

    /// Moves within a list: hinted only while there is one to move in.
    pub fn is_list(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Open)
    }

    /// Answers a question: hinted only while one is on screen.
    pub fn is_answer(self) -> bool {
        matches!(self, Self::Yes | Self::No | Self::Later)
    }
}

pub fn action(event: &KeyEvent) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|b| b.keys.iter().any(|k| k.matches(event)))
        .map(|b| b.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn the_table_decides_the_action() {
        assert_eq!(
            action(&key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Quit)
        );
        assert_eq!(
            action(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(action(&key(KeyCode::Char('c'), KeyModifiers::NONE)), None);
        assert_eq!(
            action(&key(KeyCode::Char('r'), KeyModifiers::NONE)),
            Some(Action::Retry)
        );
        assert_eq!(
            action(&key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Later)
        );
        for c in ['y', 's'] {
            assert_eq!(
                action(&key(KeyCode::Char(c), KeyModifiers::NONE)),
                Some(Action::Yes)
            );
        }
        assert_eq!(
            action(&key(KeyCode::Char('n'), KeyModifiers::NONE)),
            Some(Action::No)
        );
        assert_eq!(Key::Ctrl('c').label(), "Ctrl-C");
        assert_eq!(
            action(&key(KeyCode::Down, KeyModifiers::NONE)),
            Some(Action::Down)
        );
        assert_eq!(
            action(&key(KeyCode::Char('k'), KeyModifiers::NONE)),
            Some(Action::Up)
        );
        assert_eq!(
            action(&key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::Open)
        );
        assert_eq!(
            action(&key(KeyCode::Char('z'), KeyModifiers::CONTROL)),
            Some(Action::Suspend)
        );
        assert_eq!(action(&key(KeyCode::Char('z'), KeyModifiers::NONE)), None);
    }
}
