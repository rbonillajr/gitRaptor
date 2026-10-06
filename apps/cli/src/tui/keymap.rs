//! The single table action ↔ keys (ADR-CKP-003 § 10). It feeds the input,
//! the key hints and the help at once, so they cannot diverge.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::present::i18n::Text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Reconnect now instead of waiting for the next attempt.
    Retry,
}

/// A key as the table names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
}

impl Key {
    pub fn label(self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Ctrl(c) => format!("Ctrl-{}", c.to_ascii_uppercase()),
        }
    }

    fn matches(self, event: &KeyEvent) -> bool {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        match (self, event.code) {
            (Self::Char(k), KeyCode::Char(c)) => !ctrl && k == c,
            (Self::Ctrl(k), KeyCode::Char(c)) => ctrl && k.eq_ignore_ascii_case(&c),
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
];

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
    }
}
