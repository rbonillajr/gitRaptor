//! US-GRP-008 (T002): the engine publishes `changes_actor: unattributed` in
//! every `ready` worktree with changes, never in a clean or unavailable one,
//! and only to a connection that accepted `observation.change-attribution`.
//! `repo.add` is reserved and an in-process daemon is its own ancestor, so
//! that output is checked in `apps/cli/tests/status_changes_actor.rs`.
//! A real daemon (in-process) over temporary repos and a temporary profile
//! (NFR-01). The field is read from the JSON on purpose. The cases with a
//! Claude Code session or a registered agent need a real process: they live
//! in `apps/cli/tests/editor_unattributed.rs`.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use serde_json::{Value, json};

const CAPABILITY: &str = "observation.change-attribution";

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let config = DaemonConfig {
            dirs: dirs.clone(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: ChannelConfig {
                // No process is an agent: the reserved `repo.add` never meets agent ancestry, whoever runs the tests.
                agents: AgentMatcher::only(vec!["no-such-agent".to_owned()]),
                ..ChannelConfig::default()
            },
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
            dirs,
            handle,
            join: Some(join),
        };
        drop(r.connect());
        r
    }

    /// The library client accepts every capability it knows when it connects.
    fn connect(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(client) => return client,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    /// A raw connection that never sends `connection.accept`.
    fn bare(&self) -> Raw {
        let mut raw = Raw::open(&gitraptor_core::client::socket_path(&self.dirs).unwrap());
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

/// A repo with the main worktree and three linked ones: `dirty` (a modified
/// tracked file), `clean` and `gone` (its folder removed: unavailable).
struct Shop {
    _tmp: tempfile::TempDir,
    main: PathBuf,
    dirty: PathBuf,
    clean: PathBuf,
    gone: PathBuf,
}

fn shop(tmp: tempfile::TempDir, name: &str) -> Shop {
    let main = init_repo(tmp.path(), name, true);
    let mut linked = Vec::new();
    for suffix in ["dirty", "clean", "gone"] {
        let path = tmp.path().join(format!("{name}-{suffix}"));
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                suffix,
                path.to_str().unwrap(),
            ],
        );
        linked.push(path.canonicalize().unwrap());
    }
    std::fs::remove_dir_all(&linked[2]).unwrap();
    std::fs::write(linked[0].join("README"), "hello\nchanged\n").unwrap();
    std::fs::write(main.join("pagos.txt"), "x\n").unwrap();
    Shop {
        main: main.canonicalize().unwrap(),
        dirty: linked[0].clone(),
        clean: linked[1].clone(),
        gone: linked[2].clone(),
        _tmp: tmp,
    }
}

fn repo_scope(repo_id: &str) -> Value {
    serde_json::to_value(gitraptor_api::scope::Scope::Repo {
        repo_id: repo_id.to_owned(),
    })
    .unwrap()
}

fn worktree<'a>(worktrees: &'a Value, path: &Path) -> &'a Value {
    worktrees
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["path"]["untrusted"] == path.to_str().unwrap())
        .unwrap_or_else(|| panic!("no worktree {} in {worktrees}", path.display()))
}

fn has_changes(w: &Value) -> bool {
    w["status"]["counts"]
        .as_object()
        .is_some_and(|c| c.values().any(|n| n.as_u64() != Some(0)))
}

/// Observes `shop` and returns its repo id, a running daemon and the profile.
fn observed(name: &str) -> (Shop, TempProfile, String) {
    let s = shop(tempfile::tempdir().unwrap(), name);
    let tp = TempProfile::new();
    let (entry, _) = tp.open().add_repo(&common_dir(&s.main), None, 1).unwrap();
    (s, tp, entry.repo_id)
}

/// `engine.snapshot` of the repo `repo_id`, once the changes of `s` are read.
fn snapshot_worktrees(client: &mut Client, repo_id: &str, s: &Shop) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snap: Value = client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
        let repo = snap["repos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["repo_id"] == repo_id)
            .unwrap();
        let worktrees = repo["worktrees"].clone();
        if has_changes(worktree(&worktrees, &s.main)) || Instant::now() > deadline {
            return worktrees;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn changes_actor(w: &Value) -> Option<&Value> {
    w.get("changes_actor")
}

#[test]
fn a_dirty_worktree_publishes_changes_actor_unattributed() {
    let (s, tp, repo_id) = observed("shop");
    let r = Running::start(tp.dirs());
    let mut client = r.connect();
    let worktrees = snapshot_worktrees(&mut client, &repo_id, &s);
    let main = worktree(&worktrees, &s.main);
    assert!(has_changes(main), "{main}");
    assert_eq!(
        changes_actor(main),
        Some(&json!({"actor": "unattributed"})),
        "a ready worktree with changes must carry changes_actor unattributed: {main}"
    );
}

#[test]
fn a_clean_worktree_has_no_changes_actor() {
    let (s, tp, repo_id) = observed("shop");
    let r = Running::start(tp.dirs());
    let mut client = r.connect();
    let worktrees = snapshot_worktrees(&mut client, &repo_id, &s);
    // Its dirty siblings do carry it, so the engine did publish the field.
    assert!(
        changes_actor(worktree(&worktrees, &s.dirty)).is_some(),
        "the dirty sibling must carry changes_actor: {worktrees}"
    );
    let clean = worktree(&worktrees, &s.clean);
    assert_eq!(clean["status"]["state"], "ready", "{clean}");
    assert!(!has_changes(clean), "{clean}");
    assert!(
        changes_actor(clean).is_none(),
        "a clean worktree must not carry changes_actor: {clean}"
    );
}

#[test]
fn an_unavailable_worktree_has_no_changes_actor() {
    let (s, tp, repo_id) = observed("shop");
    let r = Running::start(tp.dirs());
    let mut client = r.connect();
    let worktrees = snapshot_worktrees(&mut client, &repo_id, &s);
    assert!(
        changes_actor(worktree(&worktrees, &s.main)).is_some(),
        "the ready worktree with changes must carry changes_actor: {worktrees}"
    );
    let gone = worktree(&worktrees, &s.gone);
    assert_eq!(gone["status"]["state"], "unavailable", "{gone}");
    assert!(
        changes_actor(gone).is_none(),
        "an unavailable worktree must not carry changes_actor: {gone}"
    );
}

#[test]
fn every_worktree_with_changes_carries_it_whatever_its_kind() {
    let (s, tp, repo_id) = observed("shop");
    let r = Running::start(tp.dirs());
    let mut client = r.connect();
    let worktrees = snapshot_worktrees(&mut client, &repo_id, &s);
    // The main worktree and a linked one, each with its own changes.
    for path in [&s.main, &s.dirty] {
        let w = worktree(&worktrees, path);
        assert!(has_changes(w), "{w}");
        assert_eq!(
            changes_actor(w),
            Some(&json!({"actor": "unattributed"})),
            "{}: {w}",
            path.display()
        );
    }
    // It names no path nor text of the repo (SEC-12).
    assert_eq!(
        changes_actor(worktree(&worktrees, &s.main))
            .and_then(Value::as_object)
            .map(|o| o.len()),
        Some(1)
    );
}

/// `engine.snapshot`, `scope.snapshot` and `worktree.state` for
/// one connection: the worktrees each shows for the repos of `s`.
struct Outputs {
    engine: Value,
    scope: Value,
    state: Value,
}

fn outputs_of_raw(r: &Running, repo_id: &str, s: &Shop) -> Outputs {
    let mut raw = r.bare();
    let scope = repo_scope(repo_id);
    let engine = raw.call(1, methods::ENGINE_SNAPSHOT, json!({}));
    let engine = engine["result"]["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["repo_id"] == repo_id)
        .unwrap()["worktrees"]
        .clone();
    let snap = raw.call(2, methods::SCOPE_SNAPSHOT, json!({ "scope": scope }));
    let next = json!({"scope": scope, "from_seq": snap["result"]["scope_seq"].as_u64().unwrap() + 1, "run_id": snap["result"]["run_id"]});
    let sub = raw.call(3, methods::SCOPE_SUBSCRIBE, next);
    assert!(sub["result"].is_object(), "{sub}");
    std::fs::write(s.main.join("nuevo.txt"), "y\n").unwrap();
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

fn outputs_of_client(r: &Running, repo_id: &str, s: &Shop) -> Outputs {
    let mut client = r.connect();
    let scope = repo_scope(repo_id);
    let engine = snapshot_worktrees(&mut client, repo_id, s);
    let snap: Value = client
        .call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
        .unwrap();
    let next = json!({"scope": scope, "from_seq": snap["scope_seq"].as_u64().unwrap() + 1, "run_id": snap["run_id"]});
    let _: Value = client.call(methods::SCOPE_SUBSCRIBE, next).unwrap();
    std::fs::write(s.main.join("nuevo.txt"), "y\n").unwrap();
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

#[test]
fn a_connection_without_the_capability_never_sees_changes_actor() {
    let (s, tp, repo_id) = observed("shop");
    let r = Running::start(tp.dirs());
    // Control: a connection that accepts the capability sees the field, so
    // its absence below is the daemon's doing.
    let mut with = r.connect();
    let seen = snapshot_worktrees(&mut with, &repo_id, &s);
    assert!(
        changes_actor(worktree(&seen, &s.main)).is_some(),
        "{CAPABILITY} is accepted by the library client and must show changes_actor: {seen}"
    );
    let o = outputs_of_raw(&r, &repo_id, &s);
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
            w.iter().all(|w| changes_actor(w).is_none()),
            "{name}: a connection without {CAPABILITY} must not see changes_actor: {worktrees}"
        );
    }
}

#[test]
fn a_connection_with_the_capability_sees_changes_actor_in_the_three_outputs() {
    let (s, tp, repo_id) = observed("shop");
    let r = Running::start(tp.dirs());
    let o = outputs_of_client(&r, &repo_id, &s);
    for (name, worktrees, path) in [
        ("engine.snapshot", &o.engine, &s.main),
        ("scope.snapshot", &o.scope, &s.main),
        ("worktree.state", &o.state, &s.main),
    ] {
        let w = worktree(worktrees, path);
        assert!(has_changes(w), "{name}: {w}");
        assert_eq!(
            changes_actor(w),
            Some(&json!({"actor": "unattributed"})),
            "{name}: a connection with {CAPABILITY} must see changes_actor: {w}"
        );
        let clean_ones = worktrees
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["status"]["state"] == "ready" && !has_changes(w));
        for clean in clean_ones {
            assert!(
                changes_actor(clean).is_none(),
                "{name}: a clean worktree must not carry it: {clean}"
            );
        }
    }
}
