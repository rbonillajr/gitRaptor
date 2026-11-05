//! Review of `raptor undo --since` texts: the confirmation of a period says how many operations
//! it takes back and how many owners when the list is cut, the undo of the last operation keeps
//! its text, and the rejections of a period speak of the period. Child module of `undo`.

use super::*;
use crate::i18n::{has_key, text_in};
use gitraptor_api::rpc::ErrorObject;

fn confirm(reason: &str, extra: serde_json::Value) -> TmConfirmData {
    let mut data = json!({
        "reason": reason,
        "undone_operation_id": "op-b",
        "undone_subtype": gitraptor_api::Untrusted::new("script"),
    });
    if let (Some(data), Some(extra)) = (data.as_object_mut(), extra.as_object()) {
        data.extend(extra.clone());
    }
    serde_json::from_value(data).expect("TmConfirmData")
}

fn rejected(reason: &str) -> ClientError {
    ClientError::Rpc(
        ErrorObject::new(code::OPERATION_REJECTED, "rejected")
            .with_data(json!({ "reason": reason })),
    )
}

#[test]
fn the_new_texts_exist_in_en_and_es_with_the_same_markers() {
    for key in [
        "undo.confirm-plan-since",
        "undo.reason.raw-git-not-covered-since",
        "undo.reason.target-unavailable-since",
    ] {
        assert!(has_key(key), "{key}");
        let en = text_in(false, key).expect(key);
        let es = text_in(true, key).expect(key);
        assert!(en.contains("{since}") && es.contains("{since}"), "{key}");
    }
}

#[test]
fn the_confirmation_of_the_last_operation_keeps_its_text() {
    let data = confirm("confirmation-required", json!({}));
    assert_eq!(
        plan_text(&data, None),
        t(
            "undo.confirm-plan",
            &[("operation", &"script"), ("id", &"op-b"), ("owners", &"")],
        )
    );
}

#[test]
fn the_confirmation_of_a_period_counts_operations_and_cut_owners() {
    let data = confirm(
        "confirmation-required",
        json!({ "undone_count": 3, "owners_total": 10 }),
    );
    let text = plan_text(&data, Some("20m"));
    let more = t("undo.overlap.more", &[("count", &10)]);
    assert_eq!(
        text,
        t(
            "undo.confirm-plan-since",
            &[
                ("count", &3),
                ("since", &"20m"),
                ("operation", &"script"),
                ("id", &"op-b"),
                ("owners", &more),
            ],
        )
    );
}

#[test]
fn the_rejections_of_a_period_speak_of_the_period() {
    let wt = Path::new("/w/feat-login");
    assert_eq!(
        failure_text(rejected("raw-git-not-covered"), wt, Some("1h")),
        t("undo.reason.raw-git-not-covered-since", &[("since", &"1h")])
    );
    assert_eq!(
        failure_text(rejected("target-unavailable"), wt, Some("1h")),
        t("undo.reason.target-unavailable-since", &[("since", &"1h")])
    );
    assert_eq!(
        failure_text(rejected("raw-git-not-covered"), wt, None),
        t("undo.reason.raw-git-not-covered", &[])
    );
}

#[test]
fn another_invalid_field_is_not_taken_for_the_period() {
    let wt = Path::new("/w/feat-login");
    let err = ClientError::Rpc(ErrorObject::new(
        code::INVALID_PARAMS,
        "worktree: not an absolute path",
    ));
    assert_ne!(
        failure_text(err, wt, Some("20m")),
        t("undo.invalid-since", &[])
    );
}
