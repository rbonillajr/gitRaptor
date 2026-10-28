//! US-GRD-014 (D8): `guard.team-confirmation` in both directions. A protected repo with `main`
//! confirmed whose main branch then fixes `develop` (a pending change) is read with
//! `guard.status` from two raw connections of the same daemon: one that accepted
//! `guard.team-confirmation` and one that did not. With it, the status carries the new
//! diagnostics, `confirm` and, once a developer accepts the plan, the `confirm-team` pending
//! action; without it, the view of today.
//!
//! The daemon runs in a child process of this test (so the developer's client, in a terminal
//! through `script`, is not its descendant); temporary profile, home and repo (NFR-01). The
//! protection is installed with `guardrails::install` over a stand-in `raptor` and dispatcher.
//!
//! Wire names assumed (kebab-case, DS-US-GRD-014 § 5 and D8): capability
//! `guard.team-confirmation`; `GuardStatus.diagnostics` codes `base-change-pending`,
//! `base-change-pending-invalid`, `floor-relax-pending`; `GuardStatus.confirm.action ==
//! "guard-confirm"`; `GuardStatus.pending.action == "confirm-team"`; method `guard.confirm` with
//! `{path}` (answers a plan carrying a `fingerprint` string) and `{path, accept}`.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::guard::ProtectionState;
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

const CAP: &str = "guard.team-confirmation";
const DEADLINE: Duration = Duration::from_secs(30);
const DAEMON_ENV: &str = "RAPTOR_TEST_GRD014_DAEMON";
const CLIENT_ENV: &str = "RAPTOR_TEST_GRD014_CLIENT";

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

/// Entry point of the daemon process: with `RAPTOR_TEST_GRD014_DAEMON` set to
/// `{"root", "home", "raptor", "stop"}` it serves the profile at `root` until `stop` exists. As a
/// normal test it does nothing.
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
        // The agents the daemon knows: none of this test's ancestors (a Claude Code session may
        // run the suite).
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
        predict: Default::default(),
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

/// Entry point of the developer's client, run in a terminal: with `RAPTOR_TEST_GRD014_CLIENT`
/// set to `{"root", "path", "out"}` it asks `guard.confirm` for the plan and accepts it with its
/// fingerprint, and writes both answers to `out`. As a normal test it does nothing.
#[test]
fn developer_client_entry() {
    let Some(spec) = std::env::var_os(CLIENT_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let path = spec["path"].as_str().unwrap();
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let mut call = |params: Value| match client.call::<_, Value>("guard.confirm", params) {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let plan = call(json!({ "path": path }));
    let mut answers = vec![plan.clone()];
    if let Some(fingerprint) = find_key(&plan, "fingerprint").and_then(Value::as_str) {
        answers.push(call(json!({ "path": path, "accept": fingerprint })));
    }
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|x| find_key(x, key))),
        Value::Array(items) => items.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

/// A stand-in `raptor` and its dispatcher next to it: the install copies the dispatcher, and the
/// hooks it leaves let every Git operation of the test go ahead.
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

/// Adds the repo to the profile and installs the protection (it confirms `main`: the repo has
/// no team settings yet). Returns the repo id.
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
    let status = install(
        &ctx,
        &repo_id,
        &common,
        &mut store,
        &registry,
        now_ms(),
        false,
    )
    .unwrap_or_else(|e| panic!("install: {e:?}"));
    assert_eq!(status.state, ProtectionState::HooksOnly);
    assert!(status.base_confirmed);
    repo_id
}

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

fn start_daemon(tp: &TempProfile, fx: &Fixture, raptor: &Path) -> DaemonProc {
    let stop = tp.root.path().join("stop-daemon");
    let spec = json!({
        "root": tp.root.path().join("profile"),
        "home": fx.home,
        "raptor": raptor,
        "stop": stop,
    });
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["daemon_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(DAEMON_ENV, spec.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
        assert!(start.elapsed() < DEADLINE, "the daemon never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    DaemonProc { child, stop }
}

/// A raw connection that accepted exactly `capabilities`.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
    pub granted: Vec<String>,
}

impl Raw {
    fn open(dirs: &ProfileDirs, capabilities: &[&str]) -> Self {
        let stream = UnixStream::connect(socket_path(dirs).unwrap()).unwrap();
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

    fn status(&mut self, repo: &Path) -> Value {
        let answer = self.call(methods::GUARD_STATUS, json!({ "path": repo }));
        answer
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("guard.status: {answer}"))
    }
}

fn diagnostics(status: &Value) -> Vec<String> {
    status["diagnostics"]
        .as_array()
        .map(|d| {
            d.iter()
                .map(|x| x.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The developer's `guard.confirm` (plan, then accept) from a terminal through `script`.
fn developer_confirms(tp: &TempProfile, repo: &Path) -> Vec<Value> {
    let out = tp.root.path().join("developer-answers.json");
    let spec = json!({
        "root": tp.root.path().join("profile"),
        "path": repo,
        "out": out,
    });
    let exe = std::env::current_exe().unwrap();
    let entry = [
        "developer_client_entry",
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
    // A clean environment, as a developer's terminal has: nothing of the session running the
    // tests reaches the client.
    let status = command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", tp.root.path())
        .env(CLIENT_ENV, spec.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        out.exists(),
        "the developer's client ended ({status}) without answers"
    );
    serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
}

#[test]
fn team_confirmation_capability_both_ways() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let tp = TempProfile::new();
    let raptor = stand_in_raptor(&tp.root.path().join("bin"));
    protect(&tp, &fx.repo, &raptor);
    // A real commit on the main branch fixes `develop`: a pending change.
    std::fs::create_dir_all(fx.repo.join(".gitraptor")).unwrap();
    std::fs::write(
        fx.repo.join(".gitraptor/settings.json"),
        r#"{"engine":{"baseBranch":"develop"}}"#,
    )
    .unwrap();
    fx.git(&["add", ".gitraptor/settings.json"]);
    fx.git(&["commit", "-q", "-m", "team base"]);
    let _daemon = start_daemon(&tp, &fx, &raptor);
    let dirs = tp.dirs();
    let today = ["guard.protection", "guard.pending-action", "guard.recovery"];
    let with: Vec<&str> = today.iter().copied().chain([CAP]).collect();
    let mut new = Raw::open(&dirs, &with);
    let mut old = Raw::open(&dirs, &today);
    assert!(new.granted.iter().any(|c| c == CAP), "{:?}", new.granted);
    assert!(!old.granted.iter().any(|c| c == CAP), "{:?}", old.granted);

    // With the capability: the new diagnostic and the hint to confirm.
    let status = new.status(&fx.repo);
    assert!(
        diagnostics(&status).contains(&"base-change-pending".to_owned()),
        "{status:#}"
    );
    assert_eq!(status["confirm"]["action"], "guard-confirm", "{status:#}");
    // Without it: the view of today, no new diagnostic, no hint.
    let status = old.status(&fx.repo);
    for code in [
        "base-change-pending",
        "base-change-pending-invalid",
        "floor-relax-pending",
    ] {
        assert!(
            !diagnostics(&status).contains(&code.to_owned()),
            "{status:#}"
        );
    }
    assert!(status.get("confirm").is_none(), "{status:#}");

    // The developer accepts the plan: `develop` instead of `main` opens the window.
    let answers = developer_confirms(&tp, &fx.repo);
    assert_eq!(answers.len(), 2, "no plan with a fingerprint: {answers:#?}");
    assert!(answers[1].get("ok").is_some(), "{answers:#?}");
    let status = new.status(&fx.repo);
    assert_eq!(status["pending"]["action"], "confirm-team", "{status:#}");
    // A client without the capability could not decode the variant: it never sees it.
    let status = old.status(&fx.repo);
    assert!(status.get("pending").is_none(), "{status:#}");
    assert!(!status.to_string().contains("confirm-team"), "{status:#}");
}
