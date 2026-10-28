//! Correcting from a folder of an observed repo that is not one of its worktrees: the folder of a
//! bare repo (and a folder inside it) whose only worktree is a linked one. The engine answers
//! `not-a-worktree` and nothing changes.
//!
//! A folder inside a worktree, its `.git` included, resolves to that worktree, so a repo with a
//! main worktree has no such folder: the bare layout is the one that does.
//!
//! Same harness as `us_grp_010.rs`: the daemon is a child process of this test binary over a
//! temporary repo, profile and HOME (NFR-01), and the developer is this binary under `script`, so
//! it has a controlling terminal. Waits end on explicit signals with a deadline.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::{ClientKind, SessionsListResult};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const ENV: &str = "RAPTOR_T_GRP010_NAW";
const DEADLINE: Duration = Duration::from_secs(30);
/// `attribution-rejected`, the block of the attribution module.
const ATTRIBUTION_REJECTED: i64 = -33160;

/// Every process of the test but the test itself is this binary with a role; as a normal test it
/// does nothing.
#[test]
fn role_entry() {
    let Some(spec) = std::env::var_os(ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    match spec["role"].as_str().unwrap() {
        "daemon" => daemon_role(&spec, dirs),
        "calls" => calls_role(&spec, &dirs),
        other => panic!("unknown role {other}"),
    }
}

fn daemon_role(spec: &Value, dirs: ProfileDirs) {
    let vars = std::env::vars_os()
        .filter(|(key, _)| key != "HOME")
        .chain([("HOME".into(), spec["home"].as_str().unwrap().into())]);
    let env = DaemonEnv::from_vars(vars);
    let config = DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig {
            // No real agent of the machine running the suite is taken for one.
            agents: AgentMatcher::only(vec!["raptor-fake-agent".into()]),
            ..ChannelConfig::default()
        },
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
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

/// Sends each `[method, params]` of `calls` and writes the answers to `out`.
fn calls_role(spec: &Value, dirs: &ProfileDirs) {
    let mut client = Client::connect(dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let answers: Vec<Value> = spec["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(
            |call| match client.call::<_, Value>(call[0].as_str().unwrap(), call[1].clone()) {
                Ok(v) => json!({ "ok": v }),
                Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
                Err(e) => json!({ "other": e.to_string() }),
            },
        )
        .collect();
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

fn role_command(exe: &Path, spec: &Value) -> Command {
    let mut c = Command::new(exe);
    c.args(["role_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV, spec.to_string());
    c
}

struct Env {
    fx: Fixture,
    tp: TempProfile,
    repo_id: String,
    bare: PathBuf,
    stop: PathBuf,
    daemon: Child,
}

impl Drop for Env {
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

/// A bare repo `bare.git` with one linked worktree, observed by a daemon in a child process.
fn env() -> Env {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let bare = fx.root.join("bare.git");
    fx.git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    let wt = fx.root.join("wt-bare");
    fx.git_in(
        &bare,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "main"],
    );
    let bare = bare.canonicalize().unwrap();
    let tp = TempProfile::new();
    let (entry, _) = tp.open().add_repo(&bare, None, 1).unwrap();
    let stop = tp.root.path().join("stop-daemon");
    let spec = json!({
        "role": "daemon",
        "root": tp.root.path().join("profile"),
        "home": fx.home,
        "stop": stop,
    });
    let daemon = role_command(&std::env::current_exe().unwrap(), &spec)
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
    Env {
        fx,
        tp,
        repo_id: entry.repo_id,
        bare,
        stop,
        daemon,
    }
}

impl Env {
    /// The calls of the developer: this binary under `script`, with a clean environment.
    fn developer(&self, calls: &[(&str, Value)]) -> Vec<Value> {
        let out = self.tp.root.path().join("answers.json");
        let _ = std::fs::remove_file(&out);
        let spec = json!({
            "role": "calls",
            "root": self.tp.root.path().join("profile"),
            "calls": calls.iter().map(|(m, p)| json!([m, p])).collect::<Vec<_>>(),
            "out": out,
        });
        let exe = std::env::current_exe().unwrap();
        let entry = ["role_entry", "--exact", "--nocapture", "--test-threads=1"];
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
            .env("HOME", &self.fx.home)
            .env(ENV, spec.to_string())
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

    /// Rows of `sessions` and `attribution_records` plus `sessions.list`: what "nothing changes"
    /// compares.
    fn state(&self) -> ((i64, i64), Value) {
        let path = self
            .tp
            .dirs()
            .repos_dir()
            .join(format!("{}.sqlite", self.repo_id));
        let conn =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        let sessions: SessionsListResult =
            Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION)
                .unwrap()
                .call(
                    methods::SESSIONS_LIST,
                    json!({"repo_id": self.repo_id, "include_ended": true}),
                )
                .unwrap();
        (
            (count("sessions"), count("attribution_records")),
            serde_json::to_value(sessions.sessions).unwrap(),
        )
    }
}

fn reason(answer: &Value) -> &str {
    assert_eq!(
        answer["code"], ATTRIBUTION_REJECTED,
        "expected attribution-rejected: {answer}"
    );
    answer["data"]["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("no reason in {answer}"))
}

/// The folder of an observed bare repo, and a folder inside it, are not worktrees: correcting or
/// withdrawing there is `not-a-worktree`, and nothing changes.
#[test]
fn a_folder_of_an_observed_repo_that_is_not_a_worktree_is_rejected() {
    let e = env();
    let refs = e.bare.join("refs");
    let before = e.state();

    let answers = e.developer(&[
        (
            methods::ATTRIBUTION_CORRECT,
            json!({"agent": {"kind": "other", "name": "Codex"}, "worktree": e.bare}),
        ),
        (
            methods::ATTRIBUTION_CORRECT,
            json!({"agent": {"kind": "other", "name": "Codex"}, "worktree": refs}),
        ),
        (methods::ATTRIBUTION_WITHDRAW, json!({"worktree": e.bare})),
    ]);

    for answer in &answers {
        assert_eq!(reason(answer), "not-a-worktree", "{answer}");
    }
    assert_eq!(e.state(), before, "no session nor attribution changed");
}
