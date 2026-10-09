//! ADR-MCP-001 § 6, NFR-02: `raptor-mcp` refuses a stdin message over 1 MiB or
//! nested deeper than 32 with a stable JSON-RPC error, without dying, and then
//! serves the next message. Temporary profile only (NFR-01).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const MCP: &str = env!("CARGO_BIN_EXE_raptor-mcp");

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2.1.284"}
        }
    })
}

#[test]
fn refuses_oversized_and_too_deep_messages_and_keeps_serving() {
    let tmp = tempfile::tempdir().unwrap();
    let mut child = Command::new(MCP)
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", tmp.path().join("profile"))
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut read = || -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).expect("stdout carries only JSON-RPC")
    };

    let big = "x".repeat(50 * 1024 * 1024);
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    let mut expected = Vec::new();
    for (line, reason) in [(big, "message-too-large"), (deep, "message-too-deep")] {
        writeln!(stdin, "{line}").unwrap();
        expected.push(reason);
        let reply = read();
        assert_eq!(reply["id"], Value::Null, "{reply}");
        assert_eq!(reply["error"]["code"], -32600, "{reply}");
        assert_eq!(reply["error"]["data"]["reason"], *expected.last().unwrap());
    }

    writeln!(stdin, "{}", initialize()).unwrap();
    let reply = read();
    assert_eq!(reply["id"], 1, "{reply}");
    assert!(reply["result"]["serverInfo"].is_object(), "{reply}");

    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
}
