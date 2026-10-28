//! US-GRD-014 (D10): a `guard.confirm` window nobody applies expires. The daemon audits it as
//! `guard.confirm` / `expired` and confirms nothing: the confirmed base stays `main`.
//!
//! The daemon runs in a child process with a short window and grace
//! (`GITRAPTOR_TEST_GUARD_WINDOW_MS`, `GITRAPTOR_TEST_GUARD_GRACE_MS`, debug builds only); the
//! developer's client runs in a terminal through `script`, so it is not the daemon's descendant.
//! Temporary profile, home and repo (NFR-01). The signal is the status itself: the test asks
//! until the pending action is gone, under a deadline.
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
use gitraptor_core::guardrails::pending::{GRACE_ENV, WINDOW_ENV};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_git::Invoker;
use gitraptor_git::resolve::{Resolution, ResolveConfig, resolve};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(30);
const DAEMON_ENV: &str = "RAPTOR_TEST_GRD014_EXPIRED_DAEMON";
const CLIENT_ENV: &str = "RAPTOR_TEST_GRD014_EXPIRED_CLIENT";

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

/// The daemon process: serves the profile at `root` until `stop` exists.
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

/// The developer's client: asks for the plan and accepts it, then never applies the window.
#[test]
fn developer_client_entry() {
    let Some(spec) = std::env::var_os(CLIENT_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let path = spec["path"].as_str().unwrap();
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let mut call = |params: Value| match client.call::<_, Value>(methods::GUARD_CONFIRM, params) {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let plan = call(json!({ "path": path }));
    let mut answers = vec![plan.clone()];
    if let Some(fingerprint) = plan["ok"]["fingerprint"].as_str() {
        answers.push(call(json!({ "path": path, "accept": fingerprint })));
    }
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
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

/// Adds the repo and installs the protection (it confirms `main`: no team settings yet).
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
    repo_id
}

struct DaemonProc {
    child: Child,
    stop: PathBuf,
}

impl DaemonProc {
    fn stop(&mut self) {
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

impl Drop for DaemonProc {
    fn drop(&mut self) {
        self.stop();
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
        .env(WINDOW_ENV, "50")
        .env(GRACE_ENV, "50")
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

/// The developer's plan and acceptance, from a terminal through `script`.
fn developer_accepts(tp: &TempProfile, repo: &Path) -> Vec<Value> {
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
    assert!(out.exists(), "the developer's client ended ({status})");
    serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
}

/// `guard.status` over a raw connection with every capability this test reads.
fn status(dirs: &ProfileDirs, repo: &Path) -> Value {
    let stream = UnixStream::connect(socket_path(dirs).unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut stream = stream;
    let mut call = |id: u64, method: &str, params: Value| -> Value {
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        stream.write_all(line.to_string().as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        loop {
            let mut answer = String::new();
            reader.read_line(&mut answer).unwrap();
            let answer: Value = serde_json::from_str(&answer).unwrap();
            if answer["id"] == id {
                return answer;
            }
        }
    };
    call(
        1,
        methods::HELLO,
        json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
    );
    call(
        2,
        methods::CONNECTION_ACCEPT,
        json!({"capabilities": ["guard.protection", "guard.pending-action", "guard.team-confirmation"]}),
    );
    let answer = call(3, methods::GUARD_STATUS, json!({ "path": repo }));
    answer
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("guard.status: {answer}"))
}

#[test]
fn an_unapplied_confirmation_expires_and_confirms_nothing() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let tp = TempProfile::new();
    let raptor = stand_in_raptor(&tp.root.path().join("bin"));
    let repo_id = protect(&tp, &fx.repo, &raptor);
    std::fs::create_dir_all(fx.repo.join(".gitraptor")).unwrap();
    std::fs::write(
        fx.repo.join(".gitraptor/settings.json"),
        r#"{"engine":{"baseBranch":"develop"}}"#,
    )
    .unwrap();
    fx.git(&["add", ".gitraptor/settings.json"]);
    fx.git(&["commit", "-q", "-m", "team base"]);
    let mut daemon = start_daemon(&tp, &fx, &raptor);

    // `develop` instead of `main`: accepting opens the window, and nobody applies it.
    let answers = developer_accepts(&tp, &fx.repo);
    assert_eq!(answers.len(), 2, "{answers:#?}");
    assert_eq!(answers[1]["ok"]["result"], "pending", "{answers:#?}");

    // The status sweeps the expired window: wait until the pending action is gone.
    let dirs = tp.dirs();
    let begin = Instant::now();
    loop {
        let s = status(&dirs, &fx.repo);
        if s.get("pending").is_none() {
            assert!(
                s["diagnostics"]
                    .as_array()
                    .is_some_and(|d| d.iter().any(|x| x == "base-change-pending")),
                "{s:#}"
            );
            break;
        }
        assert!(begin.elapsed() < DEADLINE, "never expired: {s:#}");
        std::thread::sleep(Duration::from_millis(20));
    }
    daemon.stop();

    let (profile, _) = Profile::open(tp.dirs()).unwrap();
    let outcomes: Vec<String> = profile
        .audit(0, 1000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.operation == "guard.confirm")
        .map(|(_, row)| row.outcome)
        .collect();
    assert!(outcomes.contains(&"expired".to_owned()), "{outcomes:?}");
    assert!(!outcomes.contains(&"applied".to_owned()), "{outcomes:?}");
    let (store, _) = profile.open_store(&repo_id).unwrap();
    let confirmed = store.confirmed_team_baseline().unwrap().unwrap();
    assert_eq!(confirmed.base_branch.as_str(), "main");
}
