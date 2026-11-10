//! TS-GRP-009 (DS-TS-GRP-009 § 9.4, file 4): the wire contract of the desktop class. The class is
//! deduced by the daemon, so `hello` does not change shape; the only new surface is the read-only
//! method `connection.desktop` and its result.

use gitraptor_api::capability::CAPABILITIES_PROTOCOL;
use gitraptor_api::desktop::{DesktopIdentity, DesktopReason, DesktopResult};
use gitraptor_api::messages::Hello;
use gitraptor_api::methods::{self, CONNECTION_DESKTOP};
use serde_json::json;

/// GRP009-WIRE: every reason and identity is written in kebab-case, and `as_str` (also the word of
/// the daemon's log) is the same word as the wire.
#[test]
fn desktop_result_wire_form() {
    let reasons = [
        (DesktopReason::PathIdentity, "path-identity"),
        (DesktopReason::NoIdentity, "no-identity"),
        (DesktopReason::AgentInAncestry, "agent-in-ancestry"),
        (DesktopReason::AmbiguousAncestry, "ambiguous-ancestry"),
        (DesktopReason::DaemonDescendant, "daemon-descendant"),
        (DesktopReason::RequesterUnverified, "requester-unverified"),
        (DesktopReason::IdentityUnverifiable, "identity-unverifiable"),
        (DesktopReason::DeclaredMcp, "declared-mcp"),
    ];
    for (reason, word) in reasons {
        assert_eq!(
            serde_json::to_value(reason).unwrap(),
            json!(word),
            "DesktopReason must be written as `{word}`"
        );
        assert_eq!(
            reason.as_str(),
            word,
            "DesktopReason::as_str must return the wire word `{word}` (the log uses it)"
        );
        let back: DesktopReason = serde_json::from_value(json!(word)).unwrap();
        assert_eq!(back, reason, "DesktopReason must read `{word}` back");
    }
    let identities = [
        (DesktopIdentity::Path, "path"),
        (DesktopIdentity::None, "none"),
        (DesktopIdentity::Unverifiable, "unverifiable"),
    ];
    for (identity, word) in identities {
        assert_eq!(
            serde_json::to_value(identity).unwrap(),
            json!(word),
            "DesktopIdentity must be written as `{word}`"
        );
        assert_eq!(
            identity.as_str(),
            word,
            "DesktopIdentity::as_str must return the wire word `{word}` (the log uses it)"
        );
    }
    let result = DesktopResult {
        reason: DesktopReason::NoIdentity,
        identity: DesktopIdentity::None,
    };
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({"reason": "no-identity", "identity": "none"}),
        "DesktopResult must be exactly {{reason, identity}}"
    );
}

/// GRP009-WIRE: a result with a field the contract does not have is rejected (frozen shape).
#[test]
fn desktop_result_rejects_unknown_fields() {
    let parsed = serde_json::from_value::<DesktopResult>(
        json!({"reason": "path-identity", "identity": "path", "extra": 1}),
    );
    assert!(
        parsed.is_err(),
        "DesktopResult must reject unknown fields, got {parsed:?}"
    );
}

/// GRP009-COMPAT: `connection.desktop` is a protocol 9 method, open to `mcp` and not reserved, so
/// the class can call it on the profile the daemon narrows it to.
#[test]
fn connection_desktop_is_open_to_mcp_from_protocol_9_and_not_reserved() {
    assert_eq!(CONNECTION_DESKTOP, "connection.desktop");
    let spec = methods::spec(CONNECTION_DESKTOP);
    assert!(
        spec.is_some(),
        "connection.desktop must be declared in the connection group (methods::spec is None)"
    );
    let spec = spec.unwrap();
    assert!(!spec.reserved, "connection.desktop must not be reserved");
    assert!(
        spec.mcp,
        "connection.desktop must be open to the mcp profile"
    );
    assert_eq!(
        spec.since, CAPABILITIES_PROTOCOL,
        "connection.desktop must be new from protocol {CAPABILITIES_PROTOCOL}"
    );
    assert!(
        !spec.exists_in(CAPABILITIES_PROTOCOL - 1),
        "a protocol 8 connection must not have connection.desktop"
    );
    assert!(
        spec.exists_in(CAPABILITIES_PROTOCOL),
        "a protocol 9 connection must have connection.desktop"
    );
}

/// GRP009-WIRE: `hello` does not change (ADR-GRP-016): a `desktop` client kind does not exist, so
/// an older daemon and this one read the same handshake.
#[test]
fn hello_is_unchanged() {
    let desktop = serde_json::from_value::<Hello>(
        json!({"protocol": 9, "client": "desktop", "client_version": "t"}),
    );
    assert!(
        desktop.is_err(),
        "Hello must not admit client `desktop`: the daemon deduces the class"
    );
    for kind in ["cli", "mcp", "other"] {
        let hello = serde_json::from_value::<Hello>(
            json!({"protocol": 9, "client": kind, "client_version": "t"}),
        );
        assert!(hello.is_ok(), "Hello must keep client `{kind}`: {hello:?}");
    }
}
