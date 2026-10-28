//! US-MCP-009 T004 through the real channel: `commit` is served only to a connection that
//! accepted `operation.commit`, its run result carries the commit, a failing step says
//! `failure: "unchanged"`, and a commit spends the connection's write bucket (guard 4.1).
//!
//! The daemon runs in this process over a temporary repo, profile and HOME (NFR-01) with the
//! production operations wiring. The "agent" is a copy of this test binary named
//! `raptor-fake-agent` that connects as an MCP client from the repo's folder (`role_entry`,
//! env `RAPTOR_T_CH`); the MCP scope comes from that working folder. Waits end on the daemon's
//! answers with a deadline.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::protected::OperationsWiring;
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const ENV: &str = "RAPTOR_T_CH";
const DEADLINE: Duration = Duration::from_secs(30);
const AGENT_NAME: &str = "raptor-fake-agent";
const WRITE_BURST: usize = 5;

// ----- Connections -----------------------------------------------------------------------------

/// A raw JSON-RPC connection that sends `hello` and nothing else: it accepts no capability.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

impl Raw {
    fn connect(dirs: &ProfileDirs, kind: &str) -> Self {
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(socket_path(dirs).expect("a socket path")) {
                Ok(s) => break s,
                Err(_) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("the daemon never accepted: {e}"),
            }
        };
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            next: 1,
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": kind, "client_version": "t"}),
        );
        assert!(hello.is_ok(), "hello: {hello:?}");
        raw
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, (i64, Value)> {
        let id = self.next;
        self.next += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        loop {
            let mut answer = String::new();
            self.reader.read_line(&mut answer).unwrap();
            let answer: Value = serde_json::from_str(&answer).unwrap();
            if answer["id"] == id {
                return match answer.get("error") {
                    Some(e) => Err((e["code"].as_i64().unwrap(), e["data"].clone())),
                    None => Ok(answer["result"].clone()),
                };
            }
        }
    }
}

// ----- The agent process -----------------------------------------------------------------------

fn dirs_of(root: &str) -> ProfileDirs {
    ProfileDirs::under_root(PathBuf::from(root))
}

fn failed(stage: &str, code: i64, data: Value) -> Value {
    json!({"stage": stage, "code": code, "data": data})
}

/// `operation.prepare` of a commit until the daemon has observed this folder's repo (the scope
/// is not refused any more); the answer, with how many calls it took.
fn prepare_when_observed(c: &mut Client, args: &Value) -> Result<Value, (i64, Value)> {
    let start = Instant::now();
    loop {
        let r = c.call::<_, Value>(
            methods::OPERATION_PREPARE,
            json!({"operation": "commit", "args": args}),
        );
        match r {
            Err(ClientError::Rpc(e))
                if e.code == code::SCOPE_REFUSED && start.elapsed() < DEADLINE =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(ClientError::Rpc(e)) => return Err((e.code, e.data.unwrap_or(Value::Null))),
            Err(e) => panic!("{e}"),
            Ok(v) => return Ok(v),
        }
    }
}

/// Every process of the test but the daemon is this binary with a role.
#[test]
fn role_entry() {
    let Some(spec) = std::env::var_os(ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = dirs_of(spec["root"].as_str().unwrap());
    let mut c = Client::connect(&dirs, ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let answer = match spec["role"].as_str().unwrap() {
        // One commit: prepare, then run.
        "commit" => match prepare_when_observed(&mut c, &spec["args"]) {
            Err((code, data)) => failed("prepare", code, data),
            Ok(plan) => match c.call::<_, Value>(
                methods::OPERATION_RUN,
                json!({"plan_id": plan["plan_id"], "accepted_warnings": plan["warnings"]}),
            ) {
                Ok(done) => json!({"stage": "run", "ok": done}),
                Err(ClientError::Rpc(e)) => failed("run", e.code, e.data.unwrap_or(Value::Null)),
                Err(e) => panic!("{e}"),
            },
        },
        // More prepares than the burst; the code of each, then a read.
        "burst" => {
            let mut codes = Vec::new();
            let first = prepare_when_observed(&mut c, &spec["args"]);
            codes.push(first.err().map_or(0, |(code, _)| code));
            for _ in 1..WRITE_BURST + 2 {
                let r = c.call::<_, Value>(
                    methods::OPERATION_PREPARE,
                    json!({"operation": "commit", "args": spec["args"]}),
                );
                codes.push(match r {
                    Ok(_) => 0,
                    Err(ClientError::Rpc(e)) => e.code,
                    Err(e) => panic!("{e}"),
                });
            }
            let read = c.call::<_, Value>(methods::OPERATION_DESCRIBE, json!({}));
            json!({"stage": "burst", "codes": codes, "read_ok": read.is_ok()})
        }
        other => panic!("unknown role {other}"),
    };
    // Each answer starts a line of its own: libtest prints "test … ... " without a newline.
    println!();
    println!("{answer}");
}

// ----- Daemon and fixture ----------------------------------------------------------------------

struct Running {
    fx: Fixture,
    tp: TempProfile,
    agent: PathBuf,
    handle: ShutdownHandle,
    join: Option<JoinHandle<gitraptor_core::daemon::StopReport>>,
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

/// A repo on `feat/x` observed and enabled for the MCP, with `x.txt` staged when `staged`, and
/// the daemon over it; an agent is a process named `raptor-fake-agent`.
fn start(staged: bool, pre_commit_rejects: bool) -> Running {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.git(&["checkout", "-q", "-b", "feat/x"]);
    if staged {
        fx.write("x.txt", "x\n");
        fx.git(&["add", "x.txt"]);
    }
    if pre_commit_rejects {
        let hook = fx.repo.join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho rejected >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tp = TempProfile::new();
    {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        profile
            .set_mcp_enabled(&entry.repo_id, true, "test", 2)
            .unwrap();
    }
    let vars = std::env::vars_os()
        .filter(|(key, _)| key != "HOME")
        .chain([("HOME".into(), fx.home.clone().into_os_string())]);
    let env = DaemonEnv::from_vars(vars);
    let channel = ChannelConfig {
        agents: AgentMatcher::only(vec![AGENT_NAME.into()]),
        ..ChannelConfig::default()
    };
    let daemon = Daemon::start(DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel,
        protected: None,
        operations: Some(OperationsWiring::production()),
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
        tm_capture: Default::default(),
    })
    .unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let agent = fx.root.join(AGENT_NAME);
    gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
    Running {
        fx,
        tp,
        agent,
        handle,
        join: Some(join),
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

impl Running {
    /// The agent process, in the repo's folder, with `role` and `args`; its answer line.
    fn as_agent(&self, role: &str, args: Value) -> Value {
        let spec = json!({"role": role, "root": self.tp.root.path().join("profile"), "args": args});
        let out = Command::new(&self.agent)
            .args(["role_entry", "--exact", "--nocapture", "--test-threads=1"])
            .env(ENV, spec.to_string())
            .current_dir(canonical(&self.fx.repo))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v.get("stage").is_some())
            .unwrap_or_else(|| {
                panic!(
                    "the agent gave no answer: {} {}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
            })
    }

    fn commits(&self) -> usize {
        self.fx
            .git(&["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }
}

// ----- Tests -----------------------------------------------------------------------------------

/// MCP009-CHANNEL: a connection that never accepted `operation.commit` is told the operation is
/// not implemented for it, and which story implements it.
#[test]
fn prepare_of_commit_without_the_capability_is_not_implemented() {
    let r = start(true, false);
    let mut raw = Raw::connect(&r.tp.dirs(), "cli");
    let answer = raw.call(
        methods::OPERATION_PREPARE,
        json!({
            "operation": "commit",
            "worktree": canonical(&r.fx.repo),
            "surface": "tui",
            "args": {"message": "feat: a change"}
        }),
    );
    let (got, data) = answer.expect_err("a commit prepare without the capability is refused");
    assert_eq!(got, code::NOT_IMPLEMENTED, "{data}");
    assert_eq!(
        data["implemented_by"], "US-MCP-009",
        "the refusal names the story that serves commit: {data}"
    );
    assert_eq!(r.commits(), 1, "nothing was committed");
}

/// MCP009-CHANNEL: with the capability, prepare and run of a commit return the commit in the run
/// result: its oid, the paths it took and their total.
#[test]
fn a_commit_with_the_capability_returns_the_commit_in_the_run_result() {
    let r = start(true, false);
    let before = r.commits();
    let answer = r.as_agent(
        "commit",
        json!({"message": "feat: add x\n\nCo-Authored-By: Claude <noreply@anthropic.com>", "paths": ["x.txt"]}),
    );

    assert_eq!(answer["stage"], "run", "{answer}");
    let commit = &answer["ok"]["commit"];
    let head = r.fx.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        commit["oid"].as_str(),
        Some(head.trim()),
        "the run result carries the oid of the commit made: {answer}"
    );
    assert_eq!(commit["paths_total"], 1, "{answer}");
    assert!(
        commit["paths"].to_string().contains("x.txt"),
        "the run result lists the committed path: {answer}"
    );
    assert_eq!(r.commits(), before + 1, "one commit landed");
}

/// MCP009-CHANNEL: a step that fails with the branch tip unmoved (a rejecting `pre-commit`) is
/// `OPERATION_FAILED` with the operation id and `failure: "unchanged"`.
#[test]
fn a_failing_step_returns_operation_failed_with_failure_unchanged() {
    let r = start(true, true);
    let before = r.commits();
    let answer = r.as_agent(
        "commit",
        json!({"message": "feat: add x\n\nCo-Authored-By: Claude <noreply@anthropic.com>", "paths": ["x.txt"]}),
    );

    assert_eq!(answer["stage"], "run", "{answer}");
    assert_eq!(answer["code"], code::OPERATION_FAILED, "{answer}");
    assert!(
        answer["data"]["operation_id"].is_string(),
        "the failure names the operation: {answer}"
    );
    assert_eq!(
        answer["data"]["failure"], "unchanged",
        "the failure says the branch did not move: {answer}"
    );
    assert_eq!(r.commits(), before, "the hook rejected: no commit landed");
}

/// Guard 4.1: an MCP connection's commits spend its write bucket (burst 5): after the burst the
/// next commit prepare is `RATE_LIMITED`, and reads still pass. Nothing is staged, so no prepare
/// makes a plan or touches the repo.
#[test]
fn a_commit_spends_the_connections_write_bucket() {
    let r = start(false, false);
    let before = r.commits();
    let answer = r.as_agent("burst", json!({"message": "feat: nothing staged"}));

    let codes: Vec<i64> = answer["codes"]
        .as_array()
        .unwrap_or_else(|| panic!("{answer}"))
        .iter()
        .map(|c| c.as_i64().unwrap())
        .collect();
    assert_eq!(codes.len(), WRITE_BURST + 2, "{answer}");
    assert!(
        codes[..WRITE_BURST]
            .iter()
            .all(|c| *c != code::RATE_LIMITED),
        "the burst of {WRITE_BURST} commits passes the bucket: {codes:?}"
    );
    assert_eq!(
        codes[WRITE_BURST],
        code::RATE_LIMITED,
        "a commit past the burst spends the write bucket and is rate limited: {codes:?}"
    );
    assert_eq!(answer["read_ok"], true, "reads are not limited: {answer}");
    assert_eq!(r.commits(), before, "no commit was made");
}
