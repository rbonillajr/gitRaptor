//! The contract a client of protocol 5 to 8 sees, pinned before the
//! registry moved to one file per module (ADR-GRP-016 § 1, Validation):
//! what the handshake offers for each protocol and profile, and the shapes
//! each protocol implies. These clients exist and are never asked to change.

use std::collections::BTreeSet;

use gitraptor_api::capability::{CAPABILITIES_PROTOCOL, implied};
use gitraptor_api::methods::{METHODS, MethodSpec};

/// What `hello` offers a connection of `protocol` (the daemon's rule in
/// `conn.rs`: every method of the protocol but `hello`, MCP only its own).
fn offered(protocol: u32, mcp: bool) -> BTreeSet<&'static str> {
    METHODS
        .iter()
        .filter(|m: &&MethodSpec| m.name != "hello" && m.exists_in(protocol) && (!mcp || m.mcp))
        .map(|m| m.name)
        .collect()
}

const BASE_FULL: &[&str] = &[
    "ping",
    "engine.snapshot",
    "engine.resources",
    "events.subscribe",
    "events.unsubscribe",
    "events.history",
    "sessions.list",
    "audit.list",
    "daemon.stop",
    "daemon.replace",
    "repo.add",
    "repo.retire",
    "attribution.correct",
    "attribution.withdraw-correction",
    "registration.register",
    "registration.withdraw",
    "operation.describe",
    "operation.prepare",
    "operation.run",
    "operation.cancel",
    "requester.resolve",
    "timemachine.snapshot",
    "timemachine.undo",
    "timemachine.redo",
    "timemachine.restore",
    "timemachine.timeline",
];

const COCKPIT: &[&str] = &["scope.snapshot", "scope.subscribe", "repo.locate"];

const GUARD: &[&str] = &[
    "guard.plan",
    "guard.install",
    "guard.decline",
    "guard.status",
    "guard.evaluate",
];

const MCP: &[&str] = &[
    "ping",
    "engine.snapshot",
    "events.subscribe",
    "events.unsubscribe",
    "daemon.replace",
    "registration.register",
    "operation.describe",
    "operation.prepare",
    "operation.run",
    "requester.resolve",
    "timemachine.undo",
];

fn set(parts: &[&[&'static str]]) -> BTreeSet<&'static str> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

#[test]
fn every_legacy_protocol_keeps_its_methods() {
    assert_eq!(offered(5, false), set(&[BASE_FULL]));
    assert_eq!(offered(6, false), set(&[BASE_FULL, COCKPIT]));
    assert_eq!(offered(7, false), set(&[BASE_FULL, COCKPIT, GUARD]));
    assert_eq!(offered(8, false), set(&[BASE_FULL, COCKPIT, GUARD]));
    for protocol in 5..=8 {
        assert_eq!(offered(protocol, true), set(&[MCP]), "{protocol}");
    }
}

/// Protocol 9 adds `connection.accept`, for every profile, and nothing else
/// that a connection of 8 could miss.
#[test]
fn protocol_9_adds_only_the_capability_handshake() {
    let p = CAPABILITIES_PROTOCOL;
    assert_eq!(
        offered(p, false),
        set(&[BASE_FULL, COCKPIT, GUARD, &["connection.accept"]])
    );
    assert_eq!(offered(p, true), set(&[MCP, &["connection.accept"]]));
}

/// The three shapes that came with a protocol number: the requester in
/// the handshake (6) and Git events of kind `reset` (8).
#[test]
fn every_legacy_protocol_keeps_its_shapes() {
    let none = BTreeSet::new();
    assert_eq!(implied(5), none);
    assert_eq!(implied(6), BTreeSet::from(["connection.requester"]));
    assert_eq!(implied(7), BTreeSet::from(["connection.requester"]));
    let all = BTreeSet::from(["connection.requester", "events.git-reset"]);
    assert_eq!(implied(8), all);
    assert_eq!(implied(CAPABILITIES_PROTOCOL), all);
}
