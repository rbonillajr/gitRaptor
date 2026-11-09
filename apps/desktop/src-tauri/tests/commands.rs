//! IPC1 (TS-DSK-001 T002, ADR-GRP-017 § 1): the envelope `{"params": {…}}` rejects unknown fields at
//! both levels, `app_info` answers the crate version and the protocol version, and the
//! `#[tauri::command]` functions of `src/commands` are exactly the list of `commands.json`.
//!
//! Assumed public items (Dev Spec § Firmas + critic round 1): `ipc::parse`, `ipc::CommandError`
//! (Serialize), `commands::app_info::{info, AppInfo}` (`info()` is the pure fn the command delegates to).

use gitraptor_desktop::commands::app_info;
use gitraptor_desktop::ipc::{CommandError, from_body, parse};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tauri::ipc::InvokeBody;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct EmptyParams {}

fn invalid_params(body: Value) {
    let got = parse::<EmptyParams>(&body);
    let err: CommandError = match got {
        Ok(_) => panic!("IPC1: {body} must be rejected with invalid-params"),
        Err(e) => e,
    };
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        json!({"code": "invalid-params"}),
        "IPC1: rejection of {body} must serialise to {{\"code\":\"invalid-params\"}}"
    );
}

#[test]
fn ipc1_empty_params_object_is_accepted() {
    assert!(
        parse::<EmptyParams>(&json!({"params": {}})).is_ok(),
        "IPC1: {{\"params\":{{}}}} must parse"
    );
}

#[test]
fn ipc1_unknown_field_inside_params_is_invalid_params() {
    invalid_params(json!({"params": {"x": 1}}));
}

#[test]
fn ipc1_unknown_field_outside_params_is_invalid_params() {
    invalid_params(json!({"params": {}, "extra": 1}));
}

#[test]
fn ipc1_missing_params_or_non_object_body_is_invalid_params() {
    invalid_params(json!({}));
    invalid_params(json!("x"));
    invalid_params(json!([]));
    invalid_params(Value::Null);
}

#[test]
fn ipc1_app_info_reports_crate_version_and_protocol() {
    let v = serde_json::to_value(app_info::info()).unwrap();
    assert_eq!(
        v,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": gitraptor_api::PROTOCOL_VERSION,
        }),
        "IPC1: app_info must serialise camelCase {{version, protocol}} from CARGO_PKG_VERSION and PROTOCOL_VERSION"
    );
}

#[test]
fn ipc1_from_body_accepts_json_envelope_and_rejects_raw_bytes() {
    let ok = from_body::<EmptyParams>(&InvokeBody::Json(json!({"params": {}})));
    assert!(
        ok.is_ok(),
        "IPC1: InvokeBody::Json({{\"params\":{{}}}}) must parse"
    );
    let err: CommandError = match from_body::<EmptyParams>(&InvokeBody::Raw(vec![1, 2, 3])) {
        Ok(_) => panic!("IPC1: InvokeBody::Raw must be rejected"),
        Err(e) => e,
    };
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        json!({"code": "invalid-params"}),
        "IPC1: a raw body must serialise to invalid-params"
    );
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    out
}

/// Names of the functions right after a `#[tauri::command]` attribute.
fn command_fns() -> Vec<String> {
    let mut names = Vec::new();
    for file in rust_files(&Path::new(ROOT).join("src/commands")) {
        let text = std::fs::read_to_string(&file)
            .unwrap()
            .replace("\r\n", "\n");
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "#[tauri::command]" {
                for next in lines.by_ref() {
                    let t = next.trim();
                    if let Some(rest) = t.strip_prefix("pub fn ").or_else(|| t.strip_prefix("fn "))
                    {
                        names.push(rest.split(['(', '<']).next().unwrap().trim().to_string());
                        break;
                    }
                }
            }
        }
    }
    names.sort();
    names
}

fn commands_json() -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(ROOT).join("commands.json"))
        .expect("IPC1: commands.json must exist");
    let mut v: Vec<String> = serde_json::from_str(&text).unwrap();
    v.sort();
    v
}

#[test]
fn ipc1_tauri_commands_match_commands_json() {
    let listed = commands_json();
    assert!(!listed.is_empty(), "IPC1: commands.json must not be empty");
    assert_eq!(
        command_fns(),
        listed,
        "IPC1: #[tauri::command] fns of src/commands must equal commands.json"
    );
}

#[test]
fn ipc1_capability_allows_each_command() {
    let text = std::fs::read_to_string(Path::new(ROOT).join("capabilities/main.json"))
        .expect("IPC1: capabilities/main.json must exist");
    let cap: Value = serde_json::from_str(&text).unwrap();
    let perms: Vec<&str> = cap["permissions"]
        .as_array()
        .expect("IPC1: capability has permissions")
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    for name in commands_json() {
        let permission = format!("allow-{}", name.replace('_', "-"));
        assert!(
            perms.contains(&permission.as_str()),
            "IPC1: capability main lacks {permission}"
        );
    }
}
