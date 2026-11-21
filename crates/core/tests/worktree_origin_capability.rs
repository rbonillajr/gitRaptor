//! US-WTL-005 (T005, T006): `origin` reaches only a connection that accepted
//! `observation.worktree-origin`, in `engine.snapshot`, `scope.snapshot`,
//! `worktree.state` and the worktrees page of `raptor-mcp`; `repo.add` is
//! reserved to a controlling terminal (a test client has none), so its shape
//! is checked on `RepoView` itself, which is what the channel applies to it.
//! A real daemon (in-process) over a temporary repo and profile (NFR-01);
//! the field is read from the JSON on purpose. The `mcp` client runs in
//! another process whose working folder is the worktree, as in
//! `mcp_status_full.rs`.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::{ClientKind, RepoView};
use gitraptor_api::methods::McpStatus;
use gitraptor_api::{PROTOCOL_VERSION, capability, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const CAPABILITY: &str = "observation.worktree-origin";
const CLIENT_ENV: &str = "RAPTOR_TEST_ORIGIN_CLIENT";
const DEADLINE: Duration = Duration::from_secs(30);

/// A repo with the main worktree, `wt-orca` (with the `orca/` mark in its
/// administration directory) and `manual` (nothing known about it).
struct Shop {
    f: Fixture,
    orca: PathBuf,
    manual: PathBuf,
}

fn shop() -> Shop {
    let f = Fixture::with_commit(&git_from_path());
    let orca = f.root.join("wt-orca");
    let manual = f.root.join("manual");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "orca-b",
        orca.to_str().unwrap(),
    ]);
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "manual-b",
        manual.to_str().unwrap(),
    ]);
    std::fs::create_dir_all(f.repo.join(".git/worktrees/wt-orca/orca")).unwrap();
    Shop {
        orca: orca.canonicalize().unwrap(),
        manual: manual.canonicalize().unwrap(),
        f,
    }
}

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    repo_id: String,
}

impl Running {
    /// `without_origin`: a daemon that does not serve the capability, which
    /// a connection cannot then accept.
    fn start(s: &Shop, without_origin: bool) -> Self {
        let tp = TempProfile::new();
        let repo_id;
        {
            let mut profile = tp.open();
            let common = s.f.repo.join(".git").canonicalize().unwrap();
            repo_id = profile.add_repo(&common, None, 1).unwrap().0.repo_id;
            profile.set_mcp_enabled(&repo_id, true, "test", 2).unwrap();
        }
        let vars = std::env::vars_os()
            .filter(|(key, _)| key != "HOME")
            .chain([("HOME".into(), s.f.home.clone().into_os_string())]);
        let env = DaemonEnv::from_vars(vars);
        let mut channel = ChannelConfig {
            // No process is an agent: the reserved `repo.add` never meets agent ancestry.
            agents: AgentMatcher::only(vec!["no-such-agent".to_owned()]),
            ..ChannelConfig::default()
        };
        if without_origin {
            channel.capabilities = capability::all()
                .map(|c| c.name)
                .filter(|name| *name != CAPABILITY)
                .collect();
        }
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
        let r = Self {
            tp,
            handle,
            join: Some(join),
            repo_id,
        };
        drop(r.connect());
        r
    }

    fn dirs(&self) -> ProfileDirs {
        self.tp.dirs()
    }

    /// The library client accepts every capability it knows when it connects.
    fn connect(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(client) => return client,
                Err(ClientError::NotRunning) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    /// A raw connection that never sends `connection.accept`.
    fn bare(&self) -> Raw {
        let mut raw = Raw::open(&gitraptor_core::client::socket_path(&self.dirs()).unwrap());
        raw.call(
            0,
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        raw
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

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Raw {
    fn open(path: &Path) -> Self {
        let stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        Self { stream, reader }
    }

    fn call(&mut self, id: u64, method: &str, params: Value) -> Value {
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
}

// ----- The three channel outputs ----------------------------------------------------------------

fn repo_scope(repo_id: &str) -> Value {
    serde_json::to_value(gitraptor_api::scope::Scope::Repo {
        repo_id: repo_id.to_owned(),
    })
    .unwrap()
}

fn engine_worktrees(snap: &Value, repo_id: &str) -> Option<Value> {
    let repo = snap["repos"]
        .as_array()?
        .iter()
        .find(|r| r["repo_id"] == repo_id)?;
    Some(repo["worktrees"].clone())
}

fn has_all(worktrees: &Value) -> bool {
    worktrees.as_array().is_some_and(|w| w.len() == 3)
}

fn next_worktree_state(next: &mut dyn FnMut() -> Option<Value>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(n) = next()
            && n["method"] == methods::NOTIFY_SCOPE_EVENT
            && n["params"]["event"]["kind"] == gitraptor_api::event::WORKTREE_STATE
        {
            return n["params"]["event"]["data"].clone();
        }
    }
    panic!("no worktree.state");
}

struct Outputs {
    engine: Value,
    scope: Value,
    state: Value,
}

fn outputs_of_raw(r: &Running, s: &Shop) -> Outputs {
    let mut raw = r.bare();
    let scope = repo_scope(&r.repo_id);
    let deadline = Instant::now() + DEADLINE;
    let engine = loop {
        let snap = raw.call(1, methods::ENGINE_SNAPSHOT, json!({}));
        if let Some(w) = engine_worktrees(&snap["result"], &r.repo_id)
            && has_all(&w)
        {
            break w;
        }
        assert!(
            Instant::now() < deadline,
            "the repo never showed 3 worktrees"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let snap = raw.call(2, methods::SCOPE_SNAPSHOT, json!({ "scope": scope }));
    let next = json!({"scope": scope, "from_seq": snap["result"]["scope_seq"].as_u64().unwrap() + 1, "run_id": snap["result"]["run_id"]});
    let sub = raw.call(3, methods::SCOPE_SUBSCRIBE, next);
    assert!(sub["result"].is_object(), "{sub}");
    std::fs::write(s.f.repo.join("nuevo.txt"), "y\n").unwrap();
    let state = next_worktree_state(&mut || {
        let mut line = String::new();
        raw.reader.read_line(&mut line).ok()?;
        serde_json::from_str(&line).ok()
    });
    Outputs {
        engine,
        scope: snap["result"]["repo"]["worktrees"].clone(),
        state: state["worktrees"].clone(),
    }
}

fn outputs_of_client(r: &Running, s: &Shop) -> Outputs {
    let mut client = r.connect();
    let scope = repo_scope(&r.repo_id);
    let deadline = Instant::now() + DEADLINE;
    let engine = loop {
        let snap: Value = client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
        if let Some(w) = engine_worktrees(&snap, &r.repo_id)
            && has_all(&w)
        {
            break w;
        }
        assert!(
            Instant::now() < deadline,
            "the repo never showed 3 worktrees"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let snap: Value = client
        .call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
        .unwrap();
    let next = json!({"scope": scope, "from_seq": snap["scope_seq"].as_u64().unwrap() + 1, "run_id": snap["run_id"]});
    let _: Value = client.call(methods::SCOPE_SUBSCRIBE, next).unwrap();
    std::fs::write(s.f.repo.join("nuevo.txt"), "y\n").unwrap();
    let state = next_worktree_state(&mut || {
        client
            .next_notification(Duration::from_millis(200))
            .unwrap()
            .map(|n| serde_json::to_value(n).unwrap())
    });
    Outputs {
        engine,
        scope: snap["repo"]["worktrees"].clone(),
        state: state["worktrees"].clone(),
    }
}

fn worktree<'a>(worktrees: &'a Value, path: &Path) -> &'a Value {
    worktrees
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["path"]["untrusted"] == path.to_str().unwrap())
        .unwrap_or_else(|| panic!("no worktree {} in {worktrees}", path.display()))
}

#[test]
fn no_output_leaks_origin_without_capability() {
    let s = shop();
    let r = Running::start(&s, false);
    // Control: a connection that accepts the capability sees the field, so
    // its absence below is the daemon's doing.
    let seen = outputs_of_client(&r, &s);
    assert!(
        worktree(&seen.engine, &s.orca).get("origin").is_some(),
        "{CAPABILITY} is accepted by the library client and must show origin: {}",
        seen.engine
    );
    let o = outputs_of_raw(&r, &s);
    for (name, worktrees) in [
        ("engine.snapshot", &o.engine),
        ("scope.snapshot", &o.scope),
        ("worktree.state", &o.state),
    ] {
        let w = worktrees
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no worktrees"));
        assert!(!w.is_empty(), "{name}: {worktrees}");
        assert!(
            w.iter().all(|w| w.get("origin").is_none()),
            "{name}: a connection without {CAPABILITY} must not see origin: {worktrees}"
        );
    }
}

#[test]
fn a_connection_with_the_capability_sees_origin_in_the_three_outputs() {
    let s = shop();
    let r = Running::start(&s, false);
    let o = outputs_of_client(&r, &s);
    for (name, worktrees) in [
        ("engine.snapshot", &o.engine),
        ("scope.snapshot", &o.scope),
        ("worktree.state", &o.state),
    ] {
        assert_eq!(
            worktree(worktrees, &s.orca)["origin"],
            json!({"tool": "orca", "evidence": "admin-marker"}),
            "{name}: a connection with {CAPABILITY} must see the orca mark: {worktrees}"
        );
        assert_eq!(
            worktree(worktrees, &s.manual)["origin"],
            json!({"tool": "unknown", "evidence": "none"}),
            "{name}: a foreign worktree must read unknown/none: {worktrees}"
        );
        let main = worktrees
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["main"] == true)
            .unwrap();
        assert!(
            main.get("origin").is_none(),
            "{name}: the main worktree carries no origin: {main}"
        );
    }
}

/// `repo.add` answers a `RepoView`; the channel strips the field with the
/// same method as the rest of the outputs.
#[test]
fn repo_add_shape_drops_origin_without_capability() {
    let with_origin = json!({
        "repo_id": "r1",
        "state": "observed",
        "path": {"untrusted": "/r/.git"},
        "base": {"name": null, "status": "unconfirmed"},
        "worktrees": [{
            "path": {"untrusted": "/w/feat"},
            "main": false,
            "admin_name": null,
            "status": {"state": "unavailable", "reason": "missing"},
            "origin": {"tool": "orca", "evidence": "admin-marker"}
        }]
    });
    let mut repo: RepoView = serde_json::from_value(with_origin.clone())
        .expect("RepoView must accept a worktree with origin");
    repo.without_worktree_origin();
    let mut expected = with_origin;
    expected["worktrees"][0]
        .as_object_mut()
        .unwrap()
        .remove("origin");
    assert_eq!(
        serde_json::to_value(&repo).unwrap(),
        expected,
        "without_worktree_origin must leave the JSON of before"
    );
}

// ----- The worktrees page of raptor-mcp, from a client process ----------------------------------

/// Entry point of a client process: with `RAPTOR_TEST_ORIGIN_CLIENT` set to
/// `{"root", "out"}` it connects as an `mcp` client, asks `mcp.status` and
/// the page of the other worktrees, and writes both answers to `out`. As a
/// normal test it does nothing.
#[test]
fn origin_mcp_client_entry() {
    let Some(spec) = std::env::var_os(CLIENT_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let first: Value = client.call("mcp.status", json!({})).unwrap();
    let cursor = first
        .pointer("/repo/worktrees/cursor")
        .cloned()
        .unwrap_or(Value::Null);
    let page: Value = client
        .call("mcp.status", json!({ "cursor": cursor }))
        .unwrap_or(Value::Null);
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&json!([first, page])).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

/// The `mcp` page of the other worktrees, asked from the main worktree.
fn mcp_page(r: &Running, s: &Shop) -> McpStatus {
    // Wait until the engine has read the three worktrees.
    let mut c = r.connect();
    let start = Instant::now();
    loop {
        let snap: Value = c.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
        if engine_worktrees(&snap, &r.repo_id).is_some_and(|w| has_all(&w)) {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "never observed");
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = r.tp.root.path().join("answers.json");
    let spec = json!({"root": r.tp.root.path().join("profile"), "out": out});
    let mut child: Child = Command::new(std::env::current_exe().unwrap())
        .args([
            "origin_mcp_client_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(s.f.repo.canonicalize().unwrap())
        .env(CLIENT_ENV, spec.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !out.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(out.exists(), "the client ended ({status}) without answers");
            break;
        }
        assert!(start.elapsed() < DEADLINE, "the client never answered");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.wait();
    let answers: Vec<Value> = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    serde_json::from_value(answers[1].clone())
        .unwrap_or_else(|e| panic!("not a status ({e}): {}", answers[1]))
}

fn page_origin(status: &McpStatus, name: &str) -> Option<Value> {
    let page = status.page.as_ref().expect("a page of worktrees");
    let w = page
        .worktrees
        .iter()
        .find(|w| w.name.raw() == name)
        .unwrap_or_else(|| panic!("no {name} in the page: {page:?}"));
    serde_json::to_value(w).unwrap().get("origin").cloned()
}

#[test]
fn mcp_page_carries_origin_only_with_the_capability() {
    let s = shop();
    let with = Running::start(&s, false);
    let page = mcp_page(&with, &s);
    assert_eq!(
        page_origin(&page, "wt-orca"),
        Some(json!({"tool": "orca", "evidence": "admin-marker"})),
        "the worktrees page of raptor-mcp must carry origin when the connection accepted {CAPABILITY}"
    );
    assert_eq!(
        page_origin(&page, "manual"),
        Some(json!({"tool": "unknown", "evidence": "none"})),
        "a foreign worktree must read unknown/none in the page"
    );
    drop(with);

    let without = Running::start(&s, true);
    let page = mcp_page(&without, &s);
    for name in ["wt-orca", "manual"] {
        assert_eq!(
            page_origin(&page, name),
            None,
            "{name}: the page must not carry origin without {CAPABILITY}"
        );
    }
}
