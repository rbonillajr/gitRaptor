//! V1 (ADR-GRP-017, TS-DSK-001 T003): the normal dependencies of `gitraptor-desktop` never reach the
//! engine (`gitraptor-core`, `gitraptor-git`, `gitraptor-policy`), do reach `gitraptor-api` with the
//! `client` feature, and the crate is a workspace member but not a default member.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

const DESKTOP: &str = "gitraptor-desktop";
const FORBIDDEN: &[&str] = &["gitraptor-core", "gitraptor-git", "gitraptor-policy"];

fn metadata() -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml");
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
        .expect("V1: cargo metadata must run");
    assert!(
        out.status.success(),
        "V1: cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn names_by_id(meta: &Value) -> HashMap<String, String> {
    meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn id_of(names: &HashMap<String, String>, name: &str) -> String {
    names
        .iter()
        .find(|(_, n)| n.as_str() == name)
        .unwrap_or_else(|| panic!("V1: package {name} is not in the workspace"))
        .0
        .clone()
}

/// Node ids reachable from `gitraptor-desktop` through normal (kind == null) dependencies only.
fn reachable_normal(meta: &Value, root: &str) -> HashSet<String> {
    let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["id"].as_str().unwrap(), n))
        .collect();
    let mut seen = HashSet::new();
    let mut stack = vec![root.to_string()];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for dep in nodes[id.as_str()]["deps"].as_array().unwrap() {
            let normal = dep["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k["kind"].is_null());
            if normal {
                stack.push(dep["pkg"].as_str().unwrap().to_string());
            }
        }
    }
    seen
}

#[test]
fn v1_normal_dependencies_do_not_reach_the_engine() {
    let meta = metadata();
    let names = names_by_id(&meta);
    let reached: Vec<&str> = reachable_normal(&meta, &id_of(&names, DESKTOP))
        .iter()
        .map(|id| names[id].as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .filter(|n| FORBIDDEN.contains(n))
        .collect();
    assert!(
        reached.is_empty(),
        "V1: gitraptor-desktop reaches {reached:?}"
    );
}

#[test]
fn v1_reaches_gitraptor_api_with_the_client_feature() {
    let meta = metadata();
    let names = names_by_id(&meta);
    let api = id_of(&names, "gitraptor-api");
    assert!(
        reachable_normal(&meta, &id_of(&names, DESKTOP)).contains(&api),
        "V1: gitraptor-api is not a normal dependency"
    );
    let node = meta["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == api.as_str())
        .unwrap();
    let features: Vec<&str> = node["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert!(
        features.contains(&"client"),
        "V1: gitraptor-api lacks the client feature: {features:?}"
    );
}

#[test]
fn v1_crate_is_a_member_but_not_a_default_member() {
    let meta = metadata();
    let names = names_by_id(&meta);
    let id = id_of(&names, DESKTOP);
    let has = |key: &str| {
        meta[key]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == id.as_str())
    };
    assert!(
        has("workspace_members"),
        "V1: gitraptor-desktop must be a workspace member"
    );
    assert!(
        !has("workspace_default_members"),
        "V1: gitraptor-desktop must not be in default-members"
    );
}

/// D8-NOTIF (DS-US-DSK-008 D14): no OS notifications and no sound, in the crates, the webview
/// dependencies or the webview code.
#[test]
fn no_os_notifications() {
    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for manifest in ["src-tauri/Cargo.toml", "package.json"] {
        let text = std::fs::read_to_string(desktop.join(manifest)).expect("D8-NOTIF: manifest");
        assert!(
            !text.contains("tauri-plugin-notification") && !text.contains("plugin-notification"),
            "D8-NOTIF: {manifest} depends on the notification plugin"
        );
    }
    let mut stack = vec![desktop.join("src")];
    let mut offenders = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("D8-NOTIF: src folder") {
            let path = entry.expect("D8-NOTIF: entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_source = path
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("ts" | "tsx")));
            let is_test = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains(".test."));
            if !is_source || is_test {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("D8-NOTIF: source");
            if text.contains("new Notification(") || text.contains("requestPermission") {
                offenders.push(path.display().to_string());
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "D8-NOTIF: the webview asks for OS notifications in {offenders:?}"
    );
}
