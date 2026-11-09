//! TMC010-CLI-TEXT (T005 of DS-US-TMC-010): the texts of `raptor undo --since`, in English and
//! Spanish, and which one a rejection gets. Child module of `undo` (`use super::*`).

use super::*;
use crate::i18n::{has_key, text_in};
use gitraptor_api::rpc::ErrorObject;

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
fn since_messages_exist_in_en_and_es() {
    let expected: [(&str, &[&str]); 4] = [
        (
            "undo.done-since.many",
            &["count", "id", "oldest", "since", "worktree"],
        ),
        (
            "undo.done-since.one",
            &["id", "oldest", "since", "worktree"],
        ),
        ("undo.reason.nothing-in-period", &["since"]),
        ("undo.invalid-since", &[]),
    ];
    for (key, markers) in expected {
        assert!(
            has_key(key),
            "TMC010-CLI-TEXT: {key} must exist in en and es"
        );
        for spanish in [false, true] {
            let text = text_in(spanish, key).expect(key);
            assert_eq!(
                placeholders(text),
                markers,
                "TMC010-CLI-TEXT: {key} (es={spanish}) must carry exactly its markers"
            );
        }
    }
}

fn nothing_to_undo() -> ClientError {
    ClientError::Rpc(
        ErrorObject::new(code::OPERATION_REJECTED, "rejected")
            .with_data(json!({ "reason": "nothing-to-undo" })),
    )
}

#[test]
fn nothing_to_undo_with_since_tells_the_period() {
    let wt = Path::new("/w/feat-login");
    let text = failure_text(nothing_to_undo(), wt, Some("20m"));
    assert_eq!(
        text,
        t("undo.reason.nothing-in-period", &[("since", &"20m")]),
        "TMC010-CLI-TEXT: nothing-to-undo with --since says nothing happened in that period"
    );
    assert!(text.contains("20m"), "{text}");
    let without = failure_text(nothing_to_undo(), wt, None);
    assert_eq!(
        without,
        t("undo.reason.nothing-to-undo", &[]),
        "TMC010-CLI-TEXT: without --since the text of today stays"
    );
}

#[test]
fn an_invalid_period_gets_its_own_text() {
    let wt = Path::new("/w/feat-login");
    let err = ClientError::Rpc(ErrorObject::new(
        code::INVALID_PARAMS,
        "since: a duration such as 20m",
    ));
    assert_eq!(
        failure_text(err, wt, Some("veinte")),
        t("undo.invalid-since", &[]),
        "TMC010-CLI-TEXT: INVALID_PARAMS on --since explains the accepted durations"
    );
}
