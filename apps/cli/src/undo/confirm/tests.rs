//! Unit tests of the confirmation helpers of the commands.

use std::collections::BTreeSet;

use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::{TmChallenge, TmConfirmData, TmRejectReason};
use serde_json::json;

use super::{CannotConfirm, cannot_kind, resend_params, should_ask};
use crate::i18n::text_in;

/// The `{name}` markers of a message, in no order.
fn markers(text: &str) -> BTreeSet<String> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
        .collect()
}

fn data(reason: TmRejectReason, with_challenge: bool) -> TmConfirmData {
    TmConfirmData {
        challenge: with_challenge.then(|| TmChallenge {
            token: "0123456789abcdef0123456789abcdef".into(),
            expires_in_ms: 60_000,
        }),
        ..TmConfirmData::rejected(reason, None)
    }
}

#[test]
fn every_cannot_confirm_reason_has_a_kind() {
    use CannotConfirm::{Agent, Terminal, Unverified, Windows};
    use RefusalReason as R;
    for (reason, kind) in [
        (R::Unsupported, Windows),
        (R::NoControllingTerminal, Terminal),
        (R::AgentAncestry, Agent),
        (R::SessionLeaderAgent, Agent),
        (R::DaemonDescendant, Unverified),
        (R::IdentityUnverified, Unverified),
        (R::NotAvailableToMcp, Unverified),
        (R::WorktreeMismatch, Unverified),
        (R::AgentMismatch, Unverified),
    ] {
        assert_eq!(cannot_kind(reason), kind, "{reason:?}");
    }
}

#[test]
fn the_repeated_request_only_adds_the_token() {
    let params = json!({ "worktree": "/w", "surface": "cli" });
    let token = "0123456789abcdef0123456789abcdef";
    let again = resend_params(&params, token);
    assert_eq!(
        again,
        json!({ "worktree": "/w", "surface": "cli", "confirmation": token })
    );
    // The original is untouched: the first request stays what it was.
    assert!(params.get("confirmation").is_none());
}

#[test]
fn every_confirmation_message_exists_in_both_languages() {
    let keys = [
        "tmconfirm.ask",
        "tmconfirm.declined",
        "tmconfirm.needs-terminal",
        "undo.confirm-plan",
        "undo.reason.confirmation-required",
        "undo.reason.confirmation-not-asked",
        "undo.reason.confirmation-unavailable-windows",
        "undo.reason.confirmation-needs-terminal",
        "undo.reason.confirmation-agent",
        "undo.reason.confirmation-unverified",
        "undo.reason.challenge-invalid",
    ];
    for key in keys {
        let en = text_in(false, key).unwrap_or_else(|| panic!("{key} is missing in English"));
        let es = text_in(true, key).unwrap_or_else(|| panic!("{key} is missing in Spanish"));
        assert_eq!(markers(en), markers(es), "{key}: same markers in both");
        if key.starts_with("undo.reason.confirmation-")
            && key != "undo.reason.confirmation-required"
        {
            assert!(en.contains("belongs to an agent"), "{key}: {en}");
        }
    }
    assert_eq!(
        markers(text_in(false, "undo.confirm-plan").unwrap_or_default()),
        BTreeSet::from(["id".to_owned(), "operation".to_owned(), "owners".to_owned()])
    );
}

#[test]
fn json_mode_never_asks() {
    let with_challenge = data(TmRejectReason::ConfirmationRequired, true);
    assert!(!should_ask(false, &with_challenge));
    assert!(should_ask(true, &with_challenge));
    // Nothing to ask without a challenge, or for another reason.
    assert!(!should_ask(
        true,
        &data(TmRejectReason::ConfirmationRequired, false)
    ));
    assert!(!should_ask(
        true,
        &data(TmRejectReason::ChallengeInvalid, true)
    ));
}
