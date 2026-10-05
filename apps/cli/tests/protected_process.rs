//! TS-TMC-004 at process level: who the real daemon says is asking, with
//! the real `raptor` binary as daemon over a temporary profile
//! (`GITRAPTOR_PROFILE_DIR`, debug builds only). Never this repo nor the
//! real profile (NFR-01).
//!
//! The client is this test binary (`rpc_client_entry`), run either directly
//! or as a child of the simulated agent: a copy of this binary named
//! `raptor-fake-agent` (`fake_agent_entry`), which the daemon is told is an
//! agent through the debug-only `GITRAPTOR_AGENT_EXECUTABLES`.
//!
//! macOS only. Linux: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{RequestChannel, RequesterView, ResolvedVia};
use gitraptor_api::{Actor, AgentKind, PROTOCOL_VERSION, methods};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const RPC_ENV: &str = "RAPTOR_TEST_RPC";

/// The simulated agent: runs `RAPTOR_FAKE_AGENT_ARGV` as its child.
#[test]
fn fake_agent_entry() {
    let Some(argv) = std::env::var_os(FAKE_AGENT_ARGV) else {
        return;
    };
    let argv: Vec<String> = serde_json::from_str(argv.to_str().unwrap()).unwrap();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .env_remove(FAKE_AGENT_ARGV)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// The client: `{"root", "kind", "calls": [[method, params]]}`; prints the
/// answers as one JSON line.
#[test]
fn rpc_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let kind: ClientKind = serde_json::from_value(spec["kind"].clone()).unwrap();
    let mut client = Client::connect(&dirs, kind, PROTOCOL_VERSION).unwrap();
    let mut answers = Vec::new();
    for call in spec["calls"].as_array().unwrap() {
        answers.push(
            match client.call::<_, Value>(call[0].as_str().unwrap(), call[1].clone()) {
                Ok(v) => json!({ "ok": v }),
                Err(ClientError::Rpc(e)) => json!({ "code": e.code }),
                Err(e) => json!({ "other": e.to_string() }),
            },
        );
    }
    println!("RPC-ANSWERS {}", serde_json::to_string(&answers).unwrap());
}

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fx = Self {
            tmp: tempfile::tempdir().unwrap(),
        };
        let out = Command::new(RAPTOR)
            .args(["daemon", "status"])
            .env_clear()
            .envs(fx.env())
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        fx
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().join("profile")
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            ("GITRAPTOR_PROFILE_DIR", self.root().into_os_string()),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
        ]
    }

    /// Runs the client with `calls`, directly or under the simulated agent.
    fn ask(&self, kind: ClientKind, calls: Value, under_agent: bool) -> Vec<Value> {
        let me = std::env::current_exe().unwrap();
        let client_argv = [
            me.to_string_lossy().into_owned(),
            "rpc_client_entry".into(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ];
        let spec = json!({ "root": self.root(), "kind": kind, "calls": calls });
        let mut cmd = if under_agent {
            let agent = self.tmp.path().join(FAKE_AGENT);
            if !agent.exists() {
                std::fs::copy(&me, &agent).unwrap();
            }
            let mut cmd = Command::new(agent);
            cmd.args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .env(
                FAKE_AGENT_ARGV,
                serde_json::to_string(&client_argv).unwrap(),
            );
            cmd
        } else {
            let mut cmd = Command::new(&client_argv[0]);
            cmd.args(&client_argv[1..]).env_clear();
            cmd
        };
        let out = cmd
            .envs(self.env())
            .env("PATH", "/usr/bin:/bin")
            .env(RPC_ENV, spec.to_string())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stdout
            .lines()
            .find_map(|l| l.split_once("RPC-ANSWERS ").map(|(_, j)| j.to_owned()))
            .unwrap_or_else(|| {
                panic!(
                    "no answers: {stdout} {}",
                    String::from_utf8_lossy(&out.stderr)
                )
            });
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let dirs = ProfileDirs::under_root(self.root());
        if let Ok(Some(pid)) = running_pid(&dirs.state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

fn is_claude(actor: &Actor) -> bool {
    matches!(
        actor,
        Actor::Agent {
            kind: AgentKind::ClaudeCode,
            ..
        }
    )
}

/// ADR-TMC-005 § 1: by CLI and by MCP, a client under a (simulated) Claude
/// Code is that agent; resolved in the daemon, never "human".
#[test]
fn a_client_under_an_agent_is_that_agent_by_cli_and_mcp() {
    let fx = Fixture::new();
    let resolve = json!([[methods::REQUESTER_RESOLVE, {}]]);

    let cli = fx.ask(ClientKind::Cli, resolve.clone(), true);
    let view: RequesterView = serde_json::from_value(cli[0]["ok"].clone()).unwrap();
    assert!(is_claude(&view.actor), "{view:?}");
    assert_eq!(view.via, ResolvedVia::Ancestry);
    assert_eq!(view.channel, RequestChannel::Cli);
    assert!(!view.confirmable);

    let mcp = fx.ask(ClientKind::Mcp, resolve, true);
    assert!(is_claude(
        &serde_json::from_value(mcp[0]["ok"]["actor"].clone()).unwrap()
    ));
    assert_eq!(mcp[0]["ok"]["channel"], "mcp");
}

/// Without an agent in its ancestry the client is "unattributed".
#[test]
fn a_client_without_an_agent_is_unattributed() {
    let fx = Fixture::new();
    let answers = fx.ask(
        ClientKind::Cli,
        json!([[methods::REQUESTER_RESOLVE, { "surface": "tui" }]]),
        false,
    );
    let view: RequesterView = serde_json::from_value(answers[0]["ok"].clone()).unwrap();
    assert_eq!(view.actor, Actor::Unattributed);
    assert_eq!(view.channel, RequestChannel::Tui);
    assert!(!answers[0].to_string().to_lowercase().contains("human"));
}

/// A client that declares itself another agent or a human is refused, and
/// what it is stays what the daemon resolved.
#[test]
fn a_declared_identity_changes_nothing() {
    let fx = Fixture::new();
    let answers = fx.ask(
        ClientKind::Mcp,
        json!([
            [methods::REQUESTER_RESOLVE, { "actor": { "actor": "unattributed" } }],
            [methods::TM_UNDO, { "human": true }],
            [methods::REQUESTER_RESOLVE, {}],
            // Attributed over MCP: undo passes the requester check and stops
            // at its story (no executor in the production daemon).
            [methods::TM_UNDO, { "since": "2h" }],
        ]),
        true,
    );
    assert_eq!(answers[0]["code"], code::INVALID_PARAMS);
    assert_eq!(answers[1]["code"], code::INVALID_PARAMS);
    assert!(is_claude(
        &serde_json::from_value(answers[2]["ok"]["actor"].clone()).unwrap()
    ));
    assert_eq!(answers[3]["code"], code::NOT_IMPLEMENTED);

    // Unattributed over MCP, the same undo is refused (TQ-7 → a).
    let answers = fx.ask(
        ClientKind::Mcp,
        json!([[methods::TM_UNDO, { "since": "2h" }]]),
        false,
    );
    assert_eq!(answers[0]["code"], code::SCOPE_REFUSED);
}
