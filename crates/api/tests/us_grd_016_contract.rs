//! US-GRD-016 (DS T005, T007, T009): the wire contract of the four protection states, the
//! `guard.protection-layers` capability, the `layer` filter of `guard.log` and the
//! `mcp-allowlist` log entry. Driven with JSON and strings, so this file compiles before the new
//! variants, fields and constants exist.

use gitraptor_api::guard::{GuardLogParams, LoggedOperation, ProtectionState};
use gitraptor_api::methods::GROUPS;
use serde_json::{Value, json};

/// GRD016-API-STATES: the four values of ADR-GRD-005 are read and written in kebab-case.
#[test]
fn the_four_protection_states_round_trip() {
    for name in ["unprotected", "hooks-only", "mcp-only", "full"] {
        let state: ProtectionState = serde_json::from_value(json!(name)).unwrap_or_else(|e| {
            panic!("ProtectionState must admit `{name}` (the four states are missing): {e}")
        });
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            json!(name),
            "ProtectionState `{name}` must be written back as the same string"
        );
    }
}

/// GRD016-API-CAP: the capability that carries the four states is declared by the guard group.
#[test]
fn protection_layers_capability_is_declared() {
    let guard = GROUPS
        .iter()
        .find(|g| g.module == "guard")
        .expect("the guard group");
    assert!(
        guard
            .capabilities
            .iter()
            .any(|c| c.name == "guard.protection-layers"),
        "the guard group must declare the capability `guard.protection-layers`: {:?}",
        guard
            .capabilities
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
    );
}

/// GRD016-API-LOG: `layer` filters the log; the allowlist transition is a log entry.
#[test]
fn the_log_takes_a_layer_and_an_allowlist_entry() {
    for layer in ["hooks", "mcp", "cockpit"] {
        let wire = json!({"path": "/r", "layer": layer});
        let params: GuardLogParams = serde_json::from_value(wire.clone())
            .unwrap_or_else(|e| panic!("GuardLogParams must take `layer: {layer}`: {e}"));
        assert_eq!(
            serde_json::to_value(&params).unwrap(),
            wire,
            "GuardLogParams must keep `layer: {layer}`"
        );
    }
    let without: Value = serde_json::to_value(
        serde_json::from_value::<GuardLogParams>(json!({"path": "/r"})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        without,
        json!({"path": "/r"}),
        "an absent layer is not written"
    );

    let entry = json!({"kind": "mcp-allowlist", "from": "hooks-only", "to": "full"});
    let op: LoggedOperation = serde_json::from_value(entry.clone()).unwrap_or_else(|e| {
        panic!("LoggedOperation must admit the `mcp-allowlist` entry with from and to: {e}")
    });
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        entry,
        "the `mcp-allowlist` entry must keep from and to"
    );
}
