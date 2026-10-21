//! `raptor-mcp` never asks the agent's client anything: it is built without the elicitation
//! feature of `rmcp`, nothing in its source sends an elicitation request, and a real session
//! never receives a request from the server. Temporary profile and folders only (NFR-01).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const MCP: &str = env!("CARGO_BIN_EXE_raptor-mcp");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .components()
        .collect()
}

/// The features Cargo resolved for `rmcp`: other crates of the workspace can switch one on
/// through feature unification, which the manifest of this crate would not show.
#[test]
fn the_server_is_built_without_elicitation() {
    let root = workspace_root();
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let rmcp_ids: Vec<&str> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["name"] == "rmcp")
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert!(!rmcp_ids.is_empty(), "rmcp is a dependency of raptor-mcp");
    let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
    let mut seen = 0;
    for node in nodes
        .iter()
        .filter(|n| n["id"].as_str().is_some_and(|id| rmcp_ids.contains(&id)))
    {
        seen += 1;
        let features: Vec<&str> = node["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            !features.contains(&"elicitation"),
            "rmcp resolves with elicitation: {features:?}"
        );
    }
    assert!(seen > 0, "rmcp is in the resolved graph");

    // The request type exists even without the feature: look for the identifiers in the code.
    let mut files = Vec::new();
    rust_files(&root.join("apps/mcp/src"), &mut files);
    assert!(!files.is_empty());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for needle in ["ElicitRequest", "create_elicitation", ".elicit("] {
            assert!(!text.contains(needle), "{} uses `{needle}`", file.display());
        }
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every line the server writes answers an id the test sent; none is a request or notification
/// of its own (those carry `method`).
#[test]
fn a_session_never_receives_a_request_from_the_server() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    let mut child = Command::new(MCP)
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", &profile)
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut send = |message: Value| {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    };
    send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {"elicitation": {}},
            "clientInfo": {"name": "claude-code", "version": "2.1.284"}
        }
    }));
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    send(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "snapshot", "arguments": {"label": ""}}
    }));
    send(json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "push", "arguments": {}}
    }));
    drop(send);
    drop(stdin);

    let mut ids = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let message: Value = serde_json::from_str(&line.unwrap()).expect("only JSON-RPC");
        assert!(message.get("method").is_none(), "server request: {message}");
        ids.push(
            message["id"]
                .as_u64()
                .expect("an answer carries a numeric id"),
        );
    }
    child.wait().unwrap();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2, 3, 4]);
    assert!(!profile.exists(), "the engine did not start");
}
