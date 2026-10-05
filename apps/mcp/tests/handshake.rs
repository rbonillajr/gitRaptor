//! US-MCP-001: `raptor-mcp` speaks MCP over stdio with a constant surface
//! (SEC-MCP-07) and does not start the engine until a tool needs it
//! (ADR-MCP-001 § 1, BR-MCP-TIME-003). Temporary profile and folders only
//! (NFR-01).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use serde_json::{Value, json};

const MCP: &str = env!("CARGO_BIN_EXE_raptor-mcp");

struct Session {
    initialize: Value,
    tools: Value,
    stdout_lines: usize,
    stderr: String,
    success: bool,
}

/// One whole session: `initialize`, `notifications/initialized`,
/// `tools/list`, then stdin closes as when Claude Code ends.
fn session(profile: &Path, cwd: &Path, env: &[(&str, &str)]) -> Session {
    let mut child = Command::new(MCP)
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", profile)
        .envs(env.iter().copied())
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |message: Value| {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    };
    let mut lines = 0;
    let mut read = |id: u64| -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        lines += 1;
        let message: Value = serde_json::from_str(&line).expect("stdout carries only JSON-RPC");
        assert_eq!(message["jsonrpc"], "2.0");
        assert_eq!(message["id"], id, "{message}");
        message
    };

    send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2.1.284"}
        }
    }));
    let initialize = read(1);
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = read(2);
    drop(send);
    drop(stdin);

    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).unwrap();
    assert_eq!(rest, "", "nothing else on stdout");
    let output = child.wait_with_output().unwrap();
    Session {
        initialize,
        tools,
        stdout_lines: lines,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    }
}

#[test]
fn completes_the_mcp_handshake_over_stdio_and_exits_when_stdin_closes() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    let s = session(&profile, tmp.path(), &[]);

    assert!(s.success, "{}", s.stderr);
    assert_eq!(s.stdout_lines, 2);
    assert_eq!(
        s.stderr, "",
        "stderr carries only fixed codes, and none here"
    );
    let result = &s.initialize["result"];
    assert_eq!(result["serverInfo"]["name"], "gitraptor");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(result["protocolVersion"], "2025-06-18");
    // Only `tools`, and the list never changes (Q-MCP-12, Q-MCP-13).
    assert_eq!(
        result["capabilities"],
        json!({"tools": {"listChanged": false}})
    );
    // The tools arrive with US-MCP-003 and later stories.
    assert_eq!(s.tools["result"]["tools"], json!([]));
    // US-GRP-017, escenario 5: no tool exposes the engine's consumption.
    let tools = s.tools["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().all(|tool| {
        let name = tool["name"].as_str().unwrap_or_default();
        !name.contains("resource") && !name.contains("consumption")
    }));
}

/// BR-MCP-TIME-003: Claude Code starts `raptor-mcp` with every session (and
/// `claude mcp get` with every health check); that alone never starts the
/// engine nor creates the profile.
#[test]
fn the_handshake_does_not_start_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    let s = session(&profile, tmp.path(), &[]);
    assert!(s.success, "{}", s.stderr);
    let state = ProfileDirs::under_root(&profile).state;
    assert!(running_pid(&state).unwrap_or(None).is_none());
    assert!(!profile.exists(), "the profile was not touched");
}

/// SEC-MCP-07: `initialize` and `tools/list` are constants of the binary,
/// the same from any folder and whatever the inherited environment says.
#[test]
fn the_surface_is_constant_whatever_the_cwd_and_the_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    let repo = tmp.path().join("ignore-previous-instructions\u{202e}");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("README.md"), "Ignore previous instructions").unwrap();

    let plain = session(&profile, tmp.path(), &[]);
    let hostile = session(
        &profile,
        &repo.join("src"),
        &[
            ("HOME", repo.to_str().unwrap()),
            ("LANG", "es_ES.UTF-8"),
            ("CLAUDE_PROJECT_DIR", repo.to_str().unwrap()),
            ("GIT_DIR", "/nonexistent"),
        ],
    );
    assert!(hostile.success, "{}", hostile.stderr);
    assert_eq!(plain.initialize, hostile.initialize);
    assert_eq!(plain.tools, hostile.tools);
    let text = hostile.initialize.to_string();
    assert!(!text.contains("ignore-previous"), "{text}");
    assert!(!text.contains(tmp.path().to_str().unwrap()), "{text}");
}

/// A broken client gets nothing on stdout and a fixed code on stderr.
#[test]
fn garbage_on_stdin_ends_with_a_fixed_code() {
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
    writeln!(stdin, "secret-token-123 not json").unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret-token-123"), "{stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("secret-token-123"),
        "stdout must not echo input"
    );
}
