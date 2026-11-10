//! US-CKP-012 D2 to D4 and SEC-05: the on-demand queries of the daemon (`diff.worktree`,
//! `query.cancel`), asked of a real daemon (in-process) over a raw connection: out-of-order
//! replies, cancellation, one query in flight per connection, MCP closed to the diff, nothing
//! persisted or published, and the limits. A temporary profile and temporary repos (NFR-01).
//! No fixed waits: each test waits on the answer or the event that proves the fact.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#![allow(dead_code)]

mod common;

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::diff::{
    CommittedDiff, DiffCut, DiffParams, DiffSection, FileBody, WorktreeDiff,
};
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_core::channel::EventBus;
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

// ------------------------------------------------------------------------------ the daemon

const DEADLINE: Duration = Duration::from_secs(30);
/// `query-busy`, `query-cancelled` and `diff-unavailable` (DS-US-CKP-012 § Contrato de API).
const QUERY_BUSY: i64 = -33120;
const QUERY_CANCELLED: i64 = -33121;
const DIFF_UNAVAILABLE: i64 = -33140;

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

struct Running {
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    bus: Arc<EventBus>,
}

impl Running {
    fn start(tp: &TempProfile) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let config = DaemonConfig {
            dirs: tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: Default::default(),
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
        let bus = daemon.events();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            handle,
            join: Some(join),
            bus,
        }
    }

    /// An event on the global scope: the proof that the stream flows.
    fn marker(&self) -> u64 {
        self.bus
            .publish(RESERVED_AUDIT, json!({"n": 1}), None, |_| {})
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

// ------------------------------------------------------------------------------ the repo

struct Env {
    tp: TempProfile,
    wt: PathBuf,
    repo_id: String,
}

/// `build` creates branch `feat` (and its work) in a repo with `main`; the worktree is
/// `wt-feat`. With `confirm`, `main` is the confirmed base. Nothing starts a daemon.
fn env_with(fx: &Fixture, wt: PathBuf, confirm: bool) -> Env {
    let wt = canonical(&wt);
    let tp = TempProfile::new();
    let repo_id = {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        if confirm {
            let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
            store
                .set_confirmed_team_baseline(&Confirmed {
                    base_branch: RefName::new("main").unwrap(),
                    floor: ConfirmedFloor::Absent,
                })
                .unwrap();
        }
        entry.repo_id
    };
    Env { tp, wt, repo_id }
}

fn commit_in(fx: &Fixture, wt: &Path, msg: &str) {
    fx.git_in(wt, &["add", "-A"]);
    fx.git_in(wt, &["commit", "-q", "-m", msg]);
}

// ------------------------------------------------------------------------------ the wire

/// A raw connection: lines out (several in one write when asked), lines in. Responses wait in
/// `answers` by id, in the order they arrived (`order`); notifications wait in `notes`.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    line: String,
    next_id: u64,
    answers: HashMap<u64, Value>,
    order: Vec<u64>,
    notes: VecDeque<Value>,
    log: Vec<String>,
}

impl Raw {
    fn open(tp: &TempProfile, client: &str) -> Self {
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
            next_id: 1,
            answers: HashMap::new(),
            order: Vec::new(),
            notes: VecDeque::new(),
            log: Vec::new(),
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": client, "client_version": "t"}),
        );
        assert!(hello.get("error").is_none(), "hello: {hello}");
        raw
    }

    fn cli(tp: &TempProfile) -> Self {
        Self::open(tp, "cli")
    }

    fn write(&mut self, requests: &[(&str, Value)]) -> Vec<u64> {
        let mut out = String::new();
        let mut ids = Vec::new();
        for (method, params) in requests {
            let id = self.next_id;
            self.next_id += 1;
            ids.push(id);
            out.push_str(
                &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
                    .to_string(),
            );
            out.push('\n');
        }
        self.stream.write_all(out.as_bytes()).unwrap();
        ids
    }

    /// One line, or `None` when `timeout` ran out first.
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
                    let value: Value = serde_json::from_str(&text).unwrap();
                    self.log.push(text);
                    return Some(value);
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

    /// Reads one line and files it: a response by id, anything else as a note.
    fn pump(&mut self, timeout: Duration) -> bool {
        let Some(value) = self.read_value(timeout) else {
            return false;
        };
        match value["id"].as_u64() {
            Some(id) if value.get("method").is_none() => {
                self.order.push(id);
                self.answers.insert(id, value);
            }
            _ => self.notes.push_back(value),
        }
        true
    }

    /// The answer to request `id`, whatever arrives before it.
    fn answer(&mut self, id: u64) -> Value {
        let start = Instant::now();
        loop {
            if let Some(v) = self.answers.remove(&id) {
                return v;
            }
            let left = DEADLINE.saturating_sub(start.elapsed());
            assert!(self.pump(left), "no answer to request {id}");
        }
    }

    /// Whether the answer to `id` has already arrived (without waiting).
    fn arrived(&mut self, id: u64) -> bool {
        while self.pump(Duration::from_millis(1)) {}
        self.answers.contains_key(&id)
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.write(&[(method, params)])[0];
        self.answer(id)
    }
}

fn code_of(answer: &Value) -> Option<i64> {
    answer["error"]["code"].as_i64()
}

fn params(e: &Env) -> Value {
    serde_json::to_value(DiffParams {
        repo_id: e.repo_id.clone(),
        worktree: e.wt.to_string_lossy().into_owned(),
    })
    .unwrap()
}

/// `diff.worktree` of the worktree of `e`, asked until the engine has the repo ready (the
/// daemon answers `diff-unavailable` while it has not observed it yet). Any other answer, an
/// error included, is returned as it is.
fn ask_diff(raw: &mut Raw, e: &Env) -> Value {
    let start = Instant::now();
    loop {
        let answer = raw.call(methods::DIFF_WORKTREE, params(e));
        if code_of(&answer) != Some(DIFF_UNAVAILABLE) || start.elapsed() > DEADLINE {
            return answer;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The typed result of `diff.worktree`, or a failure naming what the daemon answered.
fn diff_of(answer: &Value) -> WorktreeDiff {
    assert!(
        answer.get("error").is_none(),
        "the daemon does not serve diff.worktree: {answer}"
    );
    serde_json::from_value(answer["result"].clone())
        .unwrap_or_else(|e| panic!("diff.worktree result is not a WorktreeDiff ({e}): {answer}"))
}

fn paths(section: &DiffSection) -> Vec<String> {
    let mut out: Vec<String> = section
        .files
        .iter()
        .map(|f| f.path.raw().to_owned())
        .collect();
    out.sort();
    out
}

fn ready(diff: &WorktreeDiff) -> &DiffSection {
    match &diff.committed {
        CommittedDiff::Ready { section, .. } => section,
        other => panic!("the committed section is not ready: {other:?}"),
    }
}

// ================================================================================== the repos

const PLANTED: &str = "PLANTED-SECRET-9f3c1e7a";

/// A repo whose worktree `wt-feat` has `n` untracked files of `lines` lines each: a diff that
/// takes a while. Returns the fixture too, which owns the folders.
fn heavy_env(n: usize, lines: usize) -> (Fixture, Env) {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    let body: String = (0..lines)
        .map(|i| format!("line {i} of a file that takes a while to compare\n"))
        .collect();
    std::fs::create_dir_all(wt.join("bulk")).unwrap();
    for i in 0..n {
        std::fs::write(wt.join(format!("bulk/f{i:05}.txt")), &body).unwrap();
    }
    let e = env_with(&fx, wt, true);
    (fx, e)
}

/// The heavy repo and its daemon, with the engine already serving the repo (one diff done).
fn heavy() -> (Fixture, Env, Running) {
    let (fx, e) = heavy_env(3_000, 40);
    let d = Running::start(&e.tp);
    let mut warm = Raw::cli(&e.tp);
    let _ = diff_of(&ask_diff(&mut warm, &e));
    (fx, e, d)
}

fn subscribe_global(raw: &mut Raw) {
    let snap = raw.call(methods::SCOPE_SNAPSHOT, json!({"scope": Scope::Global}));
    assert!(snap.get("error").is_none(), "{snap}");
    let snap: ScopeSnapshot = serde_json::from_value(snap["result"].clone()).unwrap();
    let sub = raw.call(
        methods::SCOPE_SUBSCRIBE,
        json!({"scope": Scope::Global, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
    );
    assert!(sub.get("error").is_none(), "{sub}");
}

/// Reads until a `scope.event` with `seq >= marker` has come in.
fn until_event(raw: &mut Raw, marker: u64) {
    let start = Instant::now();
    loop {
        let seen = raw
            .log
            .iter()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .any(|v| {
                v["method"] == methods::NOTIFY_SCOPE_EVENT
                    && v["params"]["event"]["seq"].as_u64() >= Some(marker)
            });
        if seen {
            return;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "the stream never brought event {marker}"
        );
        raw.pump(Duration::from_millis(100));
    }
}

fn position(raw: &Raw, f: impl Fn(&Value) -> bool) -> Option<usize> {
    raw.log
        .iter()
        .position(|l| f(&serde_json::from_str::<Value>(l).unwrap()))
}

// ================================================================================== async

#[test]
fn reply_comes_out_of_order_and_does_not_block_the_stream() {
    let (_fx, e, d) = heavy();
    let mut raw = Raw::cli(&e.tp);
    subscribe_global(&mut raw);
    let ids = raw.write(&[
        (methods::DIFF_WORKTREE, params(&e)),
        (methods::PING, json!({})),
    ]);
    let marker = d.marker();
    let (diff_id, ping_id) = (ids[0], ids[1]);
    let pong = raw.answer(ping_id);
    assert_eq!(
        pong["result"], "pong",
        "ping must be answered while the diff is in flight: {pong}"
    );
    until_event(&mut raw, marker);
    let diff = raw.answer(diff_id);
    assert!(
        diff.get("error").is_none(),
        "the daemon does not serve diff.worktree: {diff}"
    );
    let at_diff = position(&raw, |v| v["id"] == diff_id).unwrap();
    let at_pong = position(&raw, |v| v["id"] == ping_id).unwrap();
    let at_event = position(&raw, |v| {
        v["method"] == methods::NOTIFY_SCOPE_EVENT && v["params"]["event"]["seq"] == marker
    })
    .expect("the marker event");
    assert!(
        at_pong < at_diff && at_event < at_diff,
        "the diff answered first (pong at {at_pong}, event at {at_event}, diff at {at_diff}): the reader or the stream was blocked"
    );
}

// ================================================================================== cancel

#[test]
fn cancel_answers_now_and_the_query_gets_query_cancelled() {
    let (_fx, e, _d) = heavy();
    let mut raw = Raw::cli(&e.tp);
    let diff_id = raw.next_id;
    let ids = raw.write(&[
        (methods::DIFF_WORKTREE, params(&e)),
        (methods::QUERY_CANCEL, json!({"request": diff_id})),
    ]);
    let cancel = raw.answer(ids[1]);
    assert_eq!(
        cancel["result"],
        json!({"cancelled": true}),
        "query.cancel of a query in flight answers {{cancelled: true}}: {cancel}"
    );
    let diff = raw.answer(ids[0]);
    assert_eq!(
        code_of(&diff),
        Some(QUERY_CANCELLED),
        "the cancelled request answers query-cancelled: {diff}"
    );
    let at = |id: u64| raw.order.iter().position(|i| *i == id).unwrap();
    assert!(
        at(ids[1]) < at(ids[0]),
        "query.cancel did not answer before the query ended"
    );
}

#[test]
fn cancel_of_a_request_not_in_flight_is_false_not_an_error() {
    let (_fx, e, _d) = heavy();
    let mut raw = Raw::cli(&e.tp);
    let done = raw.call(methods::DIFF_WORKTREE, params(&e));
    let _ = diff_of(&done);
    let answer = raw.call(methods::QUERY_CANCEL, json!({"request": 2}));
    assert_eq!(answer["result"], json!({"cancelled": false}), "{answer}");
}

#[test]
fn closing_the_connection_cancels_its_queries() {
    let (_fx, e, _d) = heavy();
    let mut a = Raw::cli(&e.tp);
    // What one whole diff takes on this machine.
    let start = Instant::now();
    let _ = diff_of(&a.call(methods::DIFF_WORKTREE, params(&e)));
    let whole = start.elapsed();
    // A connection asks for one and goes away; the next one asks right after. The pool has one
    // thread: unless the first was cancelled, the second waits for it to end.
    let mut b = Raw::cli(&e.tp);
    b.write(&[(methods::DIFF_WORKTREE, params(&e))]);
    drop(b);
    let mut c = Raw::cli(&e.tp);
    let start = Instant::now();
    let _ = diff_of(&c.call(methods::DIFF_WORKTREE, params(&e)));
    let after = start.elapsed();
    assert!(
        after < whole.mul_f32(1.5),
        "the query of a closed connection kept the pool busy: {after:?} vs {whole:?} for one"
    );
}

#[test]
fn a_second_query_in_flight_is_busy() {
    let (_fx, e, _d) = heavy();
    let mut raw = Raw::cli(&e.tp);
    let ids = raw.write(&[
        (methods::DIFF_WORKTREE, params(&e)),
        (methods::DIFF_WORKTREE, params(&e)),
    ]);
    let second = raw.answer(ids[1]);
    assert_eq!(
        code_of(&second),
        Some(QUERY_BUSY),
        "a second query in flight on the connection gets query-busy: {second}"
    );
    assert_eq!(second["error"]["data"]["limit"], 1, "{second}");
    let first = raw.answer(ids[0]);
    assert!(
        first.get("error").is_none(),
        "the first query is not harmed: {first}"
    );
}

#[test]
fn another_connection_cannot_cancel_it() {
    let (_fx, e, _d) = heavy();
    let mut a = Raw::cli(&e.tp);
    let mut b = Raw::cli(&e.tp);
    let id = a.write(&[(methods::DIFF_WORKTREE, params(&e))])[0];
    let cancel = b.call(methods::QUERY_CANCEL, json!({"request": id}));
    assert_eq!(
        cancel["result"],
        json!({"cancelled": false}),
        "a connection cancelled the query of another: {cancel}"
    );
    let diff = a.answer(id);
    assert!(
        diff.get("error").is_none(),
        "the query of the first connection was cancelled by the second: {diff}"
    );
}

// ================================================================================== mcp

#[test]
fn mcp_connection_does_not_see_diff_worktree() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    std::fs::write(wt.join("secret.txt"), format!("{PLANTED}\n")).unwrap();
    commit_in(&fx, &wt, "secret");
    let e = env_with(&fx, wt, true);
    let _d = Running::start(&e.tp);
    // The CLI gets the diff, with the content ...
    let mut cli = Raw::cli(&e.tp);
    let answer = ask_diff(&mut cli, &e);
    let _ = diff_of(&answer);
    assert!(answer.to_string().contains(PLANTED), "{answer}");
    // ... and `raptor-mcp` neither sees the methods nor gets the content.
    let mut mcp = Client::connect(&e.tp.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    for name in [methods::DIFF_WORKTREE, methods::QUERY_CANCEL] {
        assert!(
            !methods::spec(name).unwrap().mcp,
            "{name} is offered to MCP"
        );
        assert!(!mcp.hello().methods.iter().any(|m| m == name), "{name}");
        let err = mcp
            .call::<_, Value>(
                name,
                json!({"repo_id": e.repo_id, "worktree": e.wt, "request": 1}),
            )
            .unwrap_err();
        match err {
            ClientError::Rpc(o) => {
                assert_eq!(o.code, code::METHOD_NOT_FOUND, "{name}: {o:?}");
                assert!(!format!("{o:?}").contains(PLANTED));
            }
            other => panic!("{name}: expected an RPC error, got {other}"),
        }
    }
}

// ================================================================================== no persist

#[test]
fn diff_responses_are_never_logged_nor_persisted() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    std::fs::write(wt.join(".env"), format!("TOKEN={PLANTED}\n")).unwrap();
    std::fs::write(wt.join("config.txt"), format!("token = {PLANTED}\n")).unwrap();
    commit_in(&fx, &wt, "config");
    std::fs::write(wt.join("config.txt"), format!("token = {PLANTED}-2\n")).unwrap();
    let e = env_with(&fx, wt, true);
    let d = Running::start(&e.tp);
    let mut watcher = Raw::cli(&e.tp);
    subscribe_global(&mut watcher);
    let mut raw = Raw::cli(&e.tp);
    for _ in 0..3 {
        let answer = ask_diff(&mut raw, &e);
        let _ = diff_of(&answer);
        assert!(
            answer.to_string().contains(PLANTED),
            "the response of the diff has no content to protect: {answer}"
        );
    }
    // The stream: nothing of it reaches a subscriber.
    let marker = d.marker();
    until_event(&mut watcher, marker);
    for line in &watcher.log {
        assert!(
            !line.contains(PLANTED),
            "diff content in the stream: {line}"
        );
    }
    // The profile and the logs of the engine, once it has stopped.
    drop(d);
    let mut checked = 0;
    for file in common::files_under(e.tp.root.path()) {
        let bytes = std::fs::read(&file).unwrap_or_default();
        checked += 1;
        assert!(
            !bytes
                .windows(PLANTED.len())
                .any(|w| w == PLANTED.as_bytes()),
            "diff content persisted in {}",
            file.display()
        );
    }
    assert!(checked > 0);
}

// ================================================================================== limits

#[test]
fn limits_cut_files_keeps_the_total_and_never_fails() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    std::fs::create_dir_all(wt.join("many")).unwrap();
    for i in 0..1_200 {
        std::fs::write(wt.join(format!("many/f{i:04}.txt")), format!("{i}\n")).unwrap();
    }
    commit_in(&fx, &wt, "many files");
    let e = env_with(&fx, wt, true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let diff = diff_of(&ask_diff(&mut raw, &e));
    let section = ready(&diff);
    assert_eq!(
        section.files_total, 1_200,
        "files_total counts every changed file"
    );
    assert_eq!(
        section.cut,
        Some(DiffCut::Files),
        "more than 1.000 files must cut the section with `files`"
    );
    assert!(section.files.len() <= 1_200);
}

#[test]
fn limits_cut_bytes_keeps_the_reply_under_768_kib() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    std::fs::create_dir_all(wt.join("fat")).unwrap();
    let body: String = (0..100).map(|i| format!("{i:03} {:->96}\n", "")).collect();
    for i in 0..150 {
        std::fs::write(wt.join(format!("fat/f{i:03}.txt")), &body).unwrap();
    }
    commit_in(&fx, &wt, "fat files");
    let e = env_with(&fx, wt, true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let answer = ask_diff(&mut raw, &e);
    let diff = diff_of(&answer);
    let section = ready(&diff);
    assert_eq!(section.files_total, 150);
    assert_eq!(
        section.cut,
        Some(DiffCut::Bytes),
        "150 files of about 10 KiB are over the 768 KiB of a reply"
    );
    assert!(
        answer.to_string().len() <= 768 * 1024,
        "the reply is over 768 KiB: {} bytes",
        answer.to_string().len()
    );
    assert!(
        section
            .files
            .iter()
            .any(|f| matches!(f.body, FileBody::Omitted {})),
        "the files that did not fit come out as omitted"
    );
}
