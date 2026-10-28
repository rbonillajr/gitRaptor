//! US-MCP-009 S-03 (DS-009 T007): limits per requester on the channel. The daemon runs in a child
//! process of this test; the "agent" is a copy of this test binary named `raptor-fake-agent`
//! (the only name the daemon takes for an agent), and the "developer" is this binary in a
//! terminal through `script`. Every process is driven by `role_entry` (env `RAPTOR_T_LIM`).
//! Temporary profile and home only (NFR-01); waits end on files with a deadline.
//!
//! Wire facts used: the 9th connection of one agent answers `hello` with `LIMIT_REACHED` and
//! `data {"reason": "requester-connections"}`; the write bucket of a requester is shared by all
//! its connections (`MCP_WRITE_BURST` = 5 per key, taken before the scope check, so a call to
//! `operation.run` of an unknown plan on a daemon without executor counts).
//!
//! Not covered here: `an_executor_descendant_counts_for_the_plan_requester` and the hook-client
//! half of `the_developer_and_the_hook_client_take_no_agent_slot` (see the report).
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

const ENV: &str = "RAPTOR_T_LIM";
const DEADLINE: Duration = Duration::from_secs(30);
const AGENT_NAME: &str = "raptor-fake-agent";
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

fn publish(out: &Path, value: &Value) {
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(value).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

fn read_json(path: &Path) -> Value {
    wait_for(path);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// `{"ok": true}` or the refusal `{"code", "data"}` of a connection attempt.
fn attempt(dirs: &ProfileDirs, kind: ClientKind) -> (Option<Client>, Value) {
    match Client::connect(dirs, kind, PROTOCOL_VERSION) {
        Ok(c) => (Some(c), json!({"ok": true})),
        Err(ClientError::Rpc(e)) => (None, json!({"code": e.code, "data": e.data})),
        Err(e) => (None, json!({"other": e.to_string()})),
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
        // `n` connections held; the answers of each attempt.
        "ninth" | "developer" => {
            let n = spec["n"].as_u64().unwrap() as usize;
            let mut held = Vec::new();
            let mut answers = Vec::new();
            for _ in 0..n {
                let (c, a) = attempt(&dirs, ClientKind::Cli);
                held.extend(c);
                answers.push(a);
            }
            publish(&out, &json!(answers));
            if let Some(release) = spec["release"].as_str() {
                wait_for(Path::new(release));
            }
        }
        // 8 held, then the CLI the agent launches connects once.
        "cli_parent" => {
            let mut held = Vec::new();
            for _ in 0..8 {
                held.extend(attempt(&dirs, ClientKind::Cli).0);
            }
            let child_out = out.with_extension("child");
            let child_spec =
                json!({"role": "ninth", "n": 1, "root": spec["root"], "out": child_out});
            let status = Command::new(spec["orig_exe"].as_str().unwrap())
                .args(["role_entry", "--exact", "--nocapture", "--test-threads=1"])
                .env(ENV, child_spec.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
            publish(&out, &read_json(&child_out));
        }
        // 8 held, one dropped, then a new connection is retried until it fits.
        "free" => {
            let mut held: Vec<Client> = Vec::new();
            for _ in 0..8 {
                held.extend(attempt(&dirs, ClientKind::Cli).0);
            }
            let ninth = attempt(&dirs, ClientKind::Cli);
            held.pop();
            let start = Instant::now();
            let after = loop {
                let (c, a) = attempt(&dirs, ClientKind::Cli);
                held.extend(c);
                if a["ok"] == true || start.elapsed() > Duration::from_secs(10) {
                    break a;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            publish(&out, &json!({"before": ninth.1, "after": after}));
        }
        // Two MCP connections of the agent alternate calls that are writes.
        "bucket" => {
            let (a, ra) = attempt(&dirs, ClientKind::Mcp);
            let (b, rb) = attempt(&dirs, ClientKind::Mcp);
            let mut conns: Vec<Client> = a.into_iter().chain(b).collect();
            let mut codes = Vec::new();
            if conns.len() == 2 {
                for i in 0..2 * WRITE_BURST {
                    let r = conns[i % 2].call::<_, Value>(
                        methods::OPERATION_RUN,
                        json!({"plan_id": "plan-unknown"}),
                    );
                    codes.push(match r {
                        Ok(_) => json!(0),
                        Err(ClientError::Rpc(e)) => json!(e.code),
                        Err(e) => json!(e.to_string()),
                    });
                }
            }
            publish(&out, &json!({"connect": [ra, rb], "codes": codes}));
        }
        other => panic!("unknown role {other}"),
    }
}

fn daemon(spec: &Value) {
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let channel = ChannelConfig {
        // Room above the per-requester cap, so only S-03 can stop the 9th connection.
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

struct Rig {
    tp: TempProfile,
    daemon: Child,
    stop: PathBuf,
    agent_exe: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.stop, b"");
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

fn entry(exe: &Path, spec: &Value) -> Command {
    let mut c = Command::new(exe);
    c.args(["role_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV, spec.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

fn rig() -> Rig {
    let tp = TempProfile::new();
    let root = tp.root.path().join("profile");
    let stop = tp.root.path().join("stop-daemon");
    let exe = std::env::current_exe().unwrap();
    let bin = tp.root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let agent_exe = bin.join(AGENT_NAME);
    std::fs::copy(&exe, &agent_exe).unwrap();
    let spec = json!({"role": "daemon", "root": root, "out": stop});
    let daemon = entry(&exe, &spec).spawn().unwrap();
    let rig = Rig {
        tp,
        daemon,
        stop,
        agent_exe,
    };
    let start = Instant::now();
    while Client::connect(&rig.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
        assert!(start.elapsed() < DEADLINE, "the daemon never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    rig
}

impl Rig {
    fn root(&self) -> PathBuf {
        self.tp.root.path().join("profile")
    }

    /// Runs `role` as the agent (the copy named `raptor-fake-agent`) and reads its answer.
    fn as_agent(&self, role: &str, extra: Value) -> Value {
        let out = self.tp.root.path().join(format!("{role}-agent.json"));
        let mut spec = json!({
            "role": role, "root": self.root(), "out": out,
            "orig_exe": std::env::current_exe().unwrap(),
        });
        spec.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut child = entry(&self.agent_exe, &spec).spawn().unwrap();
        let answer = read_json(&out);
        let _ = child.kill();
        let _ = child.wait();
        answer
    }
}

fn is_refused(answer: &Value) -> bool {
    answer["code"] == code::LIMIT_REACHED && answer["data"]["reason"] == "requester-connections"
}

#[test]
fn a_ninth_connection_of_the_same_agent_is_refused() {
    let rig = rig();
    let answers = rig.as_agent("ninth", json!({"n": 9}));
    let answers = answers.as_array().unwrap();
    for (i, a) in answers.iter().take(8).enumerate() {
        assert_eq!(
            a["ok"],
            true,
            "connection {} of one agent must be served: {a}",
            i + 1
        );
    }
    assert!(
        is_refused(&answers[8]),
        "the 9th connection of the agent must get LIMIT_REACHED with reason requester-connections: {}",
        answers[8]
    );
}

#[test]
fn two_connections_of_one_agent_share_the_write_bucket() {
    let rig = rig();
    let r = rig.as_agent("bucket", json!({}));
    assert_eq!(r["connect"], json!([{"ok": true}, {"ok": true}]), "{r}");
    let codes = r["codes"].as_array().unwrap();
    let limited = codes.iter().filter(|c| **c == code::RATE_LIMITED).count();
    assert!(
        limited >= WRITE_BURST,
        "ten writes alternating on two connections of one agent must exhaust the shared burst of {WRITE_BURST} \
         (RATE_LIMITED from the 6th), got {limited} limited: {codes:?}"
    );
    assert_ne!(
        codes[0],
        json!(code::RATE_LIMITED),
        "the first write is within the burst: {codes:?}"
    );
}

#[test]
fn the_cli_launched_by_the_agent_counts_in_its_key() {
    let rig = rig();
    let child = rig.as_agent("cli_parent", json!({}));
    let child = &child.as_array().unwrap()[0];
    assert!(
        is_refused(child),
        "a CLI launched by the agent, with its 8 connections open, counts in the agent's key \
         and must get LIMIT_REACHED requester-connections: {child}"
    );
}

#[test]
fn the_developer_and_the_hook_client_take_no_agent_slot() {
    let rig = rig();
    // The developer: this binary in a terminal through `script`, 10 connections held.
    let out = rig.tp.root.path().join("developer.json");
    let release = rig.tp.root.path().join("developer-release");
    let spec = json!({
        "role": "developer", "root": rig.root(), "out": out, "n": 10, "release": release,
    });
    let exe = std::env::current_exe().unwrap();
    let args = ["role_entry", "--exact", "--nocapture", "--test-threads=1"];
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/script");
        c.args(["-q", "/dev/null"]).arg(&exe).args(args);
        c
    } else {
        let line = std::iter::once(exe.to_string_lossy().into_owned())
            .chain(args.iter().map(|s| (*s).to_owned()))
            .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
            .collect::<Vec<_>>()
            .join(" ");
        let mut c = Command::new("script");
        c.args(["-q", "-e", "-c", &line, "/dev/null"]);
        c
    };
    let mut developer = command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", rig.tp.root.path())
        .env(ENV, spec.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let developer_answers = read_json(&out);
    for (i, a) in developer_answers.as_array().unwrap().iter().enumerate() {
        assert_eq!(
            a["ok"],
            true,
            "developer connection {} must be served: {a}",
            i + 1
        );
    }
    // With the developer's 10 connections open, the agent still gets its 8 and no more.
    let agent = rig.as_agent("ninth", json!({"n": 9}));
    let agent = agent.as_array().unwrap();
    let served = agent.iter().filter(|a| a["ok"] == true).count();
    let _ = std::fs::write(&release, b"");
    let _ = developer.wait();
    assert_eq!(
        served, 8,
        "the developer's connections take no agent slot: the agent must get exactly 8: {agent:?}"
    );
    assert!(
        is_refused(&agent[8]),
        "the agent's 9th connection must get LIMIT_REACHED requester-connections: {}",
        agent[8]
    );
}

#[test]
fn a_closed_connection_frees_its_slot() {
    let rig = rig();
    let r = rig.as_agent("free", json!({}));
    assert!(
        is_refused(&r["before"]),
        "the 9th connection must be refused while 8 are open: {r}"
    );
    assert_eq!(
        r["after"]["ok"], true,
        "closing one connection must free its slot for a new one: {r}"
    );
}
