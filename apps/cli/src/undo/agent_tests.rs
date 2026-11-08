//! TMC011-CLI-TEXT (T009 of DS-US-TMC-011): the texts of `raptor undo --agent`, in English and
//! Spanish, with their markers. Child module of `undo` (`use super::*`).

use super::*;
use crate::i18n::{has_key, text_in};

fn placeholders(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn agent_messages_exist_in_en_and_es() {
    let expected: [(&str, &[&str]); 6] = [
        (
            "undo.done-agent",
            &["agent", "count", "id", "since", "worktree"],
        ),
        ("undo.reason.nothing-of-agent", &["agent", "since"]),
        ("undo.reason.not-selectable", &["agent"]),
        ("undo.reason.selective-undo-below", &[]),
        ("undo.reason.attribution-unavailable", &[]),
        ("undo.invalid-agent", &[]),
    ];
    for (key, markers) in expected {
        assert!(
            has_key(key),
            "TMC011-CLI-TEXT: {key} must exist in en and es"
        );
        for spanish in [false, true] {
            let text = text_in(spanish, key).expect(key);
            assert_eq!(
                placeholders(text),
                markers,
                "TMC011-CLI-TEXT: {key} (es={spanish}) must carry exactly its markers"
            );
        }
    }
}

#[test]
fn selective_undo_below_names_both_exits() {
    for spanish in [false, true] {
        let text = text_in(spanish, "undo.reason.selective-undo-below")
            .expect("TMC011-CLI-TEXT: undo.reason.selective-undo-below must exist");
        for exit in ["raptor redo", "raptor timeline"] {
            assert!(
                text.contains(exit),
                "TMC011-CLI-TEXT: selective-undo-below (es={spanish}) must name `{exit}`: {text}"
            );
        }
    }
}

#[test]
fn nothing_of_agent_names_the_worktree_scope() {
    assert!(
        text_in(false, "undo.reason.nothing-of-agent").is_some_and(|t| t.contains("this worktree")),
        "TMC011-CLI-TEXT: the English text says 'this worktree'"
    );
    assert!(
        text_in(true, "undo.reason.nothing-of-agent").is_some_and(|t| t.contains("este worktree")),
        "TMC011-CLI-TEXT: the Spanish text says 'este worktree'"
    );
}
