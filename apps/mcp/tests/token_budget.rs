//! RES-MCP-01: the catalog an agent loads with every session, whether it
//! uses the MCP or not, fits its token budget as it goes on the wire, in
//! English and in Spanish. Tokens are estimated as `ceil(bytes / 3)` of the
//! compact JSON (`gitraptor_api::mcp_view::estimated_tokens`); the failure
//! says which part is over and by how much. The results of the tools are
//! measured in `src/server.rs` (RES-MCP-02 and 03) and end to end in
//! `apps/cli/tests/mcp_allowlist.rs`. Temporary profile and folder only
//! (NFR-01).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use gitraptor_api::mcp_view::{catalog_overruns, estimated_tokens};
use serde_json::{Value, json};

const MCP: &str = env!("CARGO_BIN_EXE_raptor-mcp");

/// The `initialize` result and the tools of `tools/list` of one session.
fn catalog(lang: &str) -> (Value, Vec<Value>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut child = Command::new(MCP)
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", tmp.path().join("profile"))
        .env("LANG", lang)
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |message: Value| {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    };
    let mut read = || -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).expect("stdout carries only JSON-RPC")
    };
    send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2.1.284"}
        }
    }));
    let initialize = read()["result"].clone();
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = read()["result"]["tools"].as_array().unwrap().clone();
    drop(send);
    drop(stdin);
    child.wait().unwrap();
    (initialize, tools)
}

#[test]
fn the_catalog_fits_its_token_budget() {
    for lang in ["en_US.UTF-8", "es_ES.UTF-8"] {
        let (initialize, tools) = catalog(lang);
        let base = initialize.to_string();
        eprintln!(
            "RES-MCP-01 ({lang}): initialize ~{} tokens ({} B)",
            estimated_tokens(&base),
            base.len()
        );
        for tool in &tools {
            let text = tool.to_string();
            eprintln!(
                "RES-MCP-01 ({lang}): tool {} with its outputSchema ~{} tokens ({} B)",
                tool["name"],
                estimated_tokens(&text),
                text.len()
            );
        }
        let overruns = catalog_overruns(&initialize, &tools);
        assert!(overruns.is_empty(), "{lang}: {overruns:#?}");
    }
}
