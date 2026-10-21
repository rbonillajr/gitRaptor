//! What the keys do right now: the actions the current panel accepts (one source for the key
//! bar and for the `?` help) and the help model with its symbol legend.
//!
//! Every text goes through [`Say`], so the ASCII set never sees a Unicode glyph.

use gitraptor_theme::{ColorToken, SymbolToken};

use crate::model::{Model, Pick, SafeText};
use crate::present::i18n::{HelpPanel, Lang, Legend, Text};
use crate::tui::keymap::{Action, BINDINGS, Binding, Key};
use crate::tui::view::Say;
use crate::tui::widgets::key_hints::{HelpGroup, HelpModel, KeyHint, LegendRow};

/// The widest the help is drawn; the view clips it to the body.
pub const HELP_WIDTH: u16 = 64;

/// The panel that receives the keys: the repo picker, a question (the folder's or a
/// discovered repo) or the fleet.
pub fn context(model: &Model) -> HelpPanel {
    if matches!(model.ui.pick, Pick::Choosing { .. }) {
        HelpPanel::Picker
    } else if model.ui.pick == Pick::Asking || model.discovery_prompt().is_some() {
        HelpPanel::Question
    } else {
        HelpPanel::Fleet
    }
}

/// The actions the panel on screen accepts: the list keys only while there is a list, the
/// answers only with a question, and "later" only for a discovered repo (the folder's
/// question has no later).
pub fn available(model: &Model) -> impl Iterator<Item = &'static Binding> + '_ {
    let list = matches!(model.ui.pick, Pick::Choosing { .. });
    let discovering = model.discovery_prompt().is_some();
    let asking = model.ui.pick == Pick::Asking || discovering;
    BINDINGS
        .iter()
        .filter(move |b| list || !b.action.is_list())
        .filter(move |b| asking || !b.action.is_answer())
        .filter(move |b| discovering || b.action != Action::Later)
}

/// The symbol, the color and the words of a state.
pub(crate) fn legend_row(legend: Legend, say: Say) -> LegendRow {
    let (symbol, color) = match legend {
        Legend::Active => (SymbolToken::AgentActive, ColorToken::AgentStateActive),
        Legend::Idle => (SymbolToken::AgentIdle, ColorToken::AgentStateIdle),
        Legend::Done => (SymbolToken::AgentDone, ColorToken::AgentStateDone),
        Legend::Warning => (SymbolToken::Warning, ColorToken::StatusWarning),
        Legend::Conflict => (SymbolToken::Conflict, ColorToken::StatusWarning),
        Legend::Blocked => (SymbolToken::Blocked, ColorToken::StatusDanger),
    };
    LegendRow::new(symbol, color, say.safe(Text::Legend(legend)))
}

/// The `?` panel: the keys of the current panel, the ones that work everywhere and what the
/// symbols mean.
pub(crate) fn help(model: &Model, say: Say) -> HelpModel {
    let (global, local): (Vec<_>, Vec<_>) = available(model).partition(|b| b.action.is_global());
    let entries = |bindings: Vec<&'static Binding>| -> Vec<KeyHint> {
        bindings.into_iter().map(|b| entry(b, say)).collect()
    };
    let mut groups = Vec::new();
    if !local.is_empty() {
        groups.push(HelpGroup {
            title: say.safe(Text::HelpPanel(context(model))),
            hints: entries(local),
            legend: Vec::new(),
        });
    }
    groups.push(HelpGroup {
        title: say.safe(Text::HelpAlways),
        hints: entries(global),
        legend: Vec::new(),
    });
    groups.push(HelpGroup {
        title: say.safe(Text::HelpSymbols),
        hints: Vec::new(),
        legend: Legend::ALL.map(|l| legend_row(l, say)).to_vec(),
    });
    HelpModel {
        title: say.safe(Text::HelpTitle),
        groups,
        note: None,
        close: KeyHint::new(
            SafeText::text(&Key::Esc.label(say.set)),
            say.safe(Text::KeyClose),
        ),
    }
}

/// One action with all its keys, separated by a space (in Spanish `s` goes before `y`).
fn entry(binding: &Binding, say: Say) -> KeyHint {
    let mut keys: Vec<Key> = binding.keys.to_vec();
    if binding.action == Action::Yes && say.lang == Lang::Es {
        keys.sort_by_key(|k| *k != Key::Char('s'));
    }
    let labels: Vec<String> = keys.iter().map(|k| k.label(say.set)).collect();
    let hint = KeyHint::new(SafeText::text(&labels.join(" ")), say.safe(binding.hint));
    // Job control exists only on Unix: the help says so instead of hiding the key.
    if binding.action == Action::Suspend && !cfg!(unix) {
        hint.disabled(say.safe(Text::UnixOnly))
    } else {
        hint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConnEvent;
    use crate::tui::update::update;
    use crate::tui::view::{asking, discovering, outside, shop_model};

    fn actions(model: &Model) -> Vec<Action> {
        available(model).map(|b| b.action).collect()
    }

    fn choosing() -> Model {
        let mut model = outside(Lang::En, &["shop", "blog"]);
        update(&mut model, crate::model::Msg::Conn(ConnEvent::Unlocated));
        model
    }

    #[test]
    fn available_follows_the_panel() {
        let fleet = actions(&shop_model(Lang::En));
        for hidden in [
            Action::Up,
            Action::Down,
            Action::Open,
            Action::Yes,
            Action::No,
            Action::Later,
        ] {
            assert!(!fleet.contains(&hidden), "{hidden:?} in the fleet");
        }
        for shown in [Action::Quit, Action::Retry, Action::Suspend, Action::Help] {
            assert!(fleet.contains(&shown), "{shown:?} missing from the fleet");
        }
        let picker = actions(&choosing());
        for shown in [Action::Up, Action::Down, Action::Open] {
            assert!(picker.contains(&shown), "{shown:?} in the picker");
        }
        let question = actions(&asking(Lang::En));
        assert!(question.contains(&Action::Yes) && question.contains(&Action::No));
        assert!(!question.contains(&Action::Later), "the folder's question");
        let found = actions(&discovering(Lang::En));
        for shown in [Action::Yes, Action::No, Action::Later] {
            assert!(found.contains(&shown), "{shown:?} for a discovered repo");
        }
    }

    #[test]
    fn the_context_names_the_panel() {
        assert_eq!(context(&shop_model(Lang::En)), HelpPanel::Fleet);
        assert_eq!(context(&choosing()), HelpPanel::Picker);
        assert_eq!(context(&asking(Lang::En)), HelpPanel::Question);
        assert_eq!(context(&discovering(Lang::En)), HelpPanel::Question);
    }
}
