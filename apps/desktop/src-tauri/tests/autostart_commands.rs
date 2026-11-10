//! DSK002-CMDS (TS-DSK-002 T007): the two autostart commands are in the closed list, the handler and
//! the capability, and the capability grants no `dialog:*` to the webview; `EngineAutostartParams`
//! requires `enable` and rejects unknown fields; `CommandError` serialises `not-offered`, `cancelled`
//! and `failed`; `lib.rs` registers `tauri_plugin_dialog::init()`; `Cargo.toml` pins
//! `tauri-plugin-dialog =2.8.3`.
//!
//! Assumed public items (plan § Firmas): `commands::engine_autostart::EngineAutostartParams { enable }`
//! (`deny_unknown_fields`, `camelCase`), `CommandError::{NotOffered, Cancelled, Failed}`.

use std::path::Path;

use gitraptor_desktop::commands::engine_autostart::EngineAutostartParams;
use gitraptor_desktop::commands::engine_autostart_offer::OfferReply;
use gitraptor_desktop::engine::autostart::{NotOfferedReason, Offer};
use gitraptor_desktop::ipc::{CommandError, parse};
use serde_json::{Value, json};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn text(rel: &str) -> String {
    std::fs::read_to_string(Path::new(ROOT).join(rel))
        .unwrap_or_else(|e| panic!("DSK002-CMDS: cannot read {rel}: {e}"))
        .replace("\r\n", "\n")
}

fn json_of(rel: &str) -> Value {
    serde_json::from_str(&text(rel))
        .unwrap_or_else(|e| panic!("DSK002-CMDS: {rel} is not JSON: {e}"))
}

#[test]
fn dsk002_cmds_params_require_enable_and_reject_unknown_fields() {
    for enable in [true, false] {
        let ok = parse::<EngineAutostartParams>(&json!({"params": {"enable": enable}}));
        assert!(
            matches!(ok, Ok(ref p) if p.enable == enable),
            "DSK002-CMDS: {{\"params\":{{\"enable\":{enable}}}}} must parse"
        );
    }
    for body in [
        json!({"params": {}}),
        json!({"params": {"enable": true, "x": 1}}),
        json!({"params": {"enable": true}, "extra": 1}),
        json!({"params": {"enable": "yes"}}),
        json!({"params": {"Enable": true}}),
        json!({}),
    ] {
        let got = parse::<EngineAutostartParams>(&body);
        assert!(
            matches!(got, Err(CommandError::InvalidParams)),
            "DSK002-CMDS: {body} must be rejected with invalid-params"
        );
    }
}

#[test]
fn dsk002_cmds_the_new_errors_serialise_as_their_kebab_code() {
    for (error, code) in [
        (CommandError::NotOffered, "not-offered"),
        (CommandError::Cancelled, "cancelled"),
        (CommandError::Failed, "failed"),
    ] {
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "code": code }),
            "DSK002-CMDS: the error must serialise as {{\"code\":\"{code}\"}}"
        );
    }
}

#[test]
fn dsk002_cmds_both_commands_are_listed_in_the_handler_and_the_capability() {
    let listed: Vec<String> = serde_json::from_value(json_of("commands.json")).unwrap();
    let handler = text("src/commands/mod.rs");
    let capability = json_of("capabilities/main.json");
    let permissions: Vec<&str> = capability["permissions"]
        .as_array()
        .expect("DSK002-CMDS: capability has permissions")
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    for (name, permission) in [
        ("engine_autostart_offer", "allow-engine-autostart-offer"),
        ("engine_autostart", "allow-engine-autostart"),
    ] {
        assert!(
            listed.iter().any(|c| c == name),
            "DSK002-CMDS: commands.json lacks {name}"
        );
        assert!(
            handler.contains(&format!("{name}::{name}")),
            "DSK002-CMDS: the invoke handler must register {name}::{name}"
        );
        assert!(
            permissions.contains(&permission),
            "DSK002-CMDS: capability main lacks {permission}"
        );
    }
    assert_eq!(
        listed.len(),
        7,
        "DSK002-CMDS: commands.json is the seven commands, got {listed:?}"
    );
}

#[test]
fn dsk002_cmds_the_capability_grants_no_dialog_permission_to_the_webview() {
    let capability = text("capabilities/main.json");
    assert!(
        !capability.contains("dialog"),
        "DSK002-CMDS: the capability must not contain any dialog:* permission (the dialog is the backend's)"
    );
}

#[test]
fn dsk002_cmds_lib_registers_the_dialog_plugin() {
    assert!(
        text("src/lib.rs").contains(".plugin(tauri_plugin_dialog::init())"),
        "DSK002-CMDS: lib.rs must register .plugin(tauri_plugin_dialog::init())"
    );
}

#[test]
fn dsk002_cmds_cargo_pins_the_dialog_plugin() {
    let workspace = std::fs::read_to_string(Path::new(ROOT).join("../../../Cargo.toml"))
        .expect("DSK002-CMDS: workspace Cargo.toml must exist");
    assert!(
        workspace
            .lines()
            .any(|l| l.starts_with("tauri-plugin-dialog") && l.contains("\"=2.8.3\"")),
        "DSK002-CMDS: the workspace Cargo.toml must pin tauri-plugin-dialog = \"=2.8.3\""
    );
    assert!(
        text("Cargo.toml")
            .lines()
            .any(|l| l.starts_with("tauri-plugin-dialog")),
        "DSK002-CMDS: gitraptor-desktop must depend on tauri-plugin-dialog"
    );
}

/// Source of `src/commands/engine_autostart.rs` without whitespace, so formatting does not matter.
fn autostart_source() -> (String, String) {
    let raw = text("src/commands/engine_autostart.rs");
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    (raw, compact)
}

#[test]
fn dsk002_cmds_engine_autostart_recomputes_the_offer_itself() {
    let (_, src) = autostart_source();
    assert!(
        src.contains("run_ensure(") && src.contains(",false"),
        "DSK002-CMDS: engine_autostart must run `ensure --no-start` (run_ensure(.., false)) to recompute the offer"
    );
    assert!(
        src.contains("offer("),
        "DSK002-CMDS: engine_autostart must call autostart::offer on that report, not trust one from the webview"
    );
    assert!(
        !src.contains("offer:"),
        "DSK002-CMDS: engine_autostart must not read an offer from its params"
    );
}

#[test]
fn dsk002_cmds_engine_autostart_confirms_with_the_native_dialog_off_the_main_thread() {
    let (_, src) = autostart_source();
    assert!(
        src.contains("spawn_blocking"),
        "DSK002-CMDS: the dialog must be shown inside tauri::async_runtime::spawn_blocking"
    );
    assert!(
        src.contains("blocking_show"),
        "DSK002-CMDS: the confirm of run_choice must be the native dialog (blocking_show)"
    );
    assert!(
        src.contains("run_choice("),
        "DSK002-CMDS: engine_autostart must go through run_choice"
    );
}

#[test]
fn dsk002_cmds_engine_autostart_has_no_constant_confirm() {
    let (_, src) = autostart_source();
    for constant in ["|_|true", "|_:&Prompt|true", "|_p|true", "||true"] {
        assert!(
            !src.contains(constant),
            "DSK002-CMDS: engine_autostart must not confirm with a constant ({constant}): that skips the human's yes"
        );
    }
}

#[test]
fn dsk002_cmds_the_offer_reply_serialises_its_four_combinations() {
    let cases = [
        (Offer::Offered, json!({"offer": "offered", "reason": null})),
        (
            Offer::Registered,
            json!({"offer": "registered", "reason": null}),
        ),
        (
            Offer::NotOffered {
                reason: NotOfferedReason::Location,
            },
            json!({"offer": "not-offered", "reason": "location"}),
        ),
        (
            Offer::NotOffered {
                reason: NotOfferedReason::OtherBinary,
            },
            json!({"offer": "not-offered", "reason": "other-binary"}),
        ),
    ];
    for (offer, want) in cases {
        assert_eq!(
            serde_json::to_value(OfferReply::from(&offer)).unwrap(),
            want,
            "DSK002-CMDS: OfferReply must serialise as {want}"
        );
    }
}
