//! TS-CKP-006 (CKP006-001, -002, -005 to -008, -010, -011): `diff.commit`, `diff.between` and
//! `diff.index` asked of a real daemon (in-process) over a raw connection: the contract, the oid
//! validation, reachability, limits, cancellation and queue, no execution of repo programs, no
//! secrets outside the answer and the log lines. A temporary profile and temporary repos (NFR-01).
//! No fixed waits: each test waits on the answer or the event that proves the fact.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#![allow(dead_code)]

mod common;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::diff::index::CONFLICTS_RESERVE_BYTES;
use gitraptor_api::event::RESERVED_AUDIT;
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
use gitraptor_testkit::canary::Canary;
use serde_json::{Value, json};

// ------------------------------------------------------------------------------ the daemon

const DEADLINE: Duration = Duration::from_secs(30);
const QUERY_BUSY: i64 = -33120;
const QUERY_CANCELLED: i64 = -33121;
const DIFF_UNAVAILABLE: i64 = -33140;
const PLANTED: &str = "PLANTED-SECRET-9f3c1e7a";
const NEW_METHODS: [&str; 3] = [
    methods::DIFF_COMMIT,
    methods::DIFF_BETWEEN,
    methods::DIFF_INDEX,
];

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

    /// `deadline`: the deadline of the diff queries of this daemon (debug builds only).
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

fn env_with(fx: &Fixture, wt: PathBuf) -> Env {
    let wt = canonical(&wt);
    let tp = TempProfile::new();
    let repo_id = {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        store
            .set_confirmed_team_baseline(&Confirmed {
                base_branch: RefName::new("main").unwrap(),
                floor: ConfirmedFloor::Absent,
            })
            .unwrap();
        entry.repo_id
    };
    Env { tp, wt, repo_id }
}

fn commit_in(fx: &Fixture, wt: &Path, msg: &str) -> String {
    fx.git_in(wt, &["add", "-A"]);
    fx.git_in(wt, &["commit", "-q", "-m", msg]);
    head(fx, wt)
}

fn head(fx: &Fixture, wt: &Path) -> String {
    fx.git_in(wt, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// A repo with branch `feat` checked out in worktree `wt-feat`; `build` adds commits there.
fn repo(build: impl FnOnce(&Fixture, &Path)) -> (Fixture, Env) {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    build(&fx, &wt);
    let e = env_with(&fx, wt);
    (fx, e)
}

/// A repo with a `feat` branch of 1.200 commits that shares no history with `main`; returns the
/// oid of its root commit, which a walk reaches only after 1.200 steps.
fn long_history() -> (Fixture, Env, String) {
    let fx = Fixture::with_commit(&git());
    let mut stream = String::new();
    for i in 0..1_200 {
        stream.push_str(
            "commit refs/heads/feat\ncommitter T <t@example.com> 1700000000 +0000\ndata 2\nc\n",
        );
        if i == 0 {
            stream.push_str("M 644 inline f.txt\ndata 2\nf\n");
        }
        stream.push('\n');
    }
    let mut child = fx
        .git_command(&fx.repo, &["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success(), "fast-import failed");
    let root = fx
        .git(&["rev-list", "--max-parents=0", "feat"])
        .trim()
        .to_owned();
    let wt = fx.add_worktree("feat", "feat");
    let e = env_with(&fx, wt);
    (fx, e, root)
}

// ------------------------------------------------------------------------------ the wire

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

fn index_params(e: &Env) -> Value {
    json!({"repo_id": e.repo_id, "worktree": e.wt})
}

fn commit_params(e: &Env, commit: &str) -> Value {
    json!({"repo_id": e.repo_id, "commit": commit})
}

fn between_params(e: &Env, old: &str, new: &str) -> Value {
    json!({"repo_id": e.repo_id, "old": old, "new": new})
}

/// Asks until the engine has the repo ready (it answers `diff-unavailable` before). Any other
/// answer, an error included, is returned as it is.
fn ask(raw: &mut Raw, method: &str, params: &Value) -> Value {
    let start = Instant::now();
    loop {
        let answer = raw.call(method, params.clone());
        if code_of(&answer) != Some(DIFF_UNAVAILABLE) || start.elapsed() > DEADLINE {
            return answer;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ok(answer: &Value, what: &str) -> Value {
    assert!(
        answer.get("error").is_none(),
        "the daemon does not serve {what}: {answer}"
    );
    answer["result"].clone()
}

fn files_of(section: &Value) -> Vec<String> {
    section["files"]
        .as_array()
        .unwrap_or_else(|| panic!("no files in {section}"))
        .iter()
        .map(|f| f["path"]["untrusted"].as_str().unwrap().to_owned())
        .collect()
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

/// A repo whose `feat` commit holds 3.000 files of 40 lines: a commit diff that takes a while.
fn heavy() -> (Fixture, Env, Running, String) {
    let (fx, e) = repo(|fx, wt| {
        let body: String = (0..40)
            .map(|i| format!("line {i} of a file that takes a while to compare\n"))
            .collect();
        std::fs::create_dir_all(wt.join("bulk")).unwrap();
        for i in 0..3_000 {
            std::fs::write(wt.join(format!("bulk/f{i:05}.txt")), &body).unwrap();
        }
        commit_in(fx, wt, "bulk");
    });
    let oid = head(&fx, &e.wt);
    let d = Running::start(&e.tp);
    let mut warm = Raw::cli(&e.tp);
    let _ = ok(
        &ask(&mut warm, methods::DIFF_COMMIT, &commit_params(&e, &oid)),
        "diff.commit",
    );
    (fx, e, d, oid)
}

// ================================================================================== CKP006-001

#[test]
fn ckp006_001_methods_are_offered_to_tui_and_app_and_not_to_mcp() {
    let (_fx, e) = repo(|_, _| {});
    let _d = Running::start(&e.tp);
    let cli = Client::connect(&e.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let app = Client::connect(&e.tp.dirs(), ClientKind::Other, PROTOCOL_VERSION).unwrap();
    let mut mcp = Client::connect(&e.tp.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let oid = "1".repeat(40);
    for name in NEW_METHODS {
        assert!(
            !methods::spec(name)
                .unwrap_or_else(|| panic!("{name} is not declared"))
                .mcp,
            "{name} is offered to MCP"
        );
        assert!(
            cli.hello().methods.iter().any(|m| m == name),
            "{name} not in the hello of the TUI"
        );
        assert!(
            app.hello().methods.iter().any(|m| m == name),
            "{name} not in the hello of the app"
        );
        assert!(
            !mcp.hello().methods.iter().any(|m| m == name),
            "{name} in the hello of mcp"
        );
        let err = mcp
            .call::<_, Value>(
                name,
                json!({"repo_id": e.repo_id, "worktree": e.wt, "commit": oid, "old": oid, "new": oid}),
            )
            .unwrap_err();
        match err {
            ClientError::Rpc(o) => assert_eq!(o.code, code::METHOD_NOT_FOUND, "{name}: {o:?}"),
            other => panic!("{name}: expected an RPC error, got {other}"),
        }
    }
}

#[test]
fn ckp006_001_an_extra_field_is_invalid_params() {
    let (_fx, e) = repo(|_, _| {});
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let oid = "1".repeat(40);
    let cases = [
        (
            methods::DIFF_COMMIT,
            json!({"repo_id": e.repo_id, "commit": oid, "extra": 1}),
        ),
        (
            methods::DIFF_BETWEEN,
            json!({"repo_id": e.repo_id, "old": oid, "new": oid, "extra": 1}),
        ),
        (
            methods::DIFF_INDEX,
            json!({"repo_id": e.repo_id, "worktree": e.wt, "extra": 1}),
        ),
    ];
    for (method, params) in cases {
        let answer = raw.call(method, params);
        assert_eq!(
            code_of(&answer),
            Some(code::INVALID_PARAMS),
            "{method} must reject an unknown field: {answer}"
        );
    }
}

// ================================================================================== CKP006-002

#[test]
fn ckp006_002_malformed_oids_are_invalid_params() {
    let (_fx, e) = repo(|_, _| {});
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let valid = "1".repeat(40);
    let bad = [
        "abc1234".to_owned(),
        "A".repeat(40),
        "a".repeat(39),
        "a".repeat(41),
        "a".repeat(63),
        "a".repeat(65),
        format!("-abc{}", "0".repeat(36)),
        "HEAD".to_owned(),
        "HEAD~".to_owned(),
        "@{-1}".to_owned(),
        ":/regex".to_owned(),
        String::new(),
    ];
    for oid in &bad {
        let a = raw.call(methods::DIFF_COMMIT, commit_params(&e, oid));
        assert_eq!(
            code_of(&a),
            Some(code::INVALID_PARAMS),
            "diff.commit with {oid:?}: {a}"
        );
        let a = raw.call(methods::DIFF_BETWEEN, between_params(&e, oid, &valid));
        assert_eq!(
            code_of(&a),
            Some(code::INVALID_PARAMS),
            "diff.between old {oid:?}: {a}"
        );
        let a = raw.call(methods::DIFF_BETWEEN, between_params(&e, &valid, oid));
        assert_eq!(
            code_of(&a),
            Some(code::INVALID_PARAMS),
            "diff.between new {oid:?}: {a}"
        );
    }
    // Control: a well-formed oid passes the validation and is simply not found.
    let a = ask(&mut raw, methods::DIFF_COMMIT, &commit_params(&e, &valid));
    assert_eq!(
        code_of(&a),
        Some(code::NOT_FOUND),
        "a valid unknown oid is not-found: {a}"
    );
}

// ================================================================================== CKP006-005

#[test]
fn ckp006_005_everything_unreachable_answers_the_same_not_found() {
    let (fx, e) = repo(|fx, wt| {
        fx.git_in(wt, &["commit", "-q", "--allow-empty", "-m", "second"]);
    });
    let reachable = head(&fx, &e.wt);
    std::fs::write(fx.repo.join("loose.txt"), "loose\n").unwrap();
    let blob = fx
        .git(&["hash-object", "-w", "loose.txt"])
        .trim()
        .to_owned();
    let tree = fx
        .git_in(&e.wt, &["rev-parse", "HEAD^{tree}"])
        .trim()
        .to_owned();
    let orphan = fx
        .git(&["commit-tree", &tree, "-m", "orphan"])
        .trim()
        .to_owned();
    fx.git_in(
        &fx.other_repo,
        &["commit", "-q", "--allow-empty", "-m", "elsewhere"],
    );
    let foreign = fx
        .git_in(&fx.other_repo, &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    let ghost = "1".repeat(40);
    let sha256 = "a".repeat(64);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    // Control: the reachable commit is served.
    let _ = ok(
        &ask(
            &mut raw,
            methods::DIFF_COMMIT,
            &commit_params(&e, &reachable),
        ),
        "diff.commit",
    );
    let baseline = ask(&mut raw, methods::DIFF_COMMIT, &commit_params(&e, &ghost));
    assert_eq!(code_of(&baseline), Some(code::NOT_FOUND), "{baseline}");
    for (name, oid) in [
        ("loose blob", &blob),
        ("orphan commit", &orphan),
        ("commit of another repo", &foreign),
        ("oid of another hash type", &sha256),
    ] {
        let a = raw.call(methods::DIFF_COMMIT, commit_params(&e, oid));
        assert_eq!(
            a["error"], baseline["error"],
            "{name} must answer the same not-found as a nonexistent oid"
        );
        let a = raw.call(methods::DIFF_BETWEEN, between_params(&e, &reachable, oid));
        assert_eq!(
            a["error"], baseline["error"],
            "diff.between with {name} must answer the same not-found"
        );
    }
}

#[test]
fn ckp006_005_a_walk_cut_by_the_deadline_answers_the_same_not_found() {
    let (_fx, e, root) = long_history();
    let _d = Running::start_with(&e.tp, Some(Duration::from_millis(1)));
    let mut raw = Raw::cli(&e.tp);
    let ghost = ask(
        &mut raw,
        methods::DIFF_COMMIT,
        &commit_params(&e, &"1".repeat(40)),
    );
    assert_eq!(code_of(&ghost), Some(code::NOT_FOUND), "{ghost}");
    let cut = raw.call(methods::DIFF_COMMIT, commit_params(&e, &root));
    assert_eq!(
        cut["error"], ghost["error"],
        "a walk cut by the deadline must answer the same not-found as a nonexistent oid"
    );
}

// ================================================================================== CKP006-006

#[test]
fn ckp006_006_a_40_mb_file_is_too_large_and_a_png_is_binary() {
    let (fx, e) = repo(|fx, wt| {
        std::fs::write(wt.join("big.txt"), vec![b'a'; 40 * 1024 * 1024]).unwrap();
        std::fs::write(wt.join("img.png"), [0x89, b'P', b'N', b'G', 0, 1, 2, 3, 0]).unwrap();
        commit_in(fx, wt, "big and binary");
    });
    let oid = head(&fx, &e.wt);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let result = ok(
        &ask(&mut raw, methods::DIFF_COMMIT, &commit_params(&e, &oid)),
        "diff.commit",
    );
    let file = |name: &str| {
        result["section"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"]["untrusted"] == name)
            .unwrap_or_else(|| panic!("no entry for {name}: {result}"))
            .clone()
    };
    assert!(
        file("big.txt")["body"].get("too-large").is_some(),
        "a 40 MB file must be too-large: {}",
        file("big.txt")
    );
    assert!(
        file("img.png")["body"].get("binary").is_some(),
        "a png must be binary: {}",
        file("img.png")
    );
}

#[test]
fn ckp006_006_more_than_1000_files_cut_files_and_keep_the_total() {
    let (fx, e) = repo(|fx, wt| {
        std::fs::create_dir_all(wt.join("many")).unwrap();
        for i in 0..1_200 {
            std::fs::write(wt.join(format!("many/f{i:04}.txt")), format!("{i}\n")).unwrap();
        }
        commit_in(fx, wt, "many files");
    });
    let oid = head(&fx, &e.wt);
    let _d = Running::start_with(&e.tp, Some(Duration::from_secs(60)));
    let mut raw = Raw::cli(&e.tp);
    let answer = ask(&mut raw, methods::DIFF_COMMIT, &commit_params(&e, &oid));
    let section = ok(&answer, "diff.commit")["section"].clone();
    assert_eq!(
        section["files_total"], 1_200,
        "files_total counts every changed file"
    );
    assert_eq!(
        section["cut"], "files",
        "more than 1.000 files must cut the section with `files`"
    );
    assert!(
        answer.to_string().len() <= 768 * 1024,
        "the reply is over 768 KiB"
    );
}

#[test]
fn ckp006_006_the_reply_stays_under_768_kib() {
    let (fx, e) = repo(|fx, wt| {
        std::fs::create_dir_all(wt.join("fat")).unwrap();
        let body: String = (0..100).map(|i| format!("{i:03} {:->96}\n", "")).collect();
        for i in 0..150 {
            std::fs::write(wt.join(format!("fat/f{i:03}.txt")), &body).unwrap();
        }
        commit_in(fx, wt, "fat files");
    });
    let oid = head(&fx, &e.wt);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let answer = ask(&mut raw, methods::DIFF_COMMIT, &commit_params(&e, &oid));
    let section = ok(&answer, "diff.commit")["section"].clone();
    assert_eq!(section["files_total"], 150);
    assert_eq!(
        section["cut"], "bytes",
        "150 files of about 10 KiB are over the 768 KiB of a reply"
    );
    assert!(
        answer.to_string().len() <= 768 * 1024,
        "the reply is {} bytes",
        answer.to_string().len()
    );
}

#[test]
fn ckp006_006_a_thousand_conflicts_of_4_kib_keep_the_reply_bounded() {
    let (fx, e) = repo(|_, _| {});
    // 1.000 conflicted paths of about 4 KiB, written to the index only (no file on disk).
    let dir = vec!["d".repeat(250); 15].join("/");
    let blob = fx
        .git_in(&e.wt, &["hash-object", "-w", "a.txt"])
        .trim()
        .to_owned();
    let mut info = String::new();
    for i in 0..1_000 {
        for stage in 1..=3 {
            info.push_str(&format!("100644 {blob} {stage}\t{dir}/f{i:04}\n"));
        }
    }
    let mut child = fx
        .git_command(&e.wt, &["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(info.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success(), "update-index failed");
    let _d = Running::start_with(&e.tp, Some(Duration::from_secs(60)));
    let mut raw = Raw::cli(&e.tp);
    let answer = ask(&mut raw, methods::DIFF_INDEX, &index_params(&e));
    let result = ok(&answer, "diff.index");
    assert_eq!(
        result["conflicts_total"], 1_000,
        "conflicts_total counts every conflict"
    );
    let listed = serde_json::to_string(&result["conflicts"]).unwrap();
    assert!(
        !result["conflicts"].as_array().unwrap().is_empty(),
        "no conflict listed at all"
    );
    assert!(
        listed.len() <= CONFLICTS_RESERVE_BYTES,
        "conflicts take {} bytes, over the 64 KiB reserve",
        listed.len()
    );
    assert!(
        answer.to_string().len() <= 768 * 1024,
        "the reply is {} bytes",
        answer.to_string().len()
    );
}

// ================================================================================== CKP006-007

#[test]
fn ckp006_007_cancel_of_a_diff_commit_in_flight_gets_query_cancelled() {
    let (_fx, e, _d, oid) = heavy();
    let mut raw = Raw::cli(&e.tp);
    let diff_id = raw.next_id;
    let ids = raw.write(&[
        (methods::DIFF_COMMIT, commit_params(&e, &oid)),
        (methods::QUERY_CANCEL, json!({"request": diff_id})),
    ]);
    let cancel = raw.answer(ids[1]);
    assert_eq!(cancel["result"], json!({"cancelled": true}), "{cancel}");
    let diff = raw.answer(ids[0]);
    assert_eq!(
        code_of(&diff),
        Some(QUERY_CANCELLED),
        "the cancelled diff.commit answers query-cancelled: {diff}"
    );
}

#[test]
fn ckp006_007_a_second_query_of_the_connection_is_busy_at_once() {
    let (_fx, e, _d, oid) = heavy();
    let mut raw = Raw::cli(&e.tp);
    let ids = raw.write(&[
        (methods::DIFF_COMMIT, commit_params(&e, &oid)),
        (methods::DIFF_COMMIT, commit_params(&e, &oid)),
    ]);
    let second = raw.answer(ids[1]);
    assert_eq!(
        code_of(&second),
        Some(QUERY_BUSY),
        "a second diff.commit on the connection gets query-busy: {second}"
    );
    assert_eq!(second["error"]["data"]["limit"], 1, "{second}");
    let first = raw.answer(ids[0]);
    assert!(
        first.get("error").is_none(),
        "the first query is not harmed: {first}"
    );
}

#[test]
fn ckp006_007_a_full_queue_is_busy() {
    let (_fx, e, _d, oid) = heavy();
    // 1 running, 4 queued, 1 refused: under the 8 connections of one requester (limits.rs).
    let mut conns: Vec<Raw> = (0..6).map(|_| Raw::cli(&e.tp)).collect();
    let ids: Vec<u64> = conns
        .iter_mut()
        .map(|c| c.write(&[(methods::DIFF_COMMIT, commit_params(&e, &oid))])[0])
        .collect();
    let answers: Vec<Value> = conns
        .iter_mut()
        .zip(ids)
        .map(|(c, id)| c.answer(id))
        .collect();
    let busy = answers
        .iter()
        .filter(|a| code_of(a) == Some(QUERY_BUSY))
        .count();
    assert!(
        busy >= 1,
        "12 connections asking at once must fill the global queue of 4: no query-busy came"
    );
    assert!(
        answers
            .iter()
            .all(|a| a.get("error").is_none() || code_of(a) == Some(QUERY_BUSY)),
        "a query other than the busy ones failed"
    );
}

// ================================================================================== CKP006-008

#[test]
fn ckp006_008_no_exec_commit_between_and_index_never_run_a_program_of_the_repo() {
    let fx = Fixture::busy(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    std::fs::write(wt.join(".gitattributes"), "*.txt filter=evil diff=evil\n").unwrap();
    std::fs::write(wt.join("x.txt"), "one\n").unwrap();
    let first = commit_in(&fx, &wt, "attributes and x");
    std::fs::write(wt.join("x.txt"), "one\ntwo\n").unwrap();
    let second = commit_in(&fx, &wt, "x changes");
    std::fs::write(wt.join("x.txt"), "one\ntwo\nthree\n").unwrap();
    fx.git_in(&wt, &["add", "x.txt"]);
    std::fs::write(wt.join("x.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    std::fs::write(wt.join("y.txt"), "untracked\n").unwrap();
    let canary = Canary::arm(fx);
    let e = env_with(&canary.f, wt);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let commit = ok(
        &ask(&mut raw, methods::DIFF_COMMIT, &commit_params(&e, &second)),
        "diff.commit",
    );
    let between = ok(
        &ask(
            &mut raw,
            methods::DIFF_BETWEEN,
            &between_params(&e, &first, &second),
        ),
        "diff.between",
    );
    let index = ok(
        &ask(&mut raw, methods::DIFF_INDEX, &index_params(&e)),
        "diff.index",
    );
    // A file of diff.index nests its FileDiff under `file` (D7).
    let index_files = |section: &Value| -> Vec<String> {
        section["files"]
            .as_array()
            .unwrap_or_else(|| panic!("no files in {section}"))
            .iter()
            .map(|f| f["file"]["path"]["untrusted"].as_str().unwrap().to_owned())
            .collect()
    };
    // The diffs were computed (the check is not vacuous) ...
    assert_eq!(files_of(&commit["section"]), vec!["x.txt"]);
    assert_eq!(files_of(&between["section"]), vec!["x.txt"]);
    assert_eq!(index_files(&index["staged"]), vec!["x.txt"]);
    assert!(index_files(&index["unstaged"]).contains(&"y.txt".to_owned()));
    // ... and no program of the repo ran.
    assert!(
        canary.fired().is_empty(),
        "a diff ran a program of the repo: {:?}",
        canary.fired()
    );
}

// ================================================================================== CKP006-010/011

/// A repo with a planted secret committed twice and then changed in the working tree.
fn secret_repo() -> (Fixture, Env, String, String) {
    let (fx, e) = repo(|fx, wt| {
        std::fs::write(wt.join("config.txt"), format!("token = {PLANTED}\n")).unwrap();
        commit_in(fx, wt, "config");
    });
    let first = head(&fx, &e.wt);
    std::fs::write(e.wt.join("config.txt"), format!("token = {PLANTED}-2\n")).unwrap();
    let second = commit_in(&fx, &e.wt, "config 2");
    std::fs::write(e.wt.join("config.txt"), format!("token = {PLANTED}-3\n")).unwrap();
    (fx, e, first, second)
}

#[test]
fn ckp006_010_the_secret_reaches_only_who_asked_and_nothing_is_persisted() {
    let (_fx, e, first, second) = secret_repo();
    let d = Running::start(&e.tp);
    let mut watcher = Raw::cli(&e.tp);
    subscribe_global(&mut watcher);
    let mut raw = Raw::cli(&e.tp);
    for _ in 0..3 {
        for (method, params) in [
            (methods::DIFF_COMMIT, commit_params(&e, &second)),
            (methods::DIFF_BETWEEN, between_params(&e, &first, &second)),
            (methods::DIFF_INDEX, index_params(&e)),
        ] {
            let answer = ask(&mut raw, method, &params);
            let _ = ok(&answer, method);
            assert!(
                answer.to_string().contains(PLANTED),
                "{method} must bring the content to who asked: {answer}"
            );
        }
    }
    let marker = d.marker();
    until_event(&mut watcher, marker);
    for line in &watcher.log {
        assert!(
            !line.contains(PLANTED),
            "diff content reached another connection or the stream: {line}"
        );
    }
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

#[test]
fn ckp006_011_log_lines_carry_only_repo_ms_files_and_cut() {
    let (_fx, e, first, second) = secret_repo();
    let d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    for (method, params) in [
        (methods::DIFF_COMMIT, commit_params(&e, &second)),
        (methods::DIFF_BETWEEN, between_params(&e, &first, &second)),
        (methods::DIFF_INDEX, index_params(&e)),
    ] {
        let _ = ok(&ask(&mut raw, method, &params), method);
    }
    drop(d);
    let mut lines: Vec<String> = Vec::new();
    for file in common::files_under(e.tp.root.path()) {
        let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap_or_default()).into_owned();
        lines.extend(text.lines().map(str::to_owned));
    }
    let expected: HashSet<&str> = ["repo", "ms", "files", "cut"].into_iter().collect();
    for event in ["diff_commit", "diff_between", "diff_index"] {
        let mine: Vec<&String> = lines
            .iter()
            .filter(|l| l.split_whitespace().nth(2) == Some(event))
            .collect();
        assert!(!mine.is_empty(), "no `{event}` log line was written");
        for line in mine {
            let keys: HashSet<&str> = line
                .split_whitespace()
                .skip(3)
                .map(|t| t.split('=').next().unwrap())
                .collect();
            assert_eq!(keys, expected, "`{event}` carries other fields: {line}");
            for leaked in [first.as_str(), second.as_str(), "config.txt", PLANTED] {
                assert!(!line.contains(leaked), "`{event}` leaks {leaked}: {line}");
            }
        }
    }
}
