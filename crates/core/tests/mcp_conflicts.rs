//! `mcp.conflicts` end to end (US-MCP-016): what an agent in a worktree is told about the
//! conflicts the engine predicted, with a real daemon (in-process, predictor on
//! `PredictMerger::InProcess`) and the real client library in another process whose working
//! folder is the worktree, over a temporary profile, a temporary home and testkit repos (NFR-01).
//!
//! Answers are read as JSON (the wire), so the tests do not depend on the names the build gives
//! its types. No fixed waits: the predictor is waited for by its own `prediction.changed`
//! events, the client by a signal file or its answers; a deadline is only a ceiling.
//!
//! Layout (see `build`): `main` with `big.txt` (60 lines); worktrees `wt-a`, `wt-b` and `wt-c`.
//! `wt-a` and `wt-b` rewrite lines 40 to 58 of `big.txt` differently (a ⚡ of one file, ranges
//! `[40, 19]`, each line carrying `MARK`); `wt-c` only adds `c.txt`. With `many`, the three also
//! rewrite line 3 of 60 files, so every ⚡ is far over the page target. Pairs of the engine: six
//! (`a-b`, `a-c`, `b-c` and each with the base).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{ClientKind, RepoView};
use gitraptor_api::methods;
use gitraptor_api::prediction::{
    LimitCode, ListPredictionParams, PairStateView, PairSummary, PredictionChanged, PredictionPage,
};
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeEventNotification, ScopeSnapshot};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, PowerConfig, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::power::{FakePowerSource, PowerReading};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(60);
const CLIENT_ENV: &str = "RAPTOR_TEST_MCP_CLIENT";
/// The method under test; a literal so the daemon answers METHOD_NOT_FOUND until it exists.
const MCP_CONFLICTS: &str = "mcp.conflicts";
/// A text that only the lines in conflict carry: it must never reach an answer.
const MARK: &str = "ZQ-LINE-CONTENT-7731";
const PAIRS: usize = 6;

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

// ----- The client, in another process, with its working folder in a worktree ------------------

/// Entry point of a client process: with `RAPTOR_TEST_MCP_CLIENT` set to
/// `{"root", "calls", "out"}` it connects as an `mcp` client and goes through `calls`, each one
/// `[method, params]` or one of `["#wait", file]`, `["#signal", file]`, `["#cd", folder]` and
/// `["#poll", method, params, pointer]` (calls `method` until the answer has a number at
/// `pointer` of at least 1, at most 29 times; only the last answer is kept). A string param
/// `"$N:/pointer"` is the value at `/pointer` of the answer to call number N. The answers go to
/// `out` at the end. As a normal test it does nothing.
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
    let call_once = |client: &mut Client, method: &str, params: Value| match client
        .call::<_, Value>(method, params)
    {
        Ok(v) => (json!({ "ok": v }), v),
        Err(ClientError::Rpc(e)) => (json!({ "code": e.code, "data": e.data }), Value::Null),
        Err(e) => (json!({ "other": e.to_string() }), Value::Null),
    };
    for call in spec["calls"].as_array().unwrap() {
        let name = call[0].as_str().unwrap();
        match name {
            "#wait" => {
                let file = PathBuf::from(call[1].as_str().unwrap());
                let start = Instant::now();
                while !file.exists() {
                    assert!(start.elapsed() < DEADLINE, "the signal never came");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "#signal" => std::fs::write(call[1].as_str().unwrap(), b"").unwrap(),
            "#cd" => std::env::set_current_dir(call[1].as_str().unwrap()).unwrap(),
            "#poll" => {
                let (method, params, pointer) = (
                    call[1].as_str().unwrap(),
                    call[2].clone(),
                    call[3].as_str().unwrap(),
                );
                let mut last = (Value::Null, Value::Null);
                for _ in 0..29 {
                    last = call_once(&mut client, method, params.clone());
                    if last.1.pointer(pointer).and_then(Value::as_u64) >= Some(1) {
                        break;
                    }
                }
                answers.push(last.0);
                oks.push(last.1);
            }
            method => {
                let mut params = call[1].clone();
                if let Some(map) = params.as_object_mut() {
                    for value in map.values_mut() {
                        let Some(reference) = value.as_str().and_then(|s| s.strip_prefix('$'))
                        else {
                            continue;
                        };
                        let (n, pointer) = reference.split_once(':').unwrap();
                        let n: usize = n.parse().unwrap();
                        *value = oks[n].pointer(pointer).cloned().unwrap_or(Value::Null);
                    }
                }
                let (answer, ok) = call_once(&mut client, method, params);
                answers.push(answer);
                oks.push(ok);
            }
        }
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
    /// The answers, once the client wrote them; fails if it ended without.
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
    fn start(b: &Built, fake: Option<&FakePowerSource>) -> Self {
        Self::start_with(b, fake, PredictMerger::InProcess)
    }

    fn start_with(b: &Built, fake: Option<&FakePowerSource>, merger: PredictMerger) -> Self {
        // The daemon's `HOME` is the fixture's: a temporary folder with its own Git config.
        let vars = std::env::vars_os()
            .filter(|(key, _)| key != "HOME")
            .chain([("HOME".into(), b.fx.home.clone().into_os_string())]);
        let env = DaemonEnv::from_vars(vars);
        let power = match fake {
            Some(fake) => PowerConfig {
                source: Arc::new(fake.clone()),
                poll: Duration::from_millis(50),
            },
            None => PowerConfig::default(),
        };
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
            power,
            predict: PredictConfig { merger },
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

struct Opts {
    /// Every ⚡ is over the page target: 60 files in conflict per pair.
    many: bool,
    /// The repo is in the MCP allowlist.
    enabled: bool,
    /// A second observed repo outside the allowlist.
    second: bool,
}

impl Opts {
    fn new() -> Self {
        Self {
            many: false,
            enabled: true,
            second: false,
        }
    }
}

struct Built {
    tp: TempProfile,
    fx: Fixture,
    wt_a: PathBuf,
    repo_id: String,
    second_id: Option<String>,
}

fn big(rewrite: Option<&str>) -> String {
    (1..=60)
        .map(|n| match rewrite {
            Some(tag) if (40..=58).contains(&n) => format!("{MARK}-{tag}-{n}\n"),
            _ => format!("line {n}\n"),
        })
        .collect()
}

fn commit_all(fx: &Fixture, wt: &Path, message: &str) {
    fx.git_in(wt, &["add", "."]);
    fx.git_in(wt, &["commit", "-q", "-m", message]);
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

fn build(opts: &Opts) -> Built {
    let fx = Fixture::with_commit(&git());
    fx.write("big.txt", &big(None));
    if opts.many {
        for n in 0..60 {
            fx.write(&format!("f{n:02}.txt"), "1\n2\n3\n4\n5\n");
        }
    }
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "base"]);
    for branch in ["a", "b", "c"] {
        fx.git(&["branch", branch]);
    }
    let wt_a = canonical(&fx.add_worktree("a", "a"));
    let wt_b = canonical(&fx.add_worktree("b", "b"));
    let wt_c = canonical(&fx.add_worktree("c", "c"));
    for (wt, tag) in [(&wt_a, "a"), (&wt_b, "b")] {
        std::fs::write(wt.join("big.txt"), big(Some(tag))).unwrap();
    }
    std::fs::write(wt_c.join("c.txt"), "c\n").unwrap();
    if opts.many {
        for (wt, tag) in [(&wt_a, "a"), (&wt_b, "b"), (&wt_c, "c")] {
            for n in 0..60 {
                std::fs::write(
                    wt.join(format!("f{n:02}.txt")),
                    format!("1\n2\nrewritten-{tag}\n4\n5\n"),
                )
                .unwrap();
            }
        }
    }
    for (wt, tag) in [(&wt_a, "a"), (&wt_b, "b"), (&wt_c, "c")] {
        commit_all(&fx, wt, &format!("work {tag}"));
    }

    let tp = TempProfile::new();
    let (repo_id, second_id);
    {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        repo_id = entry.repo_id;
        confirm_base(&mut profile, &repo_id);
        if opts.enabled {
            profile.set_mcp_enabled(&repo_id, true, "test", 2).unwrap();
        }
        second_id = opts.second.then(|| {
            fx.git_in(
                &fx.other_repo,
                &["commit", "-q", "--allow-empty", "-m", "x"],
            );
            let (entry, _) = profile
                .add_repo(&canonical(&fx.other_repo.join(".git")), None, 1)
                .unwrap();
            entry.repo_id
        });
    }
    Built {
        tp,
        fx,
        wt_a,
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

/// A connection that negotiated `prediction.pairs`, subscribed to the repo scope, with the pairs
/// it knows from the snapshot and the `prediction.changed` since.
struct Watch {
    raw: Raw,
    repo_id: String,
    pairs: BTreeMap<String, PairSummary>,
}

impl Watch {
    fn start(tp: &TempProfile, repo_id: &str) -> Self {
        let mut raw = Raw::open(tp, &[methods::CAP_PREDICTION_PAIRS.name]);
        let scope = Scope::Repo {
            repo_id: repo_id.to_owned(),
        };
        let snap: ScopeSnapshot = raw.result(methods::SCOPE_SNAPSHOT, json!({"scope": scope}));
        let ScopeSnapshot::Repo(repo) = &snap else {
            panic!("{snap:?}")
        };
        let pairs = repo
            .repo
            .prediction
            .clone()
            .expect("RepoView.prediction for a connection with prediction.pairs")
            .pairs
            .into_iter()
            .map(|p| (p.pair_id.clone(), p))
            .collect();
        let _: Value = raw.result(
            methods::SCOPE_SUBSCRIBE,
            json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
        );
        Self {
            raw,
            repo_id: repo_id.to_owned(),
            pairs,
        }
    }

    /// Reads events until `done`, within the deadline.
    fn pump(&mut self, what: &str, done: impl Fn(&BTreeMap<String, PairSummary>) -> bool) {
        let start = Instant::now();
        while !done(&self.pairs) {
            let left = DEADLINE.saturating_sub(start.elapsed());
            assert!(
                !left.is_zero(),
                "no prediction.changed brought {what}; pairs: {:?}",
                self.pairs
                    .values()
                    .map(|p| (p.state, p.reason, p.level))
                    .collect::<Vec<_>>()
            );
            let Some(e) = self.raw.next_event(left) else {
                continue;
            };
            if e.event.kind != methods::PREDICTION_CHANGED {
                continue;
            }
            let changed: PredictionChanged = serde_json::from_value(e.event.data).unwrap();
            for id in &changed.removed {
                self.pairs.remove(id);
            }
            for pair in changed.pairs {
                self.pairs.insert(pair.pair_id.clone(), pair);
            }
        }
    }

    fn settle(&mut self, expected: usize) {
        self.pump("every pair current", |pairs| {
            pairs.len() == expected && pairs.values().all(|p| p.state == PairStateView::Current)
        });
    }

    fn list(&mut self) -> PredictionPage {
        let params = ListPredictionParams {
            repo_id: self.repo_id.clone(),
            ..Default::default()
        };
        self.raw.result(
            methods::PREDICTION_LIST,
            serde_json::to_value(params).unwrap(),
        )
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
            && ready(&s.repo, worktrees)
        {
            return;
        }
        assert!(start.elapsed() < DEADLINE, "the repo was never ready");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn ready(repo: &RepoView, worktrees: usize) -> bool {
    repo.worktrees.len() >= worktrees
}

/// A world ready to ask: the daemon up and the predictor settled on its six pairs.
struct World {
    b: Built,
    _d: Running,
    n: AtomicUsize,
}

fn world(opts: &Opts) -> World {
    let b = build(opts);
    let d = Running::start(&b, None);
    Watch::start(&b.tp, &b.repo_id).settle(PAIRS);
    World {
        b,
        _d: d,
        n: AtomicUsize::new(0),
    }
}

impl World {
    fn spawn_client(&self, cwd: &Path, calls: Value) -> ClientProc {
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
        ClientProc { child, out }
    }

    /// An `mcp` client process with its working folder in `cwd`.
    fn run(&self, cwd: &Path, calls: Value) -> Vec<Value> {
        self.spawn_client(cwd, calls).answers()
    }
}

// ----- What the answers say -------------------------------------------------------------------

/// The result of an answer that must be one: a daemon without the method fails here.
fn ok<'a>(criterion: &str, answer: &'a Value) -> &'a Value {
    answer.get("ok").unwrap_or_else(|| {
        panic!("{criterion}: mcp.conflicts did not answer with a result: {answer}")
    })
}

/// The code and the data of an answer that is an error.
fn error(answer: &Value) -> (i64, Option<Value>) {
    let code = answer["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("not an error: {answer}"));
    (code, answer.get("data").filter(|d| !d.is_null()).cloned())
}

fn side_name(side: &Value) -> String {
    side.pointer("/worktree/untrusted")
        .and_then(Value::as_str)
        .map_or_else(|| "base".to_owned(), str::to_owned)
}

/// `(a, b)` by folder name, `base` for the base.
fn names(pair: &Value) -> (String, String) {
    (side_name(&pair["a"]), side_name(&pair["b"]))
}

fn pairs<'a>(criterion: &str, result: &'a Value) -> &'a Vec<Value> {
    result["pairs"]
        .as_array()
        .unwrap_or_else(|| panic!("{criterion}: the answer has no pairs: {result}"))
}

fn pair_of<'a>(pairs: &'a [Value], a: &str, b: &str) -> Option<&'a Value> {
    pairs.iter().find(|p| {
        names(p) == (a.to_owned(), b.to_owned()) || names(p) == (b.to_owned(), a.to_owned())
    })
}

fn strings(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::String(s) => into.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|i| strings(i, into)),
        Value::Object(map) => map.values().for_each(|v| strings(v, into)),
        _ => {}
    }
}

// ----- Tests ----------------------------------------------------------------------------------

/// Esc. 1: the agent of `wt-a` gets the ⚡ with `wt-b`: level, path, the ranges of each side and
/// an age the daemon computed.
#[test]
fn the_agent_sees_its_predicted_conflict_with_ranges_and_age() {
    let w = world(&Opts::new());
    let before = now_ms();
    let a = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {}]]));
    let after = now_ms();
    let result = ok("MCP016-CONFLICT-SEEN", &a[0]);

    assert_eq!(result["scope"], "worktree");
    assert_eq!(result["worktree"]["untrusted"], "wt-a");
    let all = pairs("MCP016-CONFLICT-SEEN", result);
    let ab = pair_of(all, "wt-a", "wt-b")
        .unwrap_or_else(|| panic!("MCP016-CONFLICT-SEEN: no pair of wt-a with wt-b: {result}"));
    assert_eq!(names(ab).0, "wt-a", "the caller goes on side a");
    assert_eq!(ab["level"], "conflict");
    assert!(
        ab.get("state").is_none(),
        "a current pair omits its state: {ab}"
    );
    let file = &ab["conflict"]["files"][0];
    assert_eq!(file["path"]["untrusted"], "big.txt");
    assert_eq!(file["a"], json!([[40, 19]]), "{file}");
    assert_eq!(file["b"], json!([[40, 19]]), "{file}");

    let computed = ab["computed_at_utc_ms"]
        .as_i64()
        .unwrap_or_else(|| panic!("MCP016-CONFLICT-SEEN: no computed_at_utc_ms: {ab}"));
    let age = ab["age_s"]
        .as_i64()
        .unwrap_or_else(|| panic!("MCP016-CONFLICT-SEEN: no age_s: {ab}"));
    assert!(
        age >= (before - computed) / 1000 - 1 && age <= (after - computed) / 1000 + 1,
        "MCP016-CONFLICT-SEEN: age_s {age} is not now - computed_at ({computed}) by the daemon's clock"
    );
}

/// Esc. 1: no answer has the content of the lines in conflict nor a diff.
#[test]
fn no_file_content_nor_diff_in_any_answer() {
    let w = world(&Opts::new());
    let a = w.run(
        &w.b.wt_a,
        json!([[MCP_CONFLICTS, {}], [MCP_CONFLICTS, {"scope": "repo"}]]),
    );
    for answer in &a {
        let result = ok("MCP016-NO-CONTENT", answer);
        let has_files = pairs("MCP016-NO-CONTENT", result).iter().any(|p| {
            p["conflict"]["files"]
                .as_array()
                .is_some_and(|f| !f.is_empty())
        });
        assert!(
            has_files,
            "MCP016-NO-CONTENT: the fixture has a ⚡ with its file: {result}"
        );
        let text = result.to_string();
        for forbidden in [MARK, "@@", "<<<<<<<", ">>>>>>>"] {
            assert!(
                !text.contains(forbidden),
                "MCP016-NO-CONTENT: `{forbidden}` in the answer: {text}"
            );
        }
        let mut all = Vec::new();
        strings(result, &mut all);
        for s in all {
            assert!(
                !s.lines().any(|l| l.starts_with('+') || l.starts_with('-')),
                "MCP016-NO-CONTENT: a diff line in a string of the answer: {s:?}"
            );
        }
    }
}

/// Esc. 2: without a scope, only the pairs the caller is a side of.
#[test]
fn by_default_only_the_callers_pairs() {
    let w = world(&Opts::new());
    let a = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {}]]));
    let result = ok("MCP016-DEFAULT-SCOPE", &a[0]);
    let all = pairs("MCP016-DEFAULT-SCOPE", result);
    assert_eq!(
        all.len(),
        3,
        "MCP016-DEFAULT-SCOPE: wt-a is in three pairs (wt-b, wt-c, the base): {result}"
    );
    for pair in all {
        assert_eq!(
            names(pair).0,
            "wt-a",
            "MCP016-DEFAULT-SCOPE: a pair of others in the default scope: {pair}"
        );
    }
    assert!(pair_of(all, "wt-b", "wt-c").is_none());
}

/// Esc. 2: with the repo scope, every pair of the repo.
#[test]
fn the_repo_scope_gives_every_pair() {
    let w = world(&Opts::new());
    let a = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {"scope": "repo"}]]));
    let result = ok("MCP016-REPO-SCOPE", &a[0]);
    assert_eq!(result["scope"], "repo");
    let all = pairs("MCP016-REPO-SCOPE", result);
    assert_eq!(all.len(), PAIRS, "MCP016-REPO-SCOPE: {result}");
    assert!(
        pair_of(all, "wt-b", "wt-c").is_some(),
        "MCP016-REPO-SCOPE: the pair of the others is missing: {result}"
    );
}

/// Esquema (pendiente): on battery every pair waits; the answer says `pending` with its reason,
/// no level, and invents no pair.
#[test]
fn declares_a_pending_prediction_with_its_reason_without_inventing_pairs() {
    let b = build(&Opts::new());
    let fake = FakePowerSource::new(PowerReading::Ac);
    let d = Running::start(&b, Some(&fake));
    let mut watch = Watch::start(&b.tp, &b.repo_id);
    watch.settle(PAIRS);
    fake.set(PowerReading::Battery);
    watch.pump("every pair pending (power-saving)", |pairs| {
        pairs.len() == PAIRS && pairs.values().all(|p| p.state == PairStateView::Pending)
    });
    let w = World {
        b,
        _d: d,
        n: AtomicUsize::new(0),
    };
    let a = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {}]]));
    let result = ok("MCP016-PENDING", &a[0]);

    assert_eq!(result["counts"]["pending"], 3, "{result}");
    let all = pairs("MCP016-PENDING", result);
    assert_eq!(all.len(), 3, "no pair is invented: {result}");
    for pair in all {
        assert_eq!(pair["state"], "pending", "{pair}");
        assert_eq!(pair["reason"], "power-saving", "{pair}");
        assert!(pair.get("level").is_none(), "pending has no level: {pair}");
    }
    assert!(result["counts"].get("conflict").is_none(), "{result}");
}

/// Esc. 4: the limits are the ones the engine published, in its order.
#[test]
fn the_limits_are_declared_as_the_engine_published_them() {
    let w = world(&Opts::new());
    let mut watch = Watch::start(&w.b.tp, &w.b.repo_id);
    let published = watch.list().limits;
    assert_eq!(published, LimitCode::ALL.to_vec());
    let a = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {}]]));
    let result = ok("MCP016-LIMITS", &a[0]);
    assert_eq!(
        result["limits"],
        serde_json::to_value(&published).unwrap(),
        "MCP016-LIMITS: the eight codes of the engine, as it published them"
    );
}

/// P6/D11: a caller in the common directory of a bare repo, under no worktree, gets the whole
/// repo with its reason and no worktree.
#[test]
fn an_unidentified_worktree_gives_the_repo_scope_with_its_reason() {
    let fx = Fixture::with_commit(&git());
    fx.write("big.txt", &big(None));
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "base"]);
    let bare = fx.root.join("bare.git");
    fx.git(&[
        "clone",
        "-q",
        "--bare",
        fx.repo.to_str().unwrap(),
        bare.to_str().unwrap(),
    ]);
    let (wt_x, wt_y) = (fx.root.join("wt-bx"), fx.root.join("wt-by"));
    for (wt, branch, tag) in [(&wt_x, "bx", "a"), (&wt_y, "by", "b")] {
        fx.git_in(
            &bare,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                wt.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(wt.join("big.txt"), big(Some(tag))).unwrap();
        commit_all(&fx, wt, "work");
    }
    let tp = TempProfile::new();
    let repo_id = {
        let mut profile = tp.open();
        let (entry, _) = profile.add_repo(&canonical(&bare), None, 1).unwrap();
        confirm_base(&mut profile, &entry.repo_id);
        profile
            .set_mcp_enabled(&entry.repo_id, true, "test", 2)
            .unwrap();
        entry.repo_id
    };
    let b = Built {
        tp,
        fx,
        wt_a: wt_x,
        repo_id: repo_id.clone(),
        second_id: None,
    };
    let d = Running::start(&b, None);
    wait_repo(&b.tp, &repo_id, 2);
    Watch::start(&b.tp, &repo_id).pump("the pair of the two worktrees current", |pairs| {
        pairs.values().any(|p| {
            p.state == PairStateView::Current
                && p.level == Some(gitraptor_api::prediction::PairLevel::Conflict)
        })
    });
    let w = World {
        b,
        _d: d,
        n: AtomicUsize::new(0),
    };
    let a = w.run(&canonical(&bare), json!([[MCP_CONFLICTS, {}]]));
    let result = ok("MCP016-P6-UNKNOWN", &a[0]);

    assert_eq!(result["scope"], "repo", "{result}");
    assert_eq!(result["reason"], "caller-worktree-unknown", "{result}");
    assert!(
        result.get("worktree").is_none(),
        "no worktree to name: {result}"
    );
    let all = pairs("MCP016-P6-UNKNOWN", result);
    assert!(
        pair_of(all, "wt-bx", "wt-by").is_some_and(|p| p["level"] == "conflict"),
        "MCP016-P6-UNKNOWN: the pairs of the whole repo, the ⚡ of the two worktrees: {result}"
    );
}

/// P6: a repo outside the allowlist is `repo-not-enabled`, without a datum of the repo.
#[test]
fn outside_the_allowlist_is_the_repo_not_enabled_refusal() {
    let mut opts = Opts::new();
    opts.second = true;
    let b = build(&opts);
    let d = Running::start(&b, None);
    wait_repo(&b.tp, &b.repo_id, 4);
    wait_repo(&b.tp, b.second_id.as_ref().unwrap(), 1);
    let w = World {
        b,
        _d: d,
        n: AtomicUsize::new(0),
    };
    let other = canonical(&w.b.fx.other_repo);
    let a = w.run(&other, json!([[MCP_CONFLICTS, {}]]));

    let (c, data) = error(&a[0]);
    assert_eq!(c, code::SCOPE_REFUSED, "MCP016-P6-ALLOWLIST: {}", a[0]);
    assert_eq!(data, Some(json!({"reason": "not-allowlisted"})));
    let text = a[0].to_string();
    for secret in [
        other.display().to_string(),
        w.b.fx.root.display().to_string(),
        w.b.second_id.clone().unwrap(),
        "other-repo".to_owned(),
    ] {
        assert!(!text.contains(&secret), "{secret} in the refusal: {text}");
    }
}

/// D5: the page lists ⚡ first and the cursor walks the rest without a repeat or a loss. One
/// connection, because a cursor dies with it: each call takes the cursor of the one before.
#[test]
fn a_conflicts_cursor_pages_without_duplicates() {
    let mut opts = Opts::new();
    opts.many = true;
    let w = world(&opts);
    let mut calls = vec![json!([MCP_CONFLICTS, {"scope": "repo"}])];
    calls.extend((0..PAIRS).map(|i| json!([MCP_CONFLICTS, {"cursor": format!("${i}:/cursor")}])));
    let a = w.run(&w.b.wt_a, Value::Array(calls));

    assert_eq!(
        ok("MCP016-ORDER", &a[0])["truncated"],
        true,
        "MCP016-ORDER: three ⚡ of 60 files are over a page of 800 tokens: {}",
        a[0]
    );
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut ranks: Vec<u8> = Vec::new();
    let mut ended = false;
    for answer in &a {
        let page = ok("MCP016-ORDER", answer);
        for pair in pairs("MCP016-ORDER", page) {
            let (x, y) = names(pair);
            let key = if x <= y { (x, y) } else { (y, x) };
            assert!(!seen.contains(&key), "MCP016-ORDER: {key:?} twice");
            seen.push(key);
            ranks.push(match pair["level"].as_str() {
                Some("conflict") => 0,
                Some("overlap") => 1,
                _ => 2,
            });
        }
        if page["cursor"].is_null() {
            ended = true;
            break;
        }
    }
    assert!(ended, "MCP016-ORDER: the cursor never ended the walk");
    assert_eq!(seen.len(), PAIRS, "MCP016-ORDER: no pair lost: {seen:?}");
    assert!(
        ranks.windows(2).all(|r| r[0] <= r[1]),
        "MCP016-ORDER: ⚡ before ⚠ before the rest: {ranks:?}"
    );
}

/// D4: a cursor minted on another connection does not exist.
#[test]
fn a_conflicts_cursor_from_another_connection_does_not_exist() {
    let mut opts = Opts::new();
    opts.many = true;
    let w = world(&opts);
    let first = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {"scope": "repo"}]]));
    let cursor = ok("MCP016-CURSOR-DAEMON", &first[0])["cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("MCP016-CURSOR-DAEMON: no cursor to carry: {}", first[0]))
        .to_owned();
    let other = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {"cursor": cursor}]]));
    assert_eq!(
        error(&other[0]).0,
        code::NOT_FOUND,
        "MCP016-CURSOR-DAEMON: a cursor of another connection must not be served: {}",
        other[0]
    );
}

/// D4: a cursor of `mcp.conflicts` is not one of `mcp.status`, on the same connection and repo.
#[test]
fn a_conflicts_cursor_is_not_a_status_cursor() {
    let mut opts = Opts::new();
    opts.many = true;
    let w = world(&opts);
    let a = w.run(
        &w.b.wt_a,
        json!([
            [MCP_CONFLICTS, {"scope": "repo"}],
            ["mcp.status", {"cursor": "$0:/cursor"}]
        ]),
    );
    assert!(
        ok("MCP016-CURSOR-REVERSE", &a[0])["cursor"].is_string(),
        "MCP016-CURSOR-REVERSE: the first page must carry a cursor: {}",
        a[0]
    );
    assert_eq!(
        error(&a[1]).0,
        code::NOT_FOUND,
        "MCP016-CURSOR-REVERSE: mcp.status served a cursor of mcp.conflicts: {}",
        a[1]
    );
}

/// D10: every `mcp.conflicts` spends the read bucket of the profile, and over it the call is
/// `RATE_LIMITED`.
#[test]
fn conflicts_spend_the_read_bucket() {
    let w = world(&Opts::new());
    let calls: Vec<Value> = (0..40).map(|_| json!([MCP_CONFLICTS, {}])).collect();
    let a = w.run(&w.b.wt_a, Value::Array(calls));
    ok("MCP016-RATE", &a[0]);
    let first_limited = a
        .iter()
        .position(|answer| answer["code"] == code::RATE_LIMITED)
        .unwrap_or_else(|| panic!("MCP016-RATE: 40 calls in a row never hit RATE_LIMITED"));
    assert!(
        (25..=32).contains(&first_limited),
        "MCP016-RATE: the burst of the read bucket is 30; the first refusal came at call {first_limited}"
    );
    assert!(a[..first_limited].iter().all(|x| x.get("ok").is_some()));
}

/// D13: a repo in the allowlist for which the predictor published nothing (the predictor is off
/// in this daemon) is not a refusal: an answer with no pairs and `unpublished: true`.
#[test]
fn an_unpublished_prediction_is_declared_on_the_wire() {
    let b = build(&Opts::new());
    let d = Running::start_with(&b, None, PredictMerger::Off);
    wait_repo(&b.tp, &b.repo_id, 4);
    let w = World {
        b,
        _d: d,
        n: AtomicUsize::new(0),
    };
    let a = w.run(&w.b.wt_a, json!([[MCP_CONFLICTS, {}]]));
    let result = ok("MCP016-UNPUBLISHED", &a[0]);
    assert_eq!(
        result["unpublished"], true,
        "MCP016-UNPUBLISHED: nothing published must be declared: {result}"
    );
    assert!(
        result["pairs"].as_array().is_none_or(|p| p.is_empty()),
        "MCP016-UNPUBLISHED: no pair is invented: {result}"
    );
}
