//! US-GRD-014 (D9): `mcp.status` declares a pending base. A protected repo with `main` confirmed
//! receives on its main branch a real commit that fixes `engine.baseBranch: develop`; the agent's
//! `status` then says the base is `pending`, still named after the confirmed `main`, and never
//! carries the hint to confirm (the MCP layer never offers `guard.confirm`).
//!
//! Real daemon (in-process) and the real client library in another process whose working folder
//! is the repo, over a temporary profile, home and repo (NFR-01). The protection is installed
//! with `guardrails::install` over a stand-in `raptor` and dispatcher. No fixed waits: the client
//! asks until the answer changes, under a deadline.
//!
//! Wire names assumed: `repo.base.state == "pending"` (`McpBaseState::Pending`), the hint of
//! DS-US-GRD-014 D8 as a `confirm` field or the `guard-confirm` code, neither of which may reach
//! an MCP client.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::guard::ProtectionState;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::guardrails::constants::STUB_FILE;
use gitraptor_core::guardrails::install::{GuardCtx, install};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::Invoker;
use gitraptor_git::resolve::{Resolution, ResolveConfig, resolve};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(30);
const CLIENT_ENV: &str = "RAPTOR_TEST_GRD014_MCP_CLIENT";

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

/// Entry point of the agent's client: with `RAPTOR_TEST_GRD014_MCP_CLIENT` set to
/// `{"root", "out", "deadline_ms"}` it connects as an `mcp` client and asks `mcp.status` until
/// the base is `pending` or the deadline passes, and writes every answer to `out`. As a normal
/// test it does nothing.
#[test]
fn mcp_client_entry() {
    let Some(spec) = std::env::var_os(CLIENT_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let deadline = Instant::now() + Duration::from_millis(spec["deadline_ms"].as_u64().unwrap());
    let mut client = Client::connect(&dirs, ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let mut answers = Vec::new();
    loop {
        let answer = match client.call::<_, Value>(methods::MCP_STATUS, json!({})) {
            Ok(v) => json!({ "ok": v }),
            Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
            Err(e) => json!({ "other": e.to_string() }),
        };
        let pending = answer["ok"]["repo"]["base"]["state"] == "pending";
        answers.push(answer);
        if pending || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
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

/// Adds the repo to the profile with the MCP allowlist on and installs the protection (it
/// confirms `main`: the repo has no team settings yet).
fn protect(tp: &TempProfile, repo: &Path, raptor: &Path) {
    let common = repo.join(".git").canonicalize().unwrap();
    let mut profile = tp.open();
    let repo_id = profile.add_repo(&common, None, 1).unwrap().0.repo_id;
    profile.set_mcp_enabled(&repo_id, true, "test", 2).unwrap();
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
}

struct Running {
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn start(tp: &TempProfile, fx: &Fixture, raptor: &Path) -> Running {
    let vars = std::env::vars_os()
        .filter(|(key, _)| key != "HOME")
        .chain([("HOME".into(), fx.home.clone().into_os_string())]);
    let env = DaemonEnv::from_vars(vars);
    let channel = ChannelConfig {
        launch_exe: Some(raptor.to_owned()),
        ..ChannelConfig::default()
    };
    let config = DaemonConfig {
        dirs: tp.dirs(),
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
    let join = std::thread::spawn(move || daemon.run());
    let begin = Instant::now();
    while Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
        assert!(begin.elapsed() < DEADLINE, "the daemon never answered");
        std::thread::sleep(Duration::from_millis(10));
    }
    Running {
        handle,
        join: Some(join),
    }
}

/// The agent's `mcp.status` answers from a process working in `cwd`.
fn agent_status(tp: &TempProfile, cwd: &Path) -> Vec<Value> {
    let out = tp.root.path().join("mcp-answers.json");
    let spec = json!({
        "root": tp.root.path().join("profile"),
        "out": out,
        "deadline_ms": DEADLINE.as_millis() as u64,
    });
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "mcp_client_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(cwd)
        .env(CLIENT_ENV, spec.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        out.exists(),
        "the agent's client ended ({status}) without answers"
    );
    serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
}

#[test]
fn a_pending_base_is_declared() {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let tp = TempProfile::new();
    let raptor = stand_in_raptor(&tp.root.path().join("bin"));
    protect(&tp, &fx.repo, &raptor);
    let _daemon = start(&tp, &fx, &raptor);

    // A real commit on the main branch fixes `develop` while the daemon observes the repo.
    std::fs::create_dir_all(fx.repo.join(".gitraptor")).unwrap();
    std::fs::write(
        fx.repo.join(".gitraptor/settings.json"),
        r#"{"engine":{"baseBranch":"develop"}}"#,
    )
    .unwrap();
    fx.git(&["add", ".gitraptor/settings.json"]);
    fx.git(&["commit", "-q", "-m", "team base"]);

    let answers = agent_status(&tp, &fx.repo);
    let last = answers.last().expect("one answer at least");
    let repo = &last["ok"]["repo"];
    assert_eq!(repo["base"]["state"], "pending", "{last:#}");
    assert_eq!(repo["base"]["name"]["untrusted"], "main", "{last:#}");
    // The MCP layer never gets the hint to confirm.
    for answer in &answers {
        let text = answer.to_string();
        assert!(!text.contains("guard-confirm"), "{answer:#}");
        assert!(answer["ok"].get("confirm").is_none(), "{answer:#}");
        assert!(repo.get("confirm").is_none(), "{answer:#}");
    }
}
