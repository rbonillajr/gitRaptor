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
}

/// A key as the table names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Enter,
}

impl Key {
    pub fn label(self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Ctrl(c) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            Self::Up => "↑".into(),
            Self::Down => "↓".into(),
            Self::Enter => "Enter".into(),
        }
    }

    fn matches(self, event: &KeyEvent) -> bool {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        match (self, event.code) {
            (Self::Char(k), KeyCode::Char(c)) => !ctrl && k == c,
            (Self::Ctrl(k), KeyCode::Char(c)) => ctrl && k.eq_ignore_ascii_case(&c),
            (Self::Up, KeyCode::Up)
            | (Self::Down, KeyCode::Down)
            | (Self::Enter, KeyCode::Enter) => !ctrl,
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
];

impl Action {
    /// Moves within a list: hinted only while there is one to move in.
    pub fn is_list(self) -> bool {
        matches!(self, Self::Up | Self::Down | Self::Open)
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
        assert_eq!(action(&key(KeyCode::Esc, KeyModifiers::NONE)), None);
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
    }
}
