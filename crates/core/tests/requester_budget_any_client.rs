//! The read and write buckets of a requester key are spent by every connection that holds a slot
//! in it, whatever the client: the CLI an agent runs spends the same write burst as its MCP
//! server. The daemon runs in a child process; the "agent" is a copy of this test binary named
//! `raptor-fake-agent`. Temporary profile and home only; waits end on files with a deadline.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig, ChannelLimits};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_core::profile::ProfileDirs;
use serde_json::{Value, json};

const ENV: &str = "RAPTOR_T_BUDGET";
const DEADLINE: Duration = Duration::from_secs(30);
const AGENT_NAME: &str = "raptor-fake-agent";
/// `MCP_WRITE_BURST`: the writes one requester key may make at once.
const WRITE_BURST: usize = 5;

fn dirs_of(root: &str) -> ProfileDirs {
    ProfileDirs::under_root(PathBuf::from(root))
}

fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < DEADLINE, "never appeared: {path:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The code of a write: `0` when served, the RPC code otherwise.
fn write_code(client: &mut Client) -> Value {
    match client.call::<_, Value>(methods::OPERATION_RUN, json!({"plan_id": "plan-unknown"})) {
        Ok(_) => json!(0),
        Err(ClientError::Rpc(e)) => json!(e.code),
        Err(e) => json!(e.to_string()),
    }
}

/// Every process of the test is this binary with a role.
#[test]
fn role_entry() {
    let Some(spec) = std::env::var_os(ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let dirs = dirs_of(spec["root"].as_str().unwrap());
    match spec["role"].as_str().unwrap() {
        "daemon" => daemon(&spec),
        // The agent's CLI (a non-MCP connection in its key) writes past the burst; then its MCP
        // server writes once.
        "cli_writes" => {
            let mut cli = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
            let cli_codes: Vec<Value> = (0..=WRITE_BURST).map(|_| write_code(&mut cli)).collect();
            let mut mcp = Client::connect(&dirs, ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
            let mcp_code = write_code(&mut mcp);
            let answer = json!({"cli": cli_codes, "mcp": mcp_code});
            let tmp = out.with_extension("tmp");
            std::fs::write(&tmp, serde_json::to_vec(&answer).unwrap()).unwrap();
            std::fs::rename(tmp, &out).unwrap();
        }
        other => panic!("unknown role {other}"),
    }
}

fn daemon(spec: &Value) {
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let channel = ChannelConfig {
        limits: ChannelLimits {
            max_connections: 64,
            per_client: 64,
            ..ChannelLimits::default()
        },
        agents: AgentMatcher::only(vec![AGENT_NAME.into()]),
        ..ChannelConfig::default()
    };
    let config = DaemonConfig {
        dirs: dirs_of(spec["root"].as_str().unwrap()),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel,
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let stop = PathBuf::from(spec["out"].as_str().unwrap());
    std::thread::spawn(move || {
        while !stop.exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
        handle.request(StopCause::Signal("TERM"));
    });
    daemon.run();
}

fn entry(exe: &Path, spec: &Value) -> Command {
    let mut c = Command::new(exe);
    c.args(["role_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV, spec.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// Stops the daemon when the test ends, however it ends.
struct Stop {
    daemon: Child,
    file: PathBuf,
}

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.file, b"");
        let start = Instant::now();
        while self.daemon.try_wait().ok().flatten().is_none() {
            if start.elapsed() > DEADLINE {
                let _ = self.daemon.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn the_agents_cli_spends_the_shared_write_burst_of_its_key() {
    let tp = TempProfile::new();
    let root = tp.root.path().join("profile");
    let exe = std::env::current_exe().unwrap();
    let file = tp.root.path().join("stop-daemon");
    let spec = json!({"role": "daemon", "root": root, "out": file});
    let _stop = Stop {
        daemon: entry(&exe, &spec).spawn().unwrap(),
        file,
    };
    let start = Instant::now();
    while Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
        assert!(start.elapsed() < DEADLINE, "the daemon never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    let bin = tp.root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let agent_exe = bin.join(AGENT_NAME);
    std::fs::copy(&exe, &agent_exe).unwrap();
    let out = tp.root.path().join("agent.json");
    let spec = json!({"role": "cli_writes", "root": root, "out": out});
    let mut agent = entry(&agent_exe, &spec).spawn().unwrap();
    wait_for(&out);
    let r: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    let _ = agent.kill();
    let _ = agent.wait();

    let cli = r["cli"].as_array().unwrap();
    let limited = json!(code::RATE_LIMITED);
    assert!(
        cli[..WRITE_BURST].iter().all(|c| *c != limited),
        "the first {WRITE_BURST} writes of the agent's CLI are within the burst: {r}"
    );
    assert_eq!(
        cli[WRITE_BURST], limited,
        "the agent's CLI spends its key's write burst like its MCP server does: {r}"
    );
    assert_eq!(
        r["mcp"], limited,
        "the burst the CLI spent is the one the agent's MCP server shares: {r}"
    );
}
