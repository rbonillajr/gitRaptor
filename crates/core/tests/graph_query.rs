//! US-CKP-022 D2 to D5, D12, D19 and SEC-12: `graph.lanes` asked of a real daemon (in-process)
//! over a raw connection, with real Git repos. Base rules, which worktrees are lanes, the shape
//! of a lane that cannot be computed, cancellation, one query per connection, MCP closed to the
//! method, nothing logged or persisted, and the limits. A temporary profile and temporary repos
//! (NFR-01). No fixed waits: each test waits on the answer or the state that proves the fact.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#![allow(dead_code)]

mod common;

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::graph::{
    BaseGap, GraphBase, GraphCut, GraphLanes, GraphParams, Lane, LaneGap, LaneHead, LaneState,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_core::channel::{ChannelConfig, EventBus};
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

const DEADLINE: Duration = Duration::from_secs(60);
/// `query-busy` and `query-cancelled` (DS-US-CKP-012 § Contrato de API).
const QUERY_BUSY: i64 = -33120;
const QUERY_CANCELLED: i64 = -33121;
const PLANTED: &str = "PLANTED-SECRET-9f3c1e7a";

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
        Self::start_with(tp, None)
    }

    /// `deadline`: the deadline of on-demand queries of this daemon (debug builds only, D18).
    fn start_with(tp: &TempProfile, deadline: Option<Duration>) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let config = DaemonConfig {
            dirs: tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: ChannelConfig {
                test_diff_deadline: deadline,
                ..Default::default()
            },
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

// ------------------------------------------------------------------------------ the repos

struct Env {
    tp: TempProfile,
    repo_id: String,
}

/// Registers the repo of `fx` in a temporary profile; `confirm` is the confirmed base branch.
/// Nothing starts a daemon.
fn env_with(fx: &Fixture, confirm: Option<&str>) -> Env {
    let tp = TempProfile::new();
    let repo_id = {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        if let Some(name) = confirm {
            let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
            store
                .set_confirmed_team_baseline(&Confirmed {
                    base_branch: RefName::new(name).unwrap(),
                    floor: ConfirmedFloor::Absent,
                })
                .unwrap();
        }
        entry.repo_id
    };
    Env { tp, repo_id }
}

/// `n` commits (each with `msg(i)` as its whole message, newest last) on a new branch `branch`
/// that starts at `main`, written with `git fast-import` so thousands cost no process each.
fn import_branch(fx: &Fixture, branch: &str, n: usize, t0: i64, msg: &dyn Fn(usize) -> Vec<u8>) {
    let mut stream: Vec<u8> = Vec::new();
    for i in 0..n {
        let body = msg(i);
        stream.extend_from_slice(
            format!(
                "commit refs/heads/{branch}\ncommitter T <t@example.com> {} +0000\ndata {}\n",
                t0 + i as i64,
                body.len()
            )
            .as_bytes(),
        );
        stream.extend_from_slice(&body);
        stream.push(b'\n');
        if i == 0 {
            stream.extend_from_slice(b"from refs/heads/main\n");
        }
        stream.push(b'\n');
    }
    let mut child = fx
        .git_command(&fx.repo, &["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&stream).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "fast-import failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn plain(prefix: &'static str) -> impl Fn(usize) -> Vec<u8> {
    move |i| format!("{prefix} {i}").into_bytes()
}

fn commit_in(fx: &Fixture, wt: &Path, msg: &str) {
    fx.git_in(wt, &["commit", "-q", "--allow-empty", "-m", msg]);
}

/// `main` moved on, `feat` (n_feat commits) and `other` (n_other) each in their worktree.
/// Returns the paths of both.
fn shop(n_feat: usize, n_other: usize) -> (Fixture, PathBuf, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    fx.git(&["branch", "other"]);
    fx.git(&["commit", "-q", "--allow-empty", "-m", "main moves on"]);
    let feat = fx.add_worktree("feat", "feat");
    let other = fx.add_worktree("other", "other");
    for i in 0..n_feat {
        commit_in(&fx, &feat, &format!("feat {i}"));
    }
    for i in 0..n_other {
        commit_in(&fx, &other, &format!("other {i}"));
    }
    (fx, canonical(&feat), canonical(&other))
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

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.write(&[(method, params)])[0];
        self.answer(id)
    }
}

fn code_of(answer: &Value) -> Option<i64> {
    answer["error"]["code"].as_i64()
}

fn params(e: &Env) -> Value {
    serde_json::to_value(GraphParams {
        repo_id: e.repo_id.clone(),
    })
    .unwrap()
}

/// The typed result of `graph.lanes`, or a failure naming what the daemon answered.
fn lanes_of(answer: &Value) -> GraphLanes {
    assert!(
        answer.get("error").is_none(),
        "the daemon does not serve graph.lanes: {answer}"
    );
    serde_json::from_value(answer["result"].clone())
        .unwrap_or_else(|e| panic!("graph.lanes result is not a GraphLanes ({e}): {answer}"))
}

fn ask(raw: &mut Raw, e: &Env) -> GraphLanes {
    lanes_of(&raw.call(methods::GRAPH_LANES, params(e)))
}

/// The repo as `engine.snapshot` publishes it, once it is observed with `worktrees` worktrees
/// and, with `counted`, every linked worktree has its ahead/behind counted. Returns that view.
fn wait_repo(raw: &mut Raw, e: &Env, worktrees: usize, counted: bool) -> Value {
    let start = Instant::now();
    loop {
        let snap = raw.call(methods::ENGINE_SNAPSHOT, json!({}));
        let view = snap["result"]["repos"]
            .as_array()
            .and_then(|r| r.iter().find(|r| r["repo_id"] == e.repo_id.as_str()))
            .cloned();
        if let Some(view) = view {
            let list = view["worktrees"].as_array().cloned().unwrap_or_default();
            let ok = view["state"] == "observed"
                && list.len() == worktrees
                && (!counted
                    || list.iter().filter(|w| w["main"] == false).all(|w| {
                        w["status"]["state"] == "ready"
                            && w["status"]["divergence"]["state"] == "counted"
                    }));
            if ok {
                return view;
            }
        }
        assert!(
            start.elapsed() < DEADLINE,
            "the engine never published the repo with {worktrees} worktrees: {snap}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn lane_at<'a>(lanes: &'a GraphLanes, path: &Path) -> &'a Lane {
    lanes
        .lanes
        .iter()
        .find(|l| Path::new(l.worktree.raw()) == path)
        .unwrap_or_else(|| panic!("no lane for {}", path.display()))
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

fn total_of(lane: &Lane) -> (u64, bool) {
    match &lane.state {
        LaneState::Ready { total, .. } => (total.count, total.exact),
        LaneState::NotComputable { reason } => panic!("the lane is not computable: {reason:?}"),
    }
}

/// A repo whose branch `feat` has `n` commits: a lane that takes a while to walk.
fn heavy(n: usize) -> (Fixture, Env, PathBuf) {
    let fx = Fixture::with_commit(&git());
    import_branch(&fx, "feat", n, 1_800_000_000, &plain("heavy"));
    let wt = canonical(&fx.add_worktree("feat", "feat"));
    let e = env_with(&fx, Some("main"));
    (fx, e, wt)
}

// ================================================================================== totals

#[test]
fn lane_totals_match_the_ahead_of_the_stream() {
    let (fx, feat, other) = shop(7, 3);
    let e = env_with(&fx, Some("main"));
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let view = wait_repo(&mut raw, &e, 3, true);

    let lanes = ask(&mut raw, &e);
    assert!(
        matches!(&lanes.base, GraphBase::Ready { name } if name.raw() == "main"),
        "the base is the confirmed main"
    );
    assert_eq!(lanes.lanes.len(), 2, "one lane per linked worktree");
    assert_eq!(
        lanes.lanes_total, 2,
        "lanes_total is the number of lanes, not of commits"
    );
    for (path, expected) in [(&feat, 7), (&other, 3)] {
        let ahead = view["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| Path::new(w["path"]["untrusted"].as_str().unwrap()) == path.as_path())
            .map(|w| w["status"]["divergence"]["ahead"].clone())
            .unwrap();
        let (count, exact) = total_of(lane_at(&lanes, path));
        assert_eq!(
            (count, exact),
            (
                ahead["count"].as_u64().unwrap(),
                ahead["exact"].as_bool().unwrap()
            ),
            "the total of the lane of {} is not the ↑ of the stream",
            path.display()
        );
        assert_eq!(count, expected, "the lane counts the commits of its branch");
    }
}

// ================================================================================== the base

#[test]
fn unconfirmed_base_has_no_lanes() {
    let (fx, _, _) = shop(4, 2);
    let e = env_with(&fx, None);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 3, false);
    let lanes = ask(&mut raw, &e);
    assert!(
        matches!(
            &lanes.base,
            GraphBase::NotComputable { reason: BaseGap::BaseUnconfirmed { name } } if name.raw() == "main"
        ),
        "an unconfirmed base is not computable and names the resolved branch (D5)"
    );
    assert!(
        lanes.lanes.is_empty() && lanes.lanes_total == 0,
        "no lanes are drawn against an unconfirmed base"
    );
}

#[test]
fn missing_base_is_not_computable_without_lanes() {
    let (fx, _, _) = shop(4, 2);
    // `main` exists and has everything: only the confirmed `develop` is asked for.
    let e = env_with(&fx, Some("develop"));
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 3, false);
    let lanes = ask(&mut raw, &e);
    assert!(
        matches!(
            &lanes.base,
            GraphBase::NotComputable { reason: BaseGap::BaseMissing { name } } if name.raw() == "develop"
        ),
        "a base without a ref is base-missing with its name (BR-CKP-EDGE-001)"
    );
    assert!(
        lanes.lanes.is_empty() && lanes.lanes_total == 0,
        "another branch (main) must not stand in for the missing base"
    );
}

// ================================================================================== the lanes

#[test]
fn the_base_worktree_is_not_a_lane() {
    let (fx, feat, other) = shop(2, 2);
    let e = env_with(&fx, Some("main"));
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 3, false);
    let lanes = ask(&mut raw, &e);
    let mut got: Vec<&str> = lanes.lanes.iter().map(|l| l.worktree.raw()).collect();
    got.sort();
    let mut want = vec![feat.to_str().unwrap(), other.to_str().unwrap()];
    want.sort();
    assert_eq!(
        got, want,
        "the worktree with `main` checked out is the trunk"
    );
    assert!(
        lanes
            .lanes
            .iter()
            .all(|l| !matches!(&l.head, LaneHead::Branch { name } if name.raw() == "main")),
        "no lane for the base branch (D3)"
    );
    assert_eq!(lanes.lanes_total, 2);
}

#[test]
fn unavailable_and_unborn_worktrees_are_lanes_with_a_reason() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    fx.git(&["branch", "ghost"]);
    let feat = canonical(&fx.add_worktree("feat", "feat"));
    commit_in(&fx, &feat, "feat one");
    commit_in(&fx, &feat, "feat two");
    let ghost = canonical(&fx.add_worktree("ghost", "ghost"));
    let orphan = fx.root.join("wt-unborn");
    fx.git(&[
        "worktree",
        "add",
        "-q",
        "--orphan",
        "-b",
        "unborn",
        orphan.to_str().unwrap(),
    ]);
    let orphan = canonical(&orphan);
    let detached = fx.root.join("wt-detached");
    fx.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        detached.to_str().unwrap(),
        "feat",
    ]);
    let detached = canonical(&detached);
    // The folder of one worktree goes away: Git still lists it.
    std::fs::remove_dir_all(&ghost).unwrap();

    let e = env_with(&fx, Some("main"));
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 5, false);
    let lanes = ask(&mut raw, &e);
    assert_eq!(
        lanes.lanes.len(),
        4,
        "every worktree but the trunk is a lane"
    );

    assert!(
        matches!(
            &lane_at(&lanes, &ghost).state,
            LaneState::NotComputable {
                reason: LaneGap::WorktreeUnavailable
            }
        ),
        "a worktree whose folder is gone is a lane that says worktree-unavailable"
    );
    let unborn = lane_at(&lanes, &orphan);
    assert!(
        matches!(&unborn.head, LaneHead::Branch { name } if name.raw() == "unborn"),
        "an unborn branch keeps its name"
    );
    assert!(
        matches!(
            &unborn.state,
            LaneState::NotComputable {
                reason: LaneGap::NoCommits
            }
        ),
        "an unborn branch is a lane that says no-commits"
    );
    let det = lane_at(&lanes, &detached);
    assert!(
        matches!(det.head, LaneHead::Detached),
        "a detached HEAD is a lane without a branch name"
    );
    assert_eq!(total_of(det).0, 2, "its commits are still read");
    assert_eq!(total_of(lane_at(&lanes, &feat)).0, 2);
}

// ================================================================================== async

#[test]
fn cancel_answers_query_cancelled() {
    let (_fx, e, _wt) = heavy(9_000);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 2, true);
    let graph_id = raw.next_id;
    let ids = raw.write(&[
        (methods::GRAPH_LANES, params(&e)),
        (methods::QUERY_CANCEL, json!({"request": graph_id})),
    ]);
    let cancel = raw.answer(ids[1]);
    assert_eq!(
        cancel["result"],
        json!({"cancelled": true}),
        "query.cancel of a graph.lanes in flight answers {{cancelled: true}}: {cancel}"
    );
    let graph = raw.answer(ids[0]);
    assert_eq!(
        code_of(&graph),
        Some(QUERY_CANCELLED),
        "the cancelled graph.lanes answers query-cancelled: {graph}"
    );
}

#[test]
fn a_second_request_on_the_connection_is_query_busy() {
    let (_fx, e, _wt) = heavy(9_000);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 2, true);
    let ids = raw.write(&[
        (methods::GRAPH_LANES, params(&e)),
        (methods::GRAPH_LANES, params(&e)),
    ]);
    let second = raw.answer(ids[1]);
    assert_eq!(
        code_of(&second),
        Some(QUERY_BUSY),
        "a second graph.lanes in flight on the connection gets query-busy: {second}"
    );
    assert_eq!(second["error"]["data"]["limit"], 1, "{second}");
    let first = raw.answer(ids[0]);
    assert!(
        first.get("error").is_none(),
        "the first graph.lanes is not harmed: {first}"
    );
}

// ================================================================================== the repo id

#[test]
fn unknown_repo_is_not_found() {
    let (fx, _, _) = shop(1, 1);
    let e = env_with(&fx, Some("main"));
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    // The engine is observing: the repo is known and published.
    wait_repo(&mut raw, &e, 3, false);
    let unknown = raw.call(methods::GRAPH_LANES, json!({"repo_id": "ffffffff"}));
    assert_eq!(
        code_of(&unknown),
        Some(code::NOT_FOUND),
        "an unknown, well-formed repo_id with the engine observing is NOT_FOUND: {unknown}"
    );
    let malformed = raw.call(methods::GRAPH_LANES, json!({"repo_id": "../not a repo id"}));
    assert_eq!(
        code_of(&malformed),
        Some(code::INVALID_PARAMS),
        "a malformed repo_id is INVALID_PARAMS: {malformed}"
    );
}

#[test]
fn a_repo_not_yet_observed_is_repo_unavailable() {
    let (fx, _, _) = shop(1, 1);
    let e = env_with(&fx, Some("main"));
    // Known to the profile but its Git directory cannot be opened: the engine publishes the repo
    // as unavailable.
    std::fs::remove_dir_all(fx.repo.join(".git")).unwrap();
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let start = Instant::now();
    loop {
        let snap = raw.call(methods::ENGINE_SNAPSHOT, json!({}));
        let unavailable = snap["result"]["repos"].as_array().is_some_and(|r| {
            r.iter()
                .any(|r| r["repo_id"] == e.repo_id.as_str() && r["state"] == "unavailable")
        });
        if unavailable {
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "the engine never published the repo as unavailable: {snap}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let answer = raw.call(methods::GRAPH_LANES, params(&e));
    let lanes = lanes_of(&answer);
    assert!(
        matches!(
            lanes.base,
            GraphBase::NotComputable {
                reason: BaseGap::RepoUnavailable
            }
        ),
        "a known repo that is not observed answers repo-unavailable in the result, never an error: {answer}"
    );
    assert!(lanes.lanes.is_empty() && lanes.lanes_total == 0);
}

// ================================================================================== mcp

#[test]
fn mcp_connection_does_not_see_graph_lanes() {
    let (fx, _, _) = shop(2, 1);
    let e = env_with(&fx, Some("main"));
    let _d = Running::start(&e.tp);
    // The CLI gets the lanes ...
    let mut cli = Raw::cli(&e.tp);
    wait_repo(&mut cli, &e, 3, false);
    let _ = ask(&mut cli, &e);
    // ... and `raptor-mcp` neither sees the method nor gets an answer.
    let mut mcp = Client::connect(&e.tp.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    assert!(
        !methods::spec(methods::GRAPH_LANES).unwrap().mcp,
        "graph.lanes is offered to MCP"
    );
    assert!(
        !mcp.hello()
            .methods
            .iter()
            .any(|m| m == methods::GRAPH_LANES),
        "the MCP handshake lists graph.lanes"
    );
    let err = mcp
        .call::<_, Value>(methods::GRAPH_LANES, json!({"repo_id": e.repo_id}))
        .unwrap_err();
    match err {
        ClientError::Rpc(o) => assert_eq!(o.code, code::METHOD_NOT_FOUND, "{o:?}"),
        other => panic!("expected an RPC error, got {other}"),
    }
}

// ================================================================================== no persist

#[test]
fn responses_are_never_logged_nor_persisted() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let feat = fx.add_worktree("feat", "feat");
    commit_in(&fx, &feat, &format!("rotate the token {PLANTED}"));
    commit_in(&fx, &feat, &format!("use token {PLANTED}-2"));
    let e = env_with(&fx, Some("main"));
    let d = Running::start(&e.tp);
    let mut watcher = Raw::cli(&e.tp);
    subscribe_global(&mut watcher);
    let mut raw = Raw::cli(&e.tp);
    wait_repo(&mut raw, &e, 2, false);
    for _ in 0..3 {
        let answer = raw.call(methods::GRAPH_LANES, params(&e));
        let _ = lanes_of(&answer);
        assert!(
            answer.to_string().contains(PLANTED),
            "the response has no subjects to protect: {answer}"
        );
    }
    // The stream: nothing of it reaches a subscriber.
    let marker = d.marker();
    until_event(&mut watcher, marker);
    for line in &watcher.log {
        assert!(
            !line.contains(PLANTED),
            "a commit subject in the stream: {line}"
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
            "a commit subject persisted in {}",
            file.display()
        );
    }
    assert!(checked > 0);
}

// ================================================================================== limits

#[test]
fn limits_cut_and_never_fail() {
    // 1. More than 32 lanes: the first 32, `cut: lanes` and the real total.
    {
        let fx = Fixture::with_commit(&git());
        for i in 0..40 {
            let path = fx.root.join(format!("wt-b{i:02}"));
            fx.git(&[
                "worktree",
                "add",
                "-q",
                "-b",
                &format!("b{i:02}"),
                path.to_str().unwrap(),
                "main",
            ]);
        }
        let e = env_with(&fx, Some("main"));
        let _d = Running::start(&e.tp);
        let mut raw = Raw::cli(&e.tp);
        wait_repo(&mut raw, &e, 41, false);
        let lanes = ask(&mut raw, &e);
        assert_eq!(lanes.lanes.len(), 32, "a response carries at most 32 lanes");
        assert_eq!(lanes.lanes_total, 40, "lanes_total is the real number");
        assert!(
            matches!(lanes.cut, Some(GraphCut::Lanes)),
            "past 32 lanes the response is cut with `lanes`"
        );
    }

    // 2. A response over 768 KiB: commits go from the end of the longest lanes, `cut: bytes`.
    // 12 lanes of 50 commits whose subjects are 256 control bytes, which JSON escapes to six
    // bytes each: about 1 MiB without the cut.
    {
        let fx = Fixture::with_commit(&git());
        for i in 0..12 {
            import_branch(&fx, &format!("fat{i:02}"), 50, 1_800_000_000, &|n| {
                let mut m = vec![1u8; 256];
                m.extend_from_slice(format!(" {i} {n}").as_bytes());
                m
            });
            fx.add_worktree(&format!("fat{i:02}"), &format!("fat{i:02}"));
        }
        let e = env_with(&fx, Some("main"));
        let _d = Running::start(&e.tp);
        let mut raw = Raw::cli(&e.tp);
        wait_repo(&mut raw, &e, 13, false);
        let answer = raw.call(methods::GRAPH_LANES, params(&e));
        let lanes = lanes_of(&answer);
        assert!(
            matches!(lanes.cut, Some(GraphCut::Bytes)),
            "a response over 768 KiB is cut with `bytes`: {} bytes",
            answer.to_string().len()
        );
        assert!(
            answer.to_string().len() <= 768 * 1024,
            "the response is over 768 KiB: {} bytes",
            answer.to_string().len()
        );
        assert_eq!(
            lanes.lanes.len(),
            12,
            "the lanes stay, their windows shrink"
        );
        for lane in &lanes.lanes {
            assert_eq!(
                total_of(lane).0,
                50,
                "the total keeps counting what the window no longer shows"
            );
        }
    }

    // 3. A deadline that passes: the lanes that did not finish are `too-slow`, not an error.
    {
        let fx = Fixture::with_commit(&git());
        import_branch(&fx, "a-heavy", 3_000, 1_800_000_000, &plain("heavy"));
        fx.git(&["branch", "b-light"]);
        let heavy_wt = canonical(&fx.add_worktree("a-heavy", "a-heavy"));
        let light_wt = canonical(&fx.add_worktree("b-light", "b-light"));
        let e = env_with(&fx, Some("main"));
        let _d = Running::start_with(&e.tp, Some(Duration::from_millis(1)));
        let mut raw = Raw::cli(&e.tp);
        wait_repo(&mut raw, &e, 3, false);
        let answer = raw.call(methods::GRAPH_LANES, params(&e));
        let lanes = lanes_of(&answer);
        for path in [&heavy_wt, &light_wt] {
            assert!(
                matches!(
                    &lane_at(&lanes, path).state,
                    LaneState::NotComputable {
                        reason: LaneGap::TooSlow
                    }
                ),
                "with a 1 ms deadline the lane of {} is too-slow, never a partial window",
                path.display()
            );
        }
    }
}
