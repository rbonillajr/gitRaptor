//! IPC1 (TS-DSK-001 T002, ADR-GRP-017 § 1): the envelope `{"params": {…}}` rejects unknown fields at
//! both levels, `app_info` answers the crate version and the protocol version, and the
//! `#[tauri::command]` functions of `src/commands` are exactly the list of `commands.json`.
//!
//! Assumed public items (Dev Spec § Firmas + critic round 1): `ipc::parse`, `ipc::CommandError`
//! (Serialize), `commands::app_info::{info, AppInfo}` (`info()` is the pure fn the command delegates to).

use gitraptor_desktop::commands::app_info;
use gitraptor_desktop::commands::engine_retry::EngineRetryParams;
use gitraptor_desktop::commands::engine_watch::EngineWatchParams;
use gitraptor_desktop::commands::fleet_open::FleetOpenParams;
use gitraptor_desktop::commands::guard_blocks::GuardBlocksParams;
use gitraptor_desktop::engine::link::{ConnectFailure, Connector, Link};
use gitraptor_desktop::engine::{EngineHub, NotWatching};
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

// ---------------------------------------------------------------- D3-CMDS (DS-US-DSK-003 T005)

fn rejects<P: serde::de::DeserializeOwned>(body: Value) {
    let got = parse::<P>(&body);
    let err: CommandError = match got {
        Ok(_) => panic!("D3-CMDS: {body} must be rejected with invalid-params"),
        Err(e) => e,
    };
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        json!({"code": "invalid-params"}),
        "D3-CMDS: rejection of {body} must serialise to invalid-params"
    );
}

#[test]
fn d3_cmds_engine_commands_are_listed_and_allowed() {
    assert_eq!(
        commands_json(),
        [
            "app_info",
            "engine_retry",
            "engine_watch",
            "fleet_open",
            "guard_blocks"
        ],
        "D3-CMDS: commands.json is exactly app_info, engine_watch, engine_retry, fleet_open and guard_blocks"
    );
    let text = std::fs::read_to_string(Path::new(ROOT).join("capabilities/main.json")).unwrap();
    let cap: Value = serde_json::from_str(&text).unwrap();
    let perms: Vec<&str> = cap["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    for permission in ["allow-engine-watch", "allow-engine-retry"] {
        assert!(
            perms.contains(&permission),
            "D3-CMDS: capability main lacks {permission}"
        );
    }
}

#[test]
fn d3_cmds_engine_watch_takes_a_channel_and_rejects_unknown_fields() {
    let channel = json!("__CHANNEL__:1");
    assert!(
        parse::<EngineWatchParams>(&json!({"params": {"channel": channel}})).is_ok(),
        "D3-CMDS: {{\"params\":{{\"channel\":\"__CHANNEL__:1\"}}}} must parse"
    );
    rejects::<EngineWatchParams>(json!({"params": {"channel": channel, "x": 1}}));
    rejects::<EngineWatchParams>(json!({"params": {"channel": channel}, "extra": 1}));
    rejects::<EngineWatchParams>(json!({"params": {}}));
}

struct NoEngine;

impl Connector for NoEngine {
    fn connect(&mut self, _start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        Err(ConnectFailure::NotRunning)
    }
}

#[test]
fn d3_cmds_engine_retry_needs_start_rejects_unknown_fields_and_is_not_watching_alone() {
    for start in [true, false] {
        let ok = parse::<EngineRetryParams>(&json!({"params": {"start": start}}));
        assert!(
            matches!(ok, Ok(ref p) if p.start == start),
            "D3-CMDS: {{\"params\":{{\"start\":{start}}}}} must parse"
        );
    }
    rejects::<EngineRetryParams>(json!({"params": {"start": true, "x": 1}}));
    rejects::<EngineRetryParams>(json!({"params": {"start": true}, "extra": 1}));
    rejects::<EngineRetryParams>(json!({"params": {}}));
    rejects::<EngineRetryParams>(json!({"params": {"start": "yes"}}));

    let hub = EngineHub::new(Box::new(NoEngine), None);
    assert_eq!(
        hub.retry(true),
        Err(NotWatching),
        "D3-CMDS: engine_retry without a supervisor is not-watching"
    );
    assert_eq!(
        serde_json::to_value(CommandError::NotWatching).unwrap(),
        json!({"code": "not-watching"}),
        "D3-CMDS: NotWatching serialises as not-watching"
    );
}

// ---------------------------------------------------------------- D4-CMD (DS-US-DSK-004 T002)

#[test]
fn fleet_open_is_in_the_closed_list_and_the_capability() {
    assert_eq!(
        commands_json(),
        [
            "app_info",
            "engine_retry",
            "engine_watch",
            "fleet_open",
            "guard_blocks"
        ],
        "D4-CMD: commands.json is the three of US-DSK-003 plus fleet_open and guard_blocks"
    );
    assert!(
        command_fns().iter().any(|c| c == "fleet_open"),
        "D4-CMD: src/commands has a #[tauri::command] fn fleet_open"
    );
    let text = std::fs::read_to_string(Path::new(ROOT).join("capabilities/main.json")).unwrap();
    let cap: Value = serde_json::from_str(&text).unwrap();
    let perms: Vec<&str> = cap["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert!(
        perms.contains(&"allow-fleet-open"),
        "D4-CMD: capability main lacks allow-fleet-open"
    );
}

/// What `fleet_open` decides about `body` before it reaches the supervisor: the envelope and the
/// id are checked either when the params are read or when the hub is asked to open, never later.
fn fleet_open_verdict(body: &Value) -> Result<(), CommandError> {
    let params = parse::<FleetOpenParams>(body)?;
    let hub = EngineHub::new(Box::new(NoEngine), None);
    hub.open(params.repo_id).map_err(|e| match e {
        CommandError::InvalidParams => CommandError::InvalidParams,
        CommandError::NotWatching => CommandError::NotWatching,
    })
}

#[test]
fn fleet_open_accepts_params_and_rejects_bad_ids() {
    // A good id gets past the checks: with no supervisor the only thing left is not-watching.
    assert_eq!(
        fleet_open_verdict(&json!({"params": {"repoId": "tienda"}})),
        Err(CommandError::NotWatching),
        "D4-CMD: {{\"params\":{{\"repoId\":\"tienda\"}}}} must be accepted"
    );
    assert_eq!(
        fleet_open_verdict(&json!({"params": {"repoId": "a".repeat(128)}})),
        Err(CommandError::NotWatching),
        "D4-CMD: an id of exactly 128 bytes is accepted"
    );
    let invalid = [
        json!({"params": {"repoId": "tienda", "x": 1}}),
        json!({"params": {"repoId": "tienda"}, "extra": 1}),
        json!({"params": {}}),
        json!({"params": {"repoId": 7}}),
        json!({"params": {"repoId": ""}}),
        json!({"params": {"repoId": "a".repeat(129)}}),
        // 65 two-byte characters: 130 bytes, only 65 chars.
        json!({"params": {"repoId": "é".repeat(65)}}),
        json!({"params": {"repoId": "ti\u{0}enda"}}),
        json!({"params": {"repoId": "ti\nenda"}}),
        json!({"params": {"repoId": "ti\u{1b}]0;x\u{7}enda"}}),
        json!({"params": {"repoId": "ti\u{7f}enda"}}),
    ];
    for body in invalid {
        assert_eq!(
            fleet_open_verdict(&body),
            Err(CommandError::InvalidParams),
            "D4-CMD: {body} must be rejected with invalid-params"
        );
    }
}

#[test]
fn fleet_open_without_a_supervisor_is_not_watching() {
    assert!(
        command_fns().iter().any(|c| c == "fleet_open"),
        "D4-CMD: there is a #[tauri::command] fn fleet_open to answer not-watching"
    );
    let hub = EngineHub::new(Box::new(NoEngine), None);
    assert_eq!(
        hub.open("tienda".to_owned()),
        Err(CommandError::NotWatching),
        "D4-CMD: fleet_open without a supervisor is not-watching"
    );
    assert_eq!(
        serde_json::to_value(CommandError::NotWatching).unwrap(),
        json!({"code": "not-watching"}),
        "D4-CMD: NotWatching serialises as not-watching"
    );
}

/// D8-CMD (DS-US-DSK-008 D1, I-01): the predicted-conflict detail travels by the `Channel` the
/// supervisor already has, so no command name is about predictions, pairs or conflicts.
#[test]
fn the_prediction_detail_adds_no_ipc_command() {
    let about = |name: &String| {
        ["prediction", "pair", "conflict"]
            .iter()
            .any(|w| name.contains(w))
    };
    let listed: Vec<String> = commands_json().into_iter().filter(about).collect();
    assert!(
        listed.is_empty(),
        "D8-CMD: commands.json names a prediction command: {listed:?}"
    );
    let declared: Vec<String> = command_fns().into_iter().filter(about).collect();
    assert!(
        declared.is_empty(),
        "D8-CMD: a #[tauri::command] is about predictions: {declared:?}"
    );
}

/// D2-CMD (DS-US-DSK-005 D2): the graph travels by the `Channel` the supervisor already has, so no
/// command name is about the graph or its lanes. It does not pin the absolute list: the tests
/// above and below do.
#[test]
fn the_graph_adds_no_ipc_command() {
    let about = |name: &String| ["graph", "lane"].iter().any(|w| name.contains(w));
    let listed: Vec<String> = commands_json().into_iter().filter(about).collect();
    assert!(
        listed.is_empty(),
        "D2-CMD: commands.json names a graph command: {listed:?}"
    );
    let declared: Vec<String> = command_fns().into_iter().filter(about).collect();
    assert!(
        declared.is_empty(),
        "D2-CMD: a #[tauri::command] is about the graph: {declared:?}"
    );
}

// ---------------------------------------------------------------- D7-WIRE (DS-US-DSK-007 T002)

#[test]
fn guard_blocks_is_in_the_closed_list_and_the_capability() {
    assert_eq!(
        commands_json(),
        [
            "app_info",
            "engine_retry",
            "engine_watch",
            "fleet_open",
            "guard_blocks"
        ],
        "D7-WIRE: commands.json is exactly the five commands, with guard_blocks (no relaxed 'contains')"
    );
    assert_eq!(
        command_fns(),
        commands_json(),
        "D7-WIRE: the #[tauri::command] fns of src/commands are exactly the list"
    );
    let text = std::fs::read_to_string(Path::new(ROOT).join("capabilities/main.json")).unwrap();
    let cap: Value = serde_json::from_str(&text).unwrap();
    let perms: Vec<&str> = cap["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert!(
        perms.contains(&"allow-guard-blocks"),
        "D7-WIRE: capability main lacks allow-guard-blocks"
    );
    let handler = std::fs::read_to_string(Path::new(ROOT).join("src/commands/mod.rs")).unwrap();
    assert!(
        handler.contains("guard_blocks::guard_blocks"),
        "D7-WIRE: the invoke handler must register guard_blocks::guard_blocks"
    );
}

/// What `guard_blocks` decides about `body` before it reaches the supervisor.
fn guard_blocks_verdict(body: &Value) -> Result<(), CommandError> {
    let params = parse::<GuardBlocksParams>(body)?;
    let hub = EngineHub::new(Box::new(NoEngine), None);
    hub.guard_blocks(params.repo_id)
}

#[test]
fn guard_blocks_accepts_params_and_rejects_bad_ids() {
    assert!(
        commands_json().iter().any(|c| c == "guard_blocks"),
        "D7-WIRE: guard_blocks is in commands.json, so its params are reachable from the webview"
    );
    assert_eq!(
        guard_blocks_verdict(&json!({"params": {"repoId": "tienda"}})),
        Err(CommandError::NotWatching),
        "D7-WIRE: {{\"params\":{{\"repoId\":\"tienda\"}}}} must be accepted"
    );
    assert_eq!(
        guard_blocks_verdict(&json!({"params": {"repoId": "a".repeat(128)}})),
        Err(CommandError::NotWatching),
        "D7-WIRE: an id of exactly 128 bytes is accepted"
    );
    let invalid = [
        json!({"params": {"repoId": "tienda", "x": 1}}),
        json!({"params": {"repoId": "tienda"}, "extra": 1}),
        json!({"params": {}}),
        json!({"params": {"repoId": 7}}),
        json!({"params": {"repoId": ""}}),
        json!({"params": {"repoId": "a".repeat(129)}}),
        json!({"params": {"repoId": "é".repeat(65)}}),
        json!({"params": {"repoId": "ti\u{0}enda"}}),
        json!({"params": {"repoId": "ti\nenda"}}),
        json!({"params": {"repoId": "ti\u{1b}]0;x\u{7}enda"}}),
        json!({"params": {"repoId": "ti\u{7f}enda"}}),
    ];
    for body in invalid {
        assert_eq!(
            guard_blocks_verdict(&body),
            Err(CommandError::InvalidParams),
            "D7-WIRE: {body} must be rejected with invalid-params"
        );
    }
}

#[test]
fn guard_blocks_without_a_supervisor_is_not_watching() {
    assert!(
        command_fns().iter().any(|c| c == "guard_blocks")
            && commands_json().iter().any(|c| c == "guard_blocks"),
        "D7-WIRE: guard_blocks is a listed #[tauri::command] that can answer not-watching"
    );
    let hub = EngineHub::new(Box::new(NoEngine), None);
    assert_eq!(
        hub.guard_blocks("tienda".to_owned()),
        Err(CommandError::NotWatching),
        "D7-WIRE: guard_blocks without a supervisor is not-watching"
    );
}

/// D7-WIRE: the guard log travels by the `Channel` of the supervisor, so no command is about the
/// log, the status or the exception, and none receives a path.
#[test]
fn the_guard_log_adds_no_ipc_command_other_than_guard_blocks() {
    let about = |name: &String| {
        ["guard", "exception", "audit"]
            .iter()
            .any(|w| name.contains(w))
    };
    let listed: Vec<String> = commands_json().into_iter().filter(about).collect();
    assert_eq!(
        listed,
        ["guard_blocks"],
        "D7-WIRE: the only guard command is guard_blocks (never guard_exception, guard_cancel…)"
    );
}
