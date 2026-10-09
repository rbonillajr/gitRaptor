//! The wire contract of the requester's confirmation (ADR-TMC-005 § 3): the shape of the
//! rejection that carries the challenge, the token parameter and its format, the stable code of
//! a bad token and the capability that gates all of it. Dedicated file: the criterion's tests
//! must not live in a production file.

use gitraptor_api::Untrusted;
use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::capability;
use gitraptor_api::messages::RefusalReason;
use gitraptor_api::methods::CAP_TM_CONFIRMATION;
use gitraptor_api::timemachine::{
    MAX_CONFIRM_OWNERS, RestoreParams, TmChallenge, TmConfirmData, TmRejectReason, TmRejectedData,
    UndoParams,
};
use serde_json::{Value, json};

fn agent() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn full() -> TmConfirmData {
    TmConfirmData {
        reason: TmRejectReason::ConfirmationRequired,
        operation_id: Some("op-1".into()),
        challenge: Some(TmChallenge {
            token: "0123456789abcdef0123456789abcdef".into(),
            expires_in_ms: 60_000,
        }),
        cannot_confirm: Some(RefusalReason::NoControllingTerminal),
        owners: vec![agent()],
        undone_operation_id: Some("op-0".into()),
        undone_subtype: Some(Untrusted::new("reset")),
    }
}

#[test]
fn confirm_data_round_trips_and_is_a_superset() {
    let data = full();
    let json = serde_json::to_value(&data).unwrap();
    assert_eq!(serde_json::from_value::<TmConfirmData>(json).unwrap(), data);

    // Any rejection of the old shape parses as the new one.
    let old = TmRejectedData {
        reason: TmRejectReason::OtherActor,
        operation_id: Some("op-9".into()),
    };
    let parsed: TmConfirmData =
        serde_json::from_value(serde_json::to_value(&old).unwrap()).unwrap();
    assert_eq!(parsed.reason, TmRejectReason::OtherActor);
    assert_eq!(parsed.operation_id.as_deref(), Some("op-9"));
    assert!(parsed.challenge.is_none() && parsed.owners.is_empty());

    // Without the new fields it serializes exactly like the old shape.
    let plain = TmConfirmData::rejected(TmRejectReason::ChallengeInvalid, Some("op-2".into()));
    let json = serde_json::to_value(&plain).unwrap();
    let object = json.as_object().unwrap();
    let mut keys: Vec<_> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["operation_id", "reason"]);
    let none = TmConfirmData::rejected(TmRejectReason::ConfirmationRequired, None);
    assert_eq!(
        serde_json::to_value(&none).unwrap(),
        serde_json::to_value(TmRejectedData {
            reason: TmRejectReason::ConfirmationRequired,
            operation_id: None,
        })
        .unwrap()
    );

    // A field nobody declared is refused.
    let mut json = serde_json::to_value(&data).unwrap();
    json["surprise"] = json!(1);
    assert!(serde_json::from_value::<TmConfirmData>(json).is_err());
}

#[test]
fn the_challenge_debug_hides_the_token() {
    let shown = format!("{:?}", full());
    assert!(!shown.contains("0123456789abcdef"), "{shown}");
    assert!(shown.contains("60000"), "{shown}");
}

#[test]
fn the_confirmation_token_is_validated() {
    let undo = |token: &str| {
        serde_json::from_value::<UndoParams>(json!({ "confirmation": token }))
            .unwrap()
            .validate()
    };
    let restore = |token: &str| {
        serde_json::from_value::<RestoreParams>(json!({
            "snapshot_id": "00000000-0000-0000-0000-000000000001",
            "confirmation": token,
        }))
        .unwrap()
        .validate()
    };
    for ok in [
        "0123456789abcdef0123456789abcdef",
        "0123456789ABCDEF0123456789ABCDEF",
    ] {
        assert!(undo(ok).is_ok(), "{ok}");
        assert!(restore(ok).is_ok(), "{ok}");
    }
    for bad in [
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "0123456789abcdef0123456789abcdeg",
        "",
        "xyz",
    ] {
        assert_eq!(undo(bad).unwrap_err().field, "confirmation", "{bad:?}");
        assert_eq!(restore(bad).unwrap_err().field, "confirmation", "{bad:?}");
    }

    // Absent: never serialized, not even as null.
    let absent: Value =
        serde_json::to_value(serde_json::from_value::<UndoParams>(json!({})).unwrap()).unwrap();
    assert!(absent.get("confirmation").is_none(), "{absent}");
    let absent: Value = serde_json::to_value(
        serde_json::from_value::<RestoreParams>(
            json!({ "snapshot_id": "00000000-0000-0000-0000-000000000001" }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(absent.get("confirmation").is_none(), "{absent}");
}

#[test]
fn challenge_invalid_has_a_stable_code() {
    for (reason, code) in [
        (TmRejectReason::ChallengeInvalid, "challenge-invalid"),
        (
            TmRejectReason::ConfirmationUnavailable,
            "confirmation-unavailable",
        ),
    ] {
        assert_eq!(serde_json::to_value(reason).unwrap(), json!(code));
        assert_eq!(
            serde_json::from_value::<TmRejectReason>(json!(code)).unwrap(),
            reason
        );
    }
}

#[test]
fn the_confirmation_capability_is_declared() {
    assert_eq!(CAP_TM_CONFIRMATION.name, "timemachine.confirmation");
    assert!(capability::all().any(|c| c.name == "timemachine.confirmation"));
    assert_eq!(MAX_CONFIRM_OWNERS, 8);
}
