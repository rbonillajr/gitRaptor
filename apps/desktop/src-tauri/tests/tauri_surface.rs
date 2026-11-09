//! V2 (ADR-GRP-017 § 2, TS-DSK-001 T003 with M-1, M-2, M-3): the Tauri surface is exactly the one of
//! the Dev Spec: CSP, hardened flags, one capability closed to `main`, no plugins, no devtools and
//! navigation handlers in `window.rs`. Uses only std + serde_json.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none'";
const DEV_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost ws://localhost:1420; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none'";

fn path(rel: &str) -> PathBuf {
    Path::new(ROOT).join(rel)
}

fn text(rel: &str) -> String {
    std::fs::read_to_string(path(rel)).unwrap_or_else(|e| panic!("V2: cannot read {rel}: {e}"))
}

fn json(rel: &str) -> Value {
    serde_json::from_str(&text(rel)).unwrap_or_else(|e| panic!("V2: {rel} is not JSON: {e}"))
}

fn conf_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(ROOT)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("tauri") && n.ends_with(".conf.json"))
        .collect();
    v.sort();
    v
}

/// Dotted paths of every leaf (non-object or empty object) of a JSON value.
fn leaf_paths(v: &Value, prefix: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, c) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                leaf_paths(c, &p, out);
            }
        }
        _ => out.push(prefix.to_string()),
    }
}

fn commands() -> Vec<String> {
    serde_json::from_value(json("commands.json")).expect("V2: commands.json must be a string array")
}

fn metadata() -> Value {
    let manifest = path("Cargo.toml");
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .expect("V2: cargo metadata must run");
    assert!(
        out.status.success(),
        "V2: cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn v2_only_the_three_known_conf_files_exist() {
    let allowed = [
        "tauri.conf.json",
        "tauri.dev.conf.json",
        "tauri.release.conf.json",
    ];
    let names = conf_names();
    assert!(
        names.contains(&"tauri.conf.json".to_string()),
        "V2: tauri.conf.json is missing"
    );
    assert!(
        names.contains(&"tauri.dev.conf.json".to_string()),
        "V2: tauri.dev.conf.json is missing"
    );
    for n in &names {
        assert!(allowed.contains(&n.as_str()), "V2: unexpected {n} (M-1)");
    }
}

#[test]
fn v2_release_conf_only_touches_external_bin_targets_and_signing_identity() {
    let p = path("tauri.release.conf.json");
    if !p.exists() {
        assert!(
            path("tauri.conf.json").exists(),
            "V2: tauri.conf.json is missing"
        );
        return;
    }
    let mut leaves = Vec::new();
    leaf_paths(&json("tauri.release.conf.json"), "", &mut leaves);
    for l in leaves {
        assert!(
            [
                "bundle.externalBin",
                "bundle.targets",
                "bundle.macOS.signingIdentity"
            ]
            .contains(&l.as_str()),
            "V2: tauri.release.conf.json has the key {l} (M-1)"
        );
    }
}

#[test]
fn v2_base_csp_is_exact_without_network_or_unsafe_eval() {
    let conf = json("tauri.conf.json");
    let csp = conf["app"]["security"]["csp"]
        .as_str()
        .expect("V2: app.security.csp must be a string");
    assert_eq!(
        csp, CSP,
        "V2: the CSP is not the exact one of ADR-GRP-017 § 2"
    );
    assert!(!csp.contains("unsafe-eval"), "V2: CSP contains unsafe-eval");
    for scheme in ["https://", "wss://", "ws://"] {
        assert!(!csp.contains(scheme), "V2: CSP contains {scheme}");
    }
    assert_eq!(
        csp.matches("http://").count(),
        1,
        "V2: the only http origin allowed is http://ipc.localhost"
    );
}

#[test]
fn v2_dev_overlay_has_the_exact_dev_csp_and_dev_url() {
    let dev = json("tauri.dev.conf.json");
    let dev_csp = dev["app"]["security"]["devCsp"]
        .as_str()
        .expect("V2: app.security.devCsp must be a string");
    assert_eq!(dev_csp, DEV_CSP, "V2: devCsp is not the exact one");
    assert!(
        !dev_csp.contains("unsafe-eval"),
        "V2: devCsp contains unsafe-eval"
    );
    assert_eq!(
        dev["build"]["devUrl"], "http://localhost:1420",
        "V2: devUrl of the overlay"
    );
}

#[test]
fn v2_base_flags_are_hardened_and_without_dev_settings() {
    let conf = json("tauri.conf.json");
    let sec = &conf["app"]["security"];
    assert_eq!(conf["app"]["withGlobalTauri"], false, "V2: withGlobalTauri");
    assert_eq!(sec["freezePrototype"], true, "V2: freezePrototype");
    assert_eq!(
        sec["assetProtocol"]["enable"], false,
        "V2: assetProtocol.enable"
    );
    assert_eq!(
        sec["dangerousDisableAssetCspModification"], false,
        "V2: dangerousDisableAssetCspModification"
    );
    assert!(
        conf["build"].get("devUrl").is_none(),
        "V2: the base config must not have build.devUrl"
    );
    assert!(
        sec.get("devCsp").is_none(),
        "V2: the base config must not have devCsp"
    );
    assert_eq!(
        conf["app"]["windows"],
        serde_json::json!([]),
        "V2: app.windows must be empty (window.rs creates it)"
    );
    assert_eq!(
        sec["capabilities"],
        serde_json::json!(["main"]),
        "V2: capabilities must be exactly [\"main\"] (M-2)"
    );
    assert!(
        conf["bundle"].get("externalBin").is_none(),
        "V2: the base must not declare bundle.externalBin"
    );
}

#[test]
fn v2_no_plugins_file_associations_or_deep_links_in_any_conf() {
    for name in conf_names() {
        let raw = text(&name);
        let conf: Value = serde_json::from_str(&raw).unwrap();
        let plugins_empty = conf
            .get("plugins")
            .is_none_or(|p| p.as_object().is_some_and(|o| o.is_empty()));
        assert!(plugins_empty, "V2: {name} has plugins");
        assert!(
            !raw.contains("fileAssociations"),
            "V2: {name} has fileAssociations"
        );
        assert!(!raw.contains("deep-link"), "V2: {name} has deep-link");
    }
    assert!(!conf_names().is_empty(), "V2: no tauri conf found");
}

#[test]
fn v2_single_capability_closed_to_main_with_only_own_commands() {
    let dir = path("capabilities");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("V2: cannot read capabilities/: {e}"))
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        files.len(),
        1,
        "V2: exactly one capability file, found {files:?}"
    );
    let cap = json("capabilities/main.json");
    assert_eq!(cap["identifier"], "main");
    assert_eq!(
        cap["windows"],
        serde_json::json!(["main"]),
        "V2: capability windows"
    );
    for key in ["remote", "webviews", "platforms"] {
        assert!(cap.get(key).is_none(), "V2: capability has {key} (M-2)");
    }
    let got: BTreeSet<String> = cap["permissions"]
        .as_array()
        .expect("V2: permissions array")
        .iter()
        .map(|p| {
            p.as_str()
                .expect("V2: permissions must be plain strings")
                .to_string()
        })
        .collect();
    let mut want: BTreeSet<String> = commands()
        .iter()
        .map(|c| format!("allow-{}", c.replace('_', "-")))
        .collect();
    want.insert("core:window:allow-start-dragging".to_string());
    assert_eq!(
        got, want,
        "V2: permissions must be core:window:allow-start-dragging plus allow-<command>"
    );
}

#[test]
fn v2_no_unlisted_plugins_and_no_devtools() {
    let meta = metadata();
    let pkg = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "gitraptor-desktop")
        .expect("V2: gitraptor-desktop is not in the workspace");
    for dep in pkg["dependencies"].as_array().unwrap() {
        let name = dep["name"].as_str().unwrap();
        assert!(
            !name.starts_with("tauri-plugin-") || name == "tauri-plugin-dialog",
            "V2: dependency {name} is not allowed (only tauri-plugin-dialog)"
        );
    }
    let tauri = meta["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"].as_str().unwrap().contains("#tauri@"))
        .expect("V2: tauri is not resolved");
    let features: Vec<&str> = tauri["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert!(
        !features.contains(&"devtools"),
        "V2: tauri enables devtools: {features:?}"
    );

    let package = json("../package.json");
    for section in ["dependencies", "devDependencies"] {
        if let Some(map) = package[section].as_object() {
            for name in map.keys() {
                assert!(
                    !name.starts_with("@tauri-apps/plugin-"),
                    "V2: package.json depends on {name}"
                );
            }
        }
    }
}

#[test]
fn v2_window_has_content_protection_and_navigation_handlers() {
    let window = text("src/window.rs");
    for needle in [
        "content_protected(true)",
        "on_navigation(",
        "on_new_window(",
    ] {
        assert!(window.contains(needle), "V2: src/window.rs lacks {needle}");
    }
}

#[test]
fn v2_lib_registers_only_the_dialog_plugin() {
    let lib = text("src/lib.rs");
    assert!(
        lib.contains("generate_context!"),
        "V2: src/lib.rs must run the Tauri context"
    );
    for (i, _) in lib.match_indices(".plugin(") {
        assert!(
            lib[i..].starts_with(".plugin(tauri_plugin_dialog::init())"),
            "V2: lib.rs calls .plugin( with something other than tauri_plugin_dialog::init()"
        );
    }
}
