//! US-GRD-006 PR-B in the daemon (DS-US-GRD-006 D6, D11, D13, D16, D19, S-B12): the channel of
//! the conscious exception. The daemon runs in a child process of this test over a temporary
//! profile, home and repo (NFR-01), and recognises only `raptor-fake-agent`, so the suite
//! passes inside and outside a Claude Code session.
//!
//! The requester (a reserved call needs a terminal) is this test binary run through `script`
//! (`requester_entry`). It announces, waits for the window, confirms, starts a stand-in `git`
//! (a copy of this binary named `git`, `fake_git_entry`) bound with `guard.exception-child`, and
//! that git starts a hook client (`hook_client_entry`) that presents the token to
//! `guard.evaluate` as many times as asked. Everything goes through the wire: JSON strings and
//! method names, so this file compiles before the daemon implements them.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::guard::{EvaluateParams, Hook, Operation, RefUpdate, RefValue};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_core::guardrails::constants::STUB_FILE;
use gitraptor_core::guardrails::install::{GuardCtx, install};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::Invoker;
use gitraptor_git::resolve::{Resolution, ResolveConfig, resolve};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(30);
const DAEMON_ENV: &str = "RAPTOR_TEST_GRD006_DAEMON";
const REQUESTER_ENV: &str = "RAPTOR_TEST_GRD006_REQUESTER";
const HOOK_ENV: &str = "RAPTOR_TEST_GRD006_HOOK";
const TOKEN_ENV: &str = "GITRAPTOR_EXCEPTION_TOKEN";
const EXCEPTION: &str = "guard.exception";
const TODAY: [&str; 6] = [
    "guard.protection",
    "guard.pending-action",
    "guard.recovery",
    "guard.log-layers",
    "guard.prior-snapshot",
    "guard.snapshot-required",
];

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn caps(with_exception: bool) -> Vec<&'static str> {
    let mut caps: Vec<&str> = TODAY.to_vec();
    caps.push("audit.outcomes");
    if with_exception {
        caps.push(EXCEPTION);
    }
    caps
}

// ---- entry points (they do nothing as normal tests) ----

#[test]
fn daemon_entry() {
    let Some(spec) = std::env::var_os(DAEMON_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let vars = std::env::vars_os()
        .filter(|(key, _)| key != "HOME")
        .chain([("HOME".into(), spec["home"].as_str().unwrap().into())]);
    let env = DaemonEnv::from_vars(vars);
    let channel = ChannelConfig {
        launch_exe: Some(PathBuf::from(spec["raptor"].as_str().unwrap())),
        agents: AgentMatcher::only(vec!["raptor-fake-agent".into()]),
        ..ChannelConfig::default()
    };
    let config = DaemonConfig {
        dirs: ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap())),
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
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let stop = PathBuf::from(spec["stop"].as_str().unwrap());
    std::thread::spawn(move || {
        while !stop.exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
        handle.request(StopCause::Signal("TERM"));
    });
    daemon.run();
}

fn rpc(client: &mut Client, method: &str, params: Value) -> Value {
    match client.call::<_, Value>(method, params) {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    }
}

fn write_json(path: &str, value: &Value) {
    let out = PathBuf::from(path);
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(value).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

/// The requester: `{root, path, argv, out, git_copy, hook, mode}`; `mode` is `announce` (stop
/// after the announcement) or `run`.
#[test]
fn requester_entry() {
    let Some(spec) = std::env::var_os(REQUESTER_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let path = spec["path"].clone();
    let argv = spec["argv"].clone();
    let out = spec["out"].as_str().unwrap();
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let announce = rpc(
        &mut client,
        EXCEPTION,
        json!({ "path": path, "argv": argv }),
    );
    let mut report = json!({ "announce": announce });
    let pending = report["announce"]["ok"]["pending"].clone();
    if spec["mode"] == "announce" || !pending.is_object() {
        write_json(out, &report);
        return;
    }
    let action_id = pending["action_id"].as_str().unwrap().to_owned();
    let applies = pending["applies_at_ms"].as_i64().unwrap();
    let start = Instant::now();
    while now_ms() < applies + 20 {
        assert!(start.elapsed() < DEADLINE, "the window never closed");
        std::thread::sleep(Duration::from_millis(10));
    }
    let confirm = rpc(
        &mut client,
        EXCEPTION,
        json!({ "path": path, "argv": argv, "confirm": action_id }),
    );
    report["confirm"] = confirm.clone();
    let issued = confirm["ok"]["issued"].clone();
    if let Some(token) = issued["token"].as_str() {
        let mut git = Command::new(spec["git_copy"].as_str().unwrap())
            .args([
                "fake_git_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HOOK_ENV, spec["hook"].to_string())
            .env(TOKEN_ENV, token)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        report["child"] = rpc(
            &mut client,
            "guard.exception-child",
            json!({ "path": path, "actionId": action_id, "pid": git.id() }),
        );
        report["git_exit"] = json!(git.wait().unwrap().code());
        report["end"] = rpc(
            &mut client,
            "guard.exception-end",
            json!({ "path": path, "actionId": action_id }),
        );
    }
    write_json(out, &report);
}

/// The stand-in `git` (a copy of this binary named `git`): starts the hook client.
#[test]
fn fake_git_entry() {
    if std::env::var_os(HOOK_ENV).is_none() {
        return;
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "hook_client_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// The hook client: `{root, caps, params, evaluates, out}`. It sends the token it finds in its
/// environment whatever the capabilities, as a hostile or older hook would.
#[test]
fn hook_client_entry() {
    let Some(spec) = std::env::var_os(HOOK_ENV) else {
        return;
    };
    if std::env::var_os(TOKEN_ENV).is_none() {
        return;
    }
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let capabilities: Vec<&str> = spec["caps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    let mut raw = Raw::open(&dirs, &capabilities);
    let mut params = spec["params"].clone();
    params["exception_token"] = json!(std::env::var(TOKEN_ENV).unwrap());
    let answers: Vec<Value> = (0..spec["evaluates"].as_u64().unwrap())
        .map(|_| raw.call(methods::GUARD_EVALUATE, params.clone()))
        .collect();
    write_json(spec["out"].as_str().unwrap(), &json!(answers));
}

// ---- raw connection ----

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
    granted: Vec<String>,
}

impl Raw {
    fn open(dirs: &ProfileDirs, capabilities: &[&str]) -> Self {
        let path = socket_path(dirs).unwrap();
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(s) => break s,
                Err(e) if start.elapsed() > Duration::from_secs(10) => panic!("connect: {e}"),
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            next: 0,
            granted: Vec::new(),
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        assert!(hello.get("result").is_some(), "{hello}");
        let accept = raw.call(
            methods::CONNECTION_ACCEPT,
            json!({ "capabilities": capabilities }),
        );
        raw.granted = accept["result"]["capabilities"]
            .as_array()
            .unwrap_or_else(|| panic!("{accept}"))
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect();
        raw
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        loop {
            let mut answer = String::new();
            self.reader.read_line(&mut answer).unwrap();
            let answer: Value = serde_json::from_str(&answer).unwrap();
            if answer["id"] == id {
                return answer;
            }
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let answer = self.call(method, params);
        answer
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{method}: {answer}"))
    }
}

// ---- the machine ----

struct DaemonProc {
    child: Child,
    stop: PathBuf,
}

impl Drop for DaemonProc {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.stop, b"");
        let start = Instant::now();
        while self.child.try_wait().ok().flatten().is_none() {
            if start.elapsed() > DEADLINE {
                let _ = self.child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

struct Machine {
    /// First: it stops the daemon before the temporary profile goes away.
    _daemon: DaemonProc,
    tp: TempProfile,
    fx: Fixture,
    repo_id: String,
    git_copy: PathBuf,
}

fn stand_in_raptor(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    for name in ["raptor", STUB_FILE] {
        let file = dir.join(name);
        std::fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir.join("raptor")
}

impl Machine {
    /// A protected repo on `feat-x` (with `main` and a remote), and the daemon with `env` extra.
    fn new(env: &[(&str, &str)]) -> Self {
        if !cfg!(debug_assertions) {
            panic!(
                "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
            );
        }
        let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
        let tp = TempProfile::new();
        let remote = tp.root.path().join("remote.git");
        fx.git_in(
            &fx.repo,
            &["init", "-q", "--bare", remote.to_str().unwrap()],
        );
        fx.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        fx.git(&["push", "-q", "origin", "main"]);
        fx.git(&["switch", "-q", "-c", "feat-x"]);
        fx.git(&["push", "-q", "origin", "feat-x"]);
        let raptor = stand_in_raptor(&tp.root.path().join("bin"));
        let repo_id = protect(&tp, &fx.repo, &raptor);
        let git_copy = tp.root.path().join("agent-less").join("git");
        std::fs::create_dir_all(git_copy.parent().unwrap()).unwrap();
        gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &git_copy);
        let stop = tp.root.path().join("stop-daemon");
        let spec = json!({
            "root": tp.root.path().join("profile"),
            "home": fx.home,
            "raptor": raptor,
            "stop": stop,
        });
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["daemon_entry", "--exact", "--nocapture", "--test-threads=1"])
            .env(DAEMON_ENV, spec.to_string())
            .env("GITRAPTOR_TEST_GUARD_WINDOW_MS", "300")
            .env("GITRAPTOR_TEST_GUARD_EXCEPTION_CHILD_WAIT_MS", "5000")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (k, v) in env {
            command.env(k, v);
        }
        let child = command.spawn().unwrap();
        let daemon = DaemonProc { child, stop };
        let start = Instant::now();
        while Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
            assert!(start.elapsed() < DEADLINE, "the daemon never answered");
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            tp,
            fx,
            repo_id,
            git_copy,
            _daemon: daemon,
        }
    }

    fn raw(&self, with_exception: bool) -> Raw {
        Raw::open(&self.tp.dirs(), &caps(with_exception))
    }

    fn argv(&self) -> Value {
        json!(["git", "branch", "-D", "main"])
    }

    /// `git branch -D main` from `feat-x`: the evaluation the hook of that git presents.
    fn delete_main(&self) -> Value {
        let oid = self.fx.git(&["rev-parse", "main"]);
        let oid = oid.trim().to_owned();
        let params = EvaluateParams {
            exception_token: None,
            repo_id: self.repo_id.clone(),
            common_dir: self
                .fx
                .repo
                .join(".git")
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: "refs/heads/main".into(),
                    old: RefValue::Oid(oid),
                    new: RefValue::Zero,
                }],
                orphan_head: None,
            },
            authorship: None,
        };
        serde_json::to_value(params).unwrap()
    }

    /// Runs the requester under a terminal. `hook_caps`: what the hook client accepts.
    fn requester(&self, mode: &str, hook_caps: &[&str], evaluates: u64) -> (Value, Vec<Value>) {
        let out = self
            .tp
            .root
            .path()
            .join(format!("requester-{mode}-{evaluates}.json"));
        let hook_out = self
            .tp
            .root
            .path()
            .join(format!("hook-{mode}-{evaluates}.json"));
        let _ = std::fs::remove_file(&hook_out);
        let spec = json!({
            "root": self.tp.root.path().join("profile"),
            "path": self.fx.repo,
            "argv": self.argv(),
            "out": out,
            "git_copy": self.git_copy,
            "mode": mode,
            "hook": {
                "root": self.tp.root.path().join("profile"),
                "caps": hook_caps,
                "params": self.delete_main(),
                "evaluates": evaluates,
                "out": hook_out,
            },
        });
        let exe = std::env::current_exe().unwrap();
        let entry = [
            "requester_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ];
        let mut command = if cfg!(target_os = "macos") {
            let mut c = Command::new("/usr/bin/script");
            c.args(["-q", "/dev/null"]).arg(&exe).args(entry);
            c
        } else {
            let line = std::iter::once(exe.to_string_lossy().into_owned())
                .chain(entry.iter().map(|s| (*s).to_owned()))
                .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
                .collect::<Vec<_>>()
                .join(" ");
            let mut c = Command::new("script");
            c.args(["-q", "-e", "-c", &line, "/dev/null"]);
            c
        };
        let status = command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.tp.root.path())
            .env(REQUESTER_ENV, spec.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(
            out.exists(),
            "the requester ended ({status}) without a report"
        );
        let report: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        let answers = std::fs::read(&hook_out)
            .ok()
            .map(|b| serde_json::from_slice::<Vec<Value>>(&b).unwrap())
            .unwrap_or_default();
        (report, answers)
    }
}

fn protect(tp: &TempProfile, repo: &Path, raptor: &Path) -> String {
    let common = repo.join(".git").canonicalize().unwrap();
    let mut profile = tp.open();
    let repo_id = profile.add_repo(&common, None, 1).unwrap().0.repo_id;
    let (mut store, _) = profile.open_store(&repo_id).unwrap();
    let invoker = Invoker::default();
    let git = match resolve(&ResolveConfig::for_current_os(None), &invoker) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git: {diagnostics:?}"),
    };
    let dirs = tp.dirs();
    let ctx = GuardCtx {
        git: &git,
        invoker: &invoker,
        dirs: &dirs,
        instance: profile.instance_id(),
        raptor,
    };
    let registry = GuardRegistry::for_profile(dirs.clone());
    install(
        &ctx,
        &repo_id,
        &common,
        &mut store,
        &registry,
        now_ms(),
        false,
    )
    .unwrap_or_else(|e| panic!("install: {e:?}"));
    repo_id
}

fn log_entries(raw: &mut Raw, repo: &Path) -> Vec<Value> {
    let log = raw.result("guard.log", json!({ "path": repo }));
    log["entries"].as_array().cloned().unwrap_or_default()
}

fn kinds(entries: &[Value], prefix: &str) -> Vec<String> {
    entries
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .filter(|k| k.starts_with(prefix))
        .map(str::to_owned)
        .collect()
}

fn used_audits(raw: &mut Raw) -> usize {
    raw.result(methods::AUDIT_LIST, json!({}))["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["outcome"] == "used")
        .count()
}

// ---- the tests ----

#[test]
fn exception_capability_both_ways() {
    let m = Machine::new(&[("GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS", "0")]);
    let mut new = m.raw(true);
    let mut old = m.raw(false);
    assert!(
        new.granted.iter().any(|c| c == EXCEPTION),
        "{:?}",
        new.granted
    );
    assert!(
        !old.granted.iter().any(|c| c == EXCEPTION),
        "{:?}",
        old.granted
    );

    // The pending of an exception: seen with the capability, never without it.
    let (report, _) = m.requester("announce", &caps(true), 0);
    assert!(
        report["announce"]["ok"]["pending"].is_object(),
        "GRD006-CAPABILITY: guard.exception should announce an exception: {report}"
    );
    let status = new.result("guard.status", json!({ "path": m.fx.repo }));
    assert_eq!(
        status["pending"]["action"], "exception",
        "GRD006-CAPABILITY: guard.status with the capability should show the pending exception: {status:#}"
    );
    let status = old.result("guard.status", json!({ "path": m.fx.repo }));
    assert!(
        status.get("pending").is_none() && !status.to_string().contains("exception"),
        "GRD006-CAPABILITY: guard.status without the capability should hide the pending exception: {status:#}"
    );

    // Cancelled: the entry `exception-cancelled` is for the capable client only.
    new.result("guard.cancel", json!({ "path": m.fx.repo }));
    let with = log_entries(&mut new, &m.fx.repo);
    assert_eq!(
        kinds(&with, "exception"),
        ["exception-cancelled"],
        "GRD006-CAPABILITY: guard.log with the capability should carry exception-cancelled"
    );
    let without = old.result("guard.log", json!({ "path": m.fx.repo }));
    assert!(
        kinds(without["entries"].as_array().unwrap(), "exception").is_empty(),
        "GRD006-CAPABILITY: guard.log without the capability should drop the exception* entries: {without:#}"
    );
    assert!(
        without["summary"].get("exceptions").is_none()
            && without["summary"].get("exceptionsRejected").is_none(),
        "GRD006-CAPABILITY: the exception counters stay at zero without the capability: {without:#}"
    );

    // The token: a hook that did not accept the capability is not believed; one that did, is.
    let (_, answers) = m.requester("run", &caps(false), 1);
    assert_eq!(answers.len(), 1, "the hook client presented nothing");
    let result = &answers[0]["result"];
    assert!(
        result["exception"] != "applied" && result["appliedEffect"] == "deny",
        "GRD006-CAPABILITY: without guard.exception the token is not accepted: {}",
        answers[0]
    );
    let (_, answers) = m.requester("run", &caps(true), 1);
    let result = &answers[0]["result"];
    assert!(
        result["exception"] == "applied" && result["appliedEffect"] == "allow",
        "GRD006-CAPABILITY: with guard.exception the token should be accepted: {}",
        answers[0]
    );
}

#[test]
fn apply_orders_before_the_prior_and_logs_once() {
    // The prior snapshot of the rebase / branch delete fails: `apply` ran before it, so
    // `enforce` keeps `allow` with the exception applied (D13).
    let m = Machine::new(&[("GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS", "0")]);
    let (report, answers) = m.requester("run", &caps(true), 2);
    assert!(
        report["confirm"]["ok"]["issued"]["token"].is_string(),
        "GRD006-APPLY-ORDER: guard.exception should issue a token once the window closed: {report}"
    );
    assert_eq!(answers.len(), 2, "the hook client presented twice");
    for (i, answer) in answers.iter().enumerate() {
        let r = &answer["result"];
        assert!(
            r["appliedEffect"] == "allow" && r["exception"] == "applied",
            "GRD006-APPLY-ORDER: evaluation {i} should be allow with exception applied even when the prior failed: {answer}"
        );
    }
    let mut raw = m.raw(true);
    assert_eq!(
        used_audits(&mut raw),
        1,
        "GRD006-APPLY-ORDER: the audit `used` is written once per token, not per evaluation"
    );
    let entries = log_entries(&mut raw, &m.fx.repo);
    let exceptions: Vec<&Value> = entries
        .iter()
        .filter(|e| e["kind"] == "exception")
        .collect();
    assert_eq!(
        exceptions.len(),
        1,
        "GRD006-APPLY-ORDER: one `exception` log entry per token: {entries:#?}"
    );
    assert!(
        exceptions[0].to_string().contains("system.snapshot-failed"),
        "GRD006-APPLY-ORDER: the entry should carry system.snapshot-failed: {}",
        exceptions[0]
    );
    assert_eq!(exceptions[0]["count"], 1, "{}", exceptions[0]);
}

#[test]
fn no_exception_without_the_used_audit() {
    let m = Machine::new(&[("GITRAPTOR_TEST_GUARD_FAIL_USED_AUDIT", "1")]);
    let (report, answers) = m.requester("run", &caps(true), 1);
    assert!(
        report["confirm"]["ok"]["issued"]["token"].is_string(),
        "GRD006-USED-AUDIT-FAILS: guard.exception should issue a token once the window closed: {report}"
    );
    assert_eq!(answers.len(), 1, "the hook client presented nothing");
    let r = &answers[0]["result"];
    assert!(
        r["appliedEffect"] == "deny" && r["exception"] == "rejected",
        "GRD006-USED-AUDIT-FAILS: when the `used` audit cannot be written the token is denied with exception = rejected: {}",
        answers[0]
    );
    let mut raw = m.raw(true);
    let entries = log_entries(&mut raw, &m.fx.repo);
    assert!(
        kinds(&entries, "exception")
            .iter()
            .all(|k| k != "exception"),
        "GRD006-USED-AUDIT-FAILS: no `exception` entry without the used audit: {entries:#?}"
    );
    assert_eq!(used_audits(&mut raw), 0, "no `used` audit was written");
    assert!(
        kinds(&entries, "exception")
            .iter()
            .any(|k| k == "exception-rejected"),
        "GRD006-USED-AUDIT-FAILS: the denied presentation should log exception-rejected: {entries:#?}"
    );
}

/// The wire of a malformed `exception_token` (D4 of the PR-A review): the call is not broken, the
/// decision is the usual one. An in-process daemon, as `us_grd_017` does.
#[test]
fn a_malformed_exception_token_does_not_break_evaluate() {
    let tp = TempProfile::new();
    let env = DaemonEnv::from_vars(Vec::new());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        env,
        git: ResolveConfig {
            configured_path: None,
            path_env: None,
            known_locations: Vec::new(),
            shim_paths: Vec::new(),
            toolchain_gits: Vec::new(),
        },
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let repo = common::init_repo(tp.root.path(), "r", true);
    let common_dir = common::common_dir(&repo);
    let mut raw = Raw::open(&tp.dirs(), &caps(true));
    let base = json!({
        "repo_id": "0000-ffff",
        "common_dir": common_dir,
        "hook": "reference-transaction",
        "operation": {"kind": "ref-transaction", "updates": [
            {"refname": "refs/heads/x", "old": "zero", "new": "zero"}
        ]},
    });
    let plain = raw.call(methods::GUARD_EVALUATE, base.clone());
    assert!(plain["result"].is_object(), "the plain call fails: {plain}");
    for bad in ["not-a-token", "ABCDEF0123456789ABCDEF0123456789", ""] {
        let mut params = base.clone();
        params["exception_token"] = json!(bad);
        let answer = raw.call(methods::GUARD_EVALUATE, params);
        assert!(
            answer["result"].is_object(),
            "GRD006-MALFORMED-TOKEN: a malformed exception_token ({bad:?}) should not break guard.evaluate: {answer}"
        );
        assert_eq!(
            answer["result"]["appliedEffect"], plain["result"]["appliedEffect"],
            "GRD006-MALFORMED-TOKEN: the decision should be the usual one: {answer}"
        );
        assert_eq!(
            answer["result"]["exception"], "none",
            "GRD006-MALFORMED-TOKEN: no exception for a malformed token: {answer}"
        );
    }
    drop(raw);
    handle.request(StopCause::Signal("TERM"));
    let _ = join.join();
}
