//! The cursor of `mcp.conflicts` keeps the scope it was handed out for (US-MCP-016, D4): a walk
//! of the default scope stays on the caller's pairs, and a cursor does not cross to another repo
//! nor to another worktree of the same connection. A real daemon in-process with the predictor
//! on, the real client library in another process whose working folder moves between calls, a
//! temporary profile, home and testkit repos (NFR-01). No fixed waits: the predictor is waited
//! for by its `prediction.changed` events, the client by its answers; a deadline is a ceiling.
//!
//! Layout: `main` with 60 small files; worktrees `wt-a`, `wt-b` and `wt-c` rewrite line 3 of
//! each, so every pair is a ⚡ of 60 files, far over a page. Six pairs (three between worktrees,
//! three with the base). A second repo, enabled too, with only its main worktree.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_api::prediction::{PairStateView, PairSummary, PredictionChanged};
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeEventNotification, ScopeSnapshot};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(60);
const CLIENT_ENV: &str = "RAPTOR_TEST_MCP_CONFLICTS_CURSOR_CLIENT";
const MCP_CONFLICTS: &str = "mcp.conflicts";
const PAIRS: usize = 6;

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

// ----- The client, in another process ---------------------------------------------------------

/// Entry point of a client process: with the env var set to `{"root", "calls", "out"}` it
/// connects as an `mcp` client and goes through `calls`, each one `[method, params]` or
/// `["#cd", folder]`. A string param `"$N:/pointer"` is the value at `/pointer` of the answer to
/// call number N. As a normal test it does nothing.
#[test]
fn mcp_client_entry() {
    let Some(spec) = std::env::var_os(CLIENT_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let mut answers: Vec<Value> = Vec::new();
    let mut oks: Vec<Value> = Vec::new();
    for call in spec["calls"].as_array().unwrap() {
        let name = call[0].as_str().unwrap();
        if name == "#cd" {
            std::env::set_current_dir(call[1].as_str().unwrap()).unwrap();
            continue;
        }
        let mut params = call[1].clone();
        if let Some(map) = params.as_object_mut() {
            for value in map.values_mut() {
                let Some(reference) = value.as_str().and_then(|s| s.strip_prefix('$')) else {
                    continue;
                };
                let (n, pointer) = reference.split_once(':').unwrap();
                let n: usize = n.parse().unwrap();
                *value = oks[n].pointer(pointer).cloned().unwrap_or(Value::Null);
            }
        }
        let (answer, ok) = match client.call::<_, Value>(name, params) {
            Ok(v) => (json!({ "ok": v }), v),
            Err(ClientError::Rpc(e)) => (json!({ "code": e.code, "data": e.data }), Value::Null),
            Err(e) => (json!({ "other": e.to_string() }), Value::Null),
        };
        answers.push(answer);
        oks.push(ok);
    }
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

struct ClientProc {
    child: Child,
    out: PathBuf,
}

impl ClientProc {
    fn answers(mut self) -> Vec<Value> {
        let start = Instant::now();
        while !self.out.exists() {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    self.out.exists(),
                    "the client ended ({status}) without its answers"
                );
                break;
            }
            assert!(start.elapsed() < DEADLINE, "the client never answered");
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.wait();
        serde_json::from_slice(&std::fs::read(&self.out).unwrap()).unwrap()
    }
}

// ----- The daemon, in-process -----------------------------------------------------------------

struct Running {
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(b: &Built) -> Self {
        let vars = std::env::vars_os()
            .filter(|(key, _)| key != "HOME")
            .chain([("HOME".into(), b.fx.home.clone().into_os_string())]);
        let env = DaemonEnv::from_vars(vars);
        let config = DaemonConfig {
            dirs: b.tp.dirs(),
            git: env.git_resolve_config(None),
            env,
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
            predict: PredictConfig {
                merger: PredictMerger::InProcess,
            },
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            handle,
            join: Some(join),
        }
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

// ----- The repos ------------------------------------------------------------------------------

struct Built {
    tp: TempProfile,
    fx: Fixture,
    wt_a: PathBuf,
    wt_b: PathBuf,
    repo_id: String,
    second_id: String,
}

fn confirm_base(profile: &mut gitraptor_core::profile::Profile, repo_id: &str) {
    let (mut store, _) = profile.open_store(repo_id).unwrap();
    store
        .set_confirmed_team_baseline(&Confirmed {
            base_branch: RefName::new("main").unwrap(),
            floor: ConfirmedFloor::Absent,
        })
        .unwrap();
}

fn build() -> Built {
    let fx = Fixture::with_commit(&git());
    for n in 0..60 {
        fx.write(&format!("f{n:02}.txt"), "1\n2\n3\n4\n5\n");
    }
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "base"]);
    for branch in ["a", "b", "c"] {
        fx.git(&["branch", branch]);
    }
    let wt_a = canonical(&fx.add_worktree("a", "a"));
    let wt_b = canonical(&fx.add_worktree("b", "b"));
    let wt_c = canonical(&fx.add_worktree("c", "c"));
    for (wt, tag) in [(&wt_a, "a"), (&wt_b, "b"), (&wt_c, "c")] {
        for n in 0..60 {
            std::fs::write(
                wt.join(format!("f{n:02}.txt")),
                format!("1\n2\nrewritten-{tag}\n4\n5\n"),
            )
            .unwrap();
        }
        fx.git_in(wt, &["add", "."]);
        fx.git_in(wt, &["commit", "-q", "-m", &format!("work {tag}")]);
    }
    fx.git_in(
        &fx.other_repo,
        &["commit", "-q", "--allow-empty", "-m", "x"],
    );

    let tp = TempProfile::new();
    let (repo_id, second_id);
    {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        repo_id = entry.repo_id;
        confirm_base(&mut profile, &repo_id);
        profile.set_mcp_enabled(&repo_id, true, "test", 2).unwrap();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.other_repo.join(".git")), None, 1)
            .unwrap();
        second_id = entry.repo_id;
        profile
            .set_mcp_enabled(&second_id, true, "test", 3)
            .unwrap();
    }
    Built {
        tp,
        fx,
        wt_a,
        wt_b,
        repo_id,
        second_id,
    }
}

// ----- A full connection that sees the predictor's events -------------------------------------

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    line: String,
    notes: Vec<Value>,
    next_id: u64,
}

impl Raw {
    fn open(tp: &TempProfile, accept: &[&str]) -> Self {
        let path = socket_path(&tp.dirs()).unwrap();
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(stream) => break stream,
                Err(err) => {
                    assert!(
                        start.elapsed() < DEADLINE,
                        "the daemon never listened: {err}"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            line: String::new(),
            notes: Vec::new(),
            next_id: 1,
        };
        let _: Value = raw.result(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        if !accept.is_empty() {
            let _: Value = raw.result(methods::CONNECTION_ACCEPT, json!({"capabilities": accept}));
        }
        raw
    }

    fn read_value(&mut self, timeout: Duration) -> Option<Value> {
        let start = Instant::now();
        loop {
            let left = timeout.saturating_sub(start.elapsed());
            if left.is_zero() {
                return None;
            }
            self.stream.set_read_timeout(Some(left)).unwrap();
            match self.reader.read_line(&mut self.line) {
                Ok(0) => panic!("the daemon closed the connection"),
                Ok(_) if self.line.ends_with('\n') => {
                    let text = std::mem::take(&mut self.line);
                    return Some(serde_json::from_str(&text).unwrap());
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => panic!("read: {e}"),
            }
        }
    }

    fn result<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> T {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        loop {
            let value = self
                .read_value(DEADLINE)
                .unwrap_or_else(|| panic!("no answer to {method}"));
            if value["id"] == id {
                assert!(value.get("error").is_none(), "{method}: {value}");
                return serde_json::from_value(value["result"].clone()).unwrap();
            }
            self.notes.push(value);
        }
    }

    fn next_event(&mut self, timeout: Duration) -> Option<ScopeEventNotification> {
        let start = Instant::now();
        loop {
            let note = if self.notes.is_empty() {
                self.read_value(timeout.saturating_sub(start.elapsed()))?
            } else {
                self.notes.remove(0)
            };
            if note["method"] == methods::NOTIFY_SCOPE_EVENT {
                return Some(serde_json::from_value(note["params"].clone()).unwrap());
            }
        }
    }
}

/// Until the predictor published every pair of `repo_id` as current.
fn settle(tp: &TempProfile, repo_id: &str) {
    let mut raw = Raw::open(tp, &[methods::CAP_PREDICTION_PAIRS.name]);
    let scope = Scope::Repo {
        repo_id: repo_id.to_owned(),
    };
    let snap: ScopeSnapshot = raw.result(methods::SCOPE_SNAPSHOT, json!({"scope": scope}));
    let ScopeSnapshot::Repo(repo) = &snap else {
        panic!("{snap:?}")
    };
    let mut pairs: BTreeMap<String, PairSummary> = repo
        .repo
        .prediction
        .clone()
        .map(|p| p.pairs)
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.pair_id.clone(), p))
        .collect();
    let _: Value = raw.result(
        methods::SCOPE_SUBSCRIBE,
        json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
    );
    let start = Instant::now();
    while !(pairs.len() == PAIRS && pairs.values().all(|p| p.state == PairStateView::Current)) {
        let left = DEADLINE.saturating_sub(start.elapsed());
        assert!(!left.is_zero(), "the predictor never settled");
        let Some(e) = raw.next_event(left) else {
            continue;
        };
        if e.event.kind != methods::PREDICTION_CHANGED {
            continue;
        }
        let changed: PredictionChanged = serde_json::from_value(e.event.data).unwrap();
        for id in &changed.removed {
            pairs.remove(id);
        }
        for pair in changed.pairs {
            pairs.insert(pair.pair_id.clone(), pair);
        }
    }
}

/// Until `repo_id` is published with at least `worktrees` worktrees.
fn wait_repo(tp: &TempProfile, repo_id: &str, worktrees: usize) {
    let mut raw = Raw::open(tp, &[]);
    let start = Instant::now();
    loop {
        let snap: ScopeSnapshot = raw.result(
            methods::SCOPE_SNAPSHOT,
            json!({"scope": Scope::Repo { repo_id: repo_id.to_owned() }}),
        );
        if let ScopeSnapshot::Repo(s) = &snap
            && s.repo.worktrees.len() >= worktrees
        {
            return;
        }
        assert!(start.elapsed() < DEADLINE, "the repo was never ready");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct World {
    b: Built,
    _d: Running,
    n: AtomicUsize,
}

fn world() -> World {
    let b = build();
    let d = Running::start(&b);
    settle(&b.tp, &b.repo_id);
    wait_repo(&b.tp, &b.second_id, 1);
    World {
        b,
        _d: d,
        n: AtomicUsize::new(0),
    }
}

impl World {
    /// An `mcp` client process that starts in `cwd` and makes `calls` on one connection.
    fn run(&self, cwd: &Path, calls: Value) -> Vec<Value> {
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        let out = self.b.tp.root.path().join(format!("answers-{n}.json"));
        let spec = json!({
            "root": self.b.tp.root.path().join("profile"),
            "calls": calls,
            "out": out,
        });
        let child = Command::new(std::env::current_exe().unwrap())
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
            .spawn()
            .unwrap();
        ClientProc { child, out }.answers()
    }
}

fn ok(answer: &Value) -> &Value {
    answer
        .get("ok")
        .unwrap_or_else(|| panic!("mcp.conflicts did not answer with a result: {answer}"))
}

fn side_name(side: &Value) -> String {
    side.pointer("/worktree/untrusted")
        .and_then(Value::as_str)
        .map_or_else(|| "base".to_owned(), str::to_owned)
}

fn cursor_of(answer: &Value) -> String {
    ok(answer)["cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("the first page must carry a cursor: {answer}"))
        .to_owned()
}

// ----- Tests ----------------------------------------------------------------------------------

/// D2/D4: a walk of the default scope with its cursor gives only the caller's pairs, the caller
/// always on side a, and as many as the counts of the first page say.
#[test]
fn a_default_scope_walk_stays_on_the_callers_pairs() {
    let w = world();
    let mut calls = vec![json!([MCP_CONFLICTS, {}])];
    calls.extend((0..PAIRS).map(|i| json!([MCP_CONFLICTS, {"cursor": format!("${i}:/cursor")}])));
    let a = w.run(&w.b.wt_a, Value::Array(calls));

    let first = ok(&a[0]);
    assert_eq!(first["scope"], "worktree", "{first}");
    assert_eq!(
        first["truncated"], true,
        "three ⚡ of 60 files must not fit one page: {first}"
    );
    let counted: u64 = first["counts"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_u64)
        .sum();
    let mut seen: Vec<String> = Vec::new();
    let mut ended = false;
    for answer in &a {
        let page = ok(answer);
        for pair in page["pairs"].as_array().unwrap() {
            let (x, y) = (side_name(&pair["a"]), side_name(&pair["b"]));
            assert_eq!(x, "wt-a", "the caller goes on side a: {pair}");
            assert_ne!(y, "wt-a", "{pair}");
            assert!(!seen.contains(&y), "{y} twice in the walk");
            seen.push(y);
        }
        if page["cursor"].is_null() {
            ended = true;
            break;
        }
    }
    assert!(ended, "the cursor never ended the walk");
    seen.sort();
    assert_eq!(
        seen,
        ["base", "wt-b", "wt-c"],
        "only the caller's pairs, every one of them"
    );
    assert_eq!(
        counted, 3,
        "the counts are the caller's pairs: {}",
        first["counts"]
    );
}

/// D4: a cursor handed out in one repo does not exist for a caller now in another repo, on the
/// same connection.
#[test]
fn a_cursor_of_another_repo_does_not_exist() {
    let w = world();
    let other = canonical(&w.b.fx.other_repo);
    let a = w.run(
        &w.b.wt_a,
        json!([
            [MCP_CONFLICTS, {"scope": "repo"}],
            ["#cd", other],
            [MCP_CONFLICTS, {"cursor": "$0:/cursor"}]
        ]),
    );
    cursor_of(&a[0]);
    assert_eq!(
        a[1]["code"],
        code::NOT_FOUND,
        "a cursor of repo A must not be served in repo B: {}",
        a[1]
    );
}

/// D4: a cursor of the worktree scope does not exist for a caller now in another worktree of the
/// same repo, on the same connection.
#[test]
fn a_worktree_cursor_from_another_worktree_does_not_exist() {
    let w = world();
    let a = w.run(
        &w.b.wt_a,
        json!([
            [MCP_CONFLICTS, {}],
            ["#cd", w.b.wt_b],
            [MCP_CONFLICTS, {"cursor": "$0:/cursor"}]
        ]),
    );
    cursor_of(&a[0]);
    assert_eq!(
        a[1]["code"],
        code::NOT_FOUND,
        "a cursor of wt-a's pairs must not be served to wt-b: {}",
        a[1]
    );
}
