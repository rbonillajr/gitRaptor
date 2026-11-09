//! TS-CKP-011 T001: confirming the base branch with the daemon running refreshes the published
//! base and makes the predictor replan the pairs against it, without a restart (closes G-B of
//! US-CKP-006).
//!
//! `guard.confirm` and `guard.install` are reserved commands: the developer's client cannot be a
//! descendant of the daemon. So the daemon runs in a child process of this test binary and the
//! developer's client runs in a terminal through `script`, the shape of
//! `us_grd_014_confirm_expired.rs`. The predictor, the raw connection and the fixtures are those
//! of `predict_publish.rs` (the daemon predicts in process, `PredictMerger::InProcess`).
//!
//! Temporary profile, home and repos (NFR-01); never this repo nor the real profile. The repo
//! starts WITHOUT a confirmed base (the only exception is C4, which needs an earlier base whose
//! result can arrive late). Layout: `main` (checked out in the main worktree, moves `a.txt`
//! after `a` and `b` forked), the linked worktrees `wt-a` (branch `a`) and `wt-b` (branch `b`)
//! that change the same line of `a.txt`, and `develop` (still at the fork: nothing conflicts
//! with it). Pairs: (a, b), (a, base) and (b, base).
//!
//! No fixed waits: every test waits for the `prediction.changed` that proves the fact, with a
//! deadline only as a ceiling. A fact that must NOT happen is closed by a later marker (a commit
//! of `wt-b`) that has to arrive first.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::prediction::{
    PairLevel, PairReason, PairSideView, PairStateView, PairSummary, PredictionChanged,
};
use gitraptor_api::scope::{Scope, ScopeEventNotification, ScopeSnapshot};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError, socket_path};
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause, TierConfig};
use gitraptor_core::guardrails::constants::STUB_FILE;
use gitraptor_core::guardrails::pending::{GRACE_ENV, WINDOW_ENV};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(20);
const DAEMON_ENV: &str = "RAPTOR_TEST_TSCKP011_DAEMON";
const CLIENT_ENV: &str = "RAPTOR_TEST_TSCKP011_CLIENT";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

// ------------------------------------------------------------------ the daemon (child process)

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
    let tiers = match spec["dormant_ms"].as_u64() {
        Some(ms) => TierConfig {
            dormant_after: Some(Duration::from_millis(ms)),
            check_every: Duration::from_millis(50),
            ..TierConfig::default()
        },
        None => TierConfig::default(),
    };
    let config = DaemonConfig {
        dirs: gitraptor_core::profile::ProfileDirs::under_root(PathBuf::from(
            spec["root"].as_str().unwrap(),
        )),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel,
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers,
        discovery: Default::default(),
        power: Default::default(),
        predict: PredictConfig {
            merger: PredictMerger::InProcess,
        },
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

// ------------------------------------------------------------------ the developer's client

/// The developer's client: `confirm` asks for the plan, accepts it and, when the base changes
/// (a window opens), applies it once the window closed; `install` asks `guard.install`.
#[test]
fn developer_client_entry() {
    let Some(spec) = std::env::var_os(CLIENT_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = gitraptor_core::profile::ProfileDirs::under_root(PathBuf::from(
        spec["root"].as_str().unwrap(),
    ));
    let path = spec["path"].as_str().unwrap();
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let mut call = |method: &str, params: Value| match client.call::<_, Value>(method, params) {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let mut answers = Vec::new();
    if spec["op"] == "install" {
        answers.push(call(methods::GUARD_INSTALL, json!({ "path": path })));
    } else {
        let plan = call(methods::GUARD_CONFIRM, json!({ "path": path }));
        answers.push(plan.clone());
        if let Some(fingerprint) = plan["ok"]["fingerprint"].as_str() {
            let accepted = call(
                methods::GUARD_CONFIRM,
                json!({ "path": path, "accept": fingerprint }),
            );
            answers.push(accepted.clone());
            if let Some(action_id) = accepted["ok"]["action"]["action_id"].as_str() {
                let begin = Instant::now();
                loop {
                    let applied = call(
                        methods::GUARD_CONFIRM,
                        json!({ "path": path, "confirm": action_id }),
                    );
                    let done = applied["ok"]["result"] == "applied";
                    answers.push(applied);
                    if done || begin.elapsed() > DEADLINE {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

// ------------------------------------------------------------------ the repo

struct Env {
    tp: TempProfile,
    fx: Fixture,
    raptor: PathBuf,
    wt_a: PathBuf,
    wt_b: PathBuf,
    repo_id: String,
    calls: std::cell::Cell<u32>,
}

fn commit_in(fx: &Fixture, wt: &Path, file: &str, content: &str) {
    std::fs::write(wt.join(file), content).unwrap();
    fx.git_in(wt, &["add", "."]);
    fx.git_in(wt, &["commit", "-q", "-m", "work"]);
}

/// The repo, its worktrees and the profile, before any daemon starts. `prior` is a base the
/// profile already confirmed (only C4); `team_base` commits team settings naming that base on
/// `main`.
fn env(prior: Option<&str>, team_base: Option<&str>) -> Env {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "a"]);
    fx.git(&["branch", "b"]);
    fx.git(&["branch", "develop"]);
    let wt_a = canonical(&fx.add_worktree("a", "a"));
    let wt_b = canonical(&fx.add_worktree("b", "b"));
    // `main` moves the line `a` and `b` will also change: both conflict with the base `main`.
    fx.write("a.txt", "alpha-main\n");
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "main moves a.txt"]);
    if let Some(base) = team_base {
        std::fs::create_dir_all(fx.repo.join(".gitraptor")).unwrap();
        std::fs::write(
            fx.repo.join(".gitraptor/settings.json"),
            format!(r#"{{"engine":{{"baseBranch":"{base}"}}}}"#),
        )
        .unwrap();
        fx.git(&["add", ".gitraptor/settings.json"]);
        fx.git(&["commit", "-q", "-m", "team base"]);
    }
    commit_in(&fx, &wt_a, "a.txt", "alpha-a\n");
    commit_in(&fx, &wt_b, "a.txt", "alpha-b\n");
    let tp = TempProfile::new();
    let raptor = stand_in_raptor(&tp.root.path().join("bin"));
    let repo_id = {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        if let Some(base) = prior {
            let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
            store
                .set_confirmed_team_baseline(&Confirmed {
                    base_branch: RefName::new(base).unwrap(),
                    floor: ConfirmedFloor::Absent,
                })
                .unwrap();
        }
        entry.repo_id
    };
    Env {
        tp,
        fx,
        raptor,
        wt_a,
        wt_b,
        repo_id,
        calls: std::cell::Cell::new(0),
    }
}

fn start_daemon(e: &Env, dormant_ms: Option<u64>) -> DaemonProc {
    let stop = e.tp.root.path().join("stop-daemon");
    let spec = json!({
        "root": e.tp.root.path().join("profile"),
        "home": e.fx.home,
        "raptor": e.raptor,
        "stop": stop,
        "dormant_ms": dormant_ms,
    });
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["daemon_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(DAEMON_ENV, spec.to_string())
        .env(WINDOW_ENV, "50")
        .env(GRACE_ENV, "5000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while Client::connect(&e.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
        assert!(start.elapsed() < DEADLINE, "the daemon never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    DaemonProc { child, stop }
}

/// What the developer's client (a terminal through `script`) answered to `op`.
fn developer(e: &Env, op: &str) -> Vec<Value> {
    let n = e.calls.get() + 1;
    e.calls.set(n);
    let out = e.tp.root.path().join(format!("developer-answers-{n}.json"));
    let spec = json!({
        "root": e.tp.root.path().join("profile"),
        "path": e.fx.repo,
        "op": op,
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
        .env("HOME", e.tp.root.path())
        .env(CLIENT_ENV, spec.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(out.exists(), "the developer's client ended ({status})");
    serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
}

/// The developer confirmed the base: the last answer says `applied`.
fn assert_confirmed(answers: &[Value]) {
    assert_eq!(
        answers.last().map(|a| a["ok"]["result"].clone()),
        Some(json!("applied")),
        "guard.confirm did not apply: {answers:#?}"
    );
}

// ------------------------------------------------------------------ the wire

/// A raw connection: a line out, a line in. Notifications that arrive before an answer wait in
/// `notes`.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    line: String,
    notes: VecDeque<Value>,
    next_id: u64,
}

impl Raw {
    fn capable(tp: &TempProfile) -> Self {
        let path = socket_path(&tp.dirs()).unwrap();
        let stream = UnixStream::connect(&path).expect("the daemon listens");
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            line: String::new(),
            notes: VecDeque::new(),
            next_id: 1,
        };
        let _: Value = raw.result(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        let granted: Value = raw.result(
            methods::CONNECTION_ACCEPT,
            json!({"capabilities": [methods::CAP_PREDICTION_PAIRS.name]}),
        );
        assert!(
            granted["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == methods::CAP_PREDICTION_PAIRS.name),
            "the daemon does not serve prediction.pairs: {granted}"
        );
        raw
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

    fn call(&mut self, method: &str, params: Value) -> Value {
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
                return value;
            }
            self.notes.push_back(value);
        }
    }

    fn result<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> T {
        let answer = self.call(method, params);
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        serde_json::from_value(answer["result"].clone()).unwrap()
    }

    /// The next `scope.event`, or `None` when `timeout` ran out.
    fn next_event(&mut self, timeout: Duration) -> Option<ScopeEventNotification> {
        let start = Instant::now();
        loop {
            let note = match self.notes.pop_front() {
                Some(note) => note,
                None => self.read_value(timeout.saturating_sub(start.elapsed()))?,
            };
            if note["method"] == methods::NOTIFY_SCOPE_EVENT {
                return Some(serde_json::from_value(note["params"].clone()).unwrap());
            }
        }
    }

    fn snapshot(&mut self, scope: &Scope) -> ScopeSnapshot {
        self.result(methods::SCOPE_SNAPSHOT, json!({"scope": scope}))
    }

    fn subscribe(&mut self, scope: &Scope, snap: &ScopeSnapshot) {
        let _: Value = self.result(
            methods::SCOPE_SUBSCRIBE,
            json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
        );
    }
}

// ------------------------------------------------------------------ pairs

fn side_is(side: &PairSideView, want: Option<&Path>) -> bool {
    match (side, want) {
        (PairSideView::Worktree { root }, Some(p)) => Path::new(root.raw()) == p,
        (PairSideView::Base {}, None) => true,
        _ => false,
    }
}

fn is_pair(p: &PairSummary, a: &Path, b: Option<&Path>) -> bool {
    side_is(&p.a, Some(a)) && side_is(&p.b, b)
}

fn find<'a>(
    pairs: &'a BTreeMap<String, PairSummary>,
    a: &Path,
    b: Option<&Path>,
) -> Option<&'a PairSummary> {
    pairs.values().find(|p| is_pair(p, a, b))
}

fn brief(pairs: &BTreeMap<String, PairSummary>) -> String {
    pairs
        .values()
        .map(|p| format!("{:?}/{:?}/{:?}", p.state, p.reason, p.level))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A pair as one `prediction.changed` batch carried it.
struct Seen {
    pair: PairSummary,
}

/// A connection that negotiated `prediction.pairs`, subscribed to the repo scope, with the pairs
/// it knows from its snapshot and the events since.
struct Watch {
    raw: Raw,
    scope: Scope,
    pairs: BTreeMap<String, PairSummary>,
    seen: Vec<Seen>,
}

impl Watch {
    fn start(tp: &TempProfile, repo_id: &str) -> Self {
        let mut raw = Raw::capable(tp);
        let scope = Scope::Repo {
            repo_id: repo_id.to_owned(),
        };
        let snap = raw.snapshot(&scope);
        let ScopeSnapshot::Repo(repo) = &snap else {
            panic!("{snap:?}")
        };
        let prediction = repo.repo.prediction.clone().expect(
            "RepoView.prediction is missing for a connection that negotiated prediction.pairs",
        );
        let pairs = prediction
            .pairs
            .into_iter()
            .map(|p| (p.pair_id.clone(), p))
            .collect();
        raw.subscribe(&scope, &snap);
        Self {
            raw,
            scope,
            pairs,
            seen: Vec::new(),
        }
    }

    fn apply(&mut self, e: ScopeEventNotification) {
        assert_eq!(e.scope, self.scope);
        if e.event.kind != methods::PREDICTION_CHANGED {
            return;
        }
        let changed: PredictionChanged = serde_json::from_value(e.event.data).unwrap();
        for id in &changed.removed {
            self.pairs.remove(id);
        }
        for pair in changed.pairs {
            self.pairs.insert(pair.pair_id.clone(), pair.clone());
            self.seen.push(Seen { pair });
        }
    }

    /// Reads events until `done`, within the deadline.
    fn pump(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            let left = DEADLINE.saturating_sub(start.elapsed());
            assert!(
                !left.is_zero(),
                "no prediction.changed brought {what}; pairs now: [{}]",
                brief(&self.pairs)
            );
            if let Some(e) = self.raw.next_event(left) {
                self.apply(e);
            }
        }
    }

    fn mark(&self) -> usize {
        self.seen.len()
    }

    /// What the batches since `mark` said about one pair, in order.
    fn since(&self, mark: usize, a: &Path, b: Option<&Path>) -> Vec<&PairSummary> {
        self.seen[mark..]
            .iter()
            .map(|s| &s.pair)
            .filter(|p| is_pair(p, a, b))
            .collect()
    }

    /// The state of a pair as the batches left it.
    fn state_of(&self, a: &Path, b: Option<&Path>) -> Option<PairStateView> {
        find(&self.pairs, a, b).map(|p| p.state)
    }

    /// Waits until the base pair of `a` waits for a confirmed base and the pair of the two
    /// worktrees is calculated: the predictor settled with no base.
    fn settle_without_base(&mut self, e: &Env) {
        self.pump("the base pair pending (base-unconfirmed)", |w| {
            find(&w.pairs, &e.wt_a, None).is_some_and(|p| p.state == PairStateView::Pending)
                && find(&w.pairs, &e.wt_a, None).and_then(|p| p.reason)
                    == Some(PairReason::BaseUnconfirmed)
                && w.state_of(&e.wt_a, Some(&e.wt_b)) == Some(PairStateView::Current)
        });
    }

    /// A commit in `wt-b` and the wait for the batch of its own pair with the base: whatever the
    /// daemon was going to publish for the pair of `wt-a` before it has arrived by then.
    fn marker(&mut self, e: &Env, file: &str) {
        let at = self.mark();
        commit_in(&e.fx, &e.wt_b, file, "marker\n");
        self.pump(
            "the marker: the pair of wt-b with the base recalculated",
            |w| {
                w.since(at, &e.wt_b, None)
                    .iter()
                    .any(|p| p.state == PairStateView::Current)
            },
        );
    }
}

// ================================================================================== tests

/// C1: after `guard.confirm` the base pair goes pending -> calculating -> current, with its
/// level and its conflicting files, all by `prediction.changed`.
#[test]
fn confirm_turns_the_base_pair_from_pending_to_current() {
    let e = env(None, None);
    let _daemon = start_daemon(&e, None);
    let mut w = Watch::start(&e.tp, &e.repo_id);
    w.settle_without_base(&e);
    let mark = w.mark();

    assert_confirmed(&developer(&e, "confirm"));

    w.pump(
        "C1: the base pair current after confirming the base (the daemon did not replan)",
        |w| w.state_of(&e.wt_a, None) == Some(PairStateView::Current),
    );
    let states: Vec<PairStateView> = w
        .since(mark, &e.wt_a, None)
        .iter()
        .map(|p| p.state)
        .collect();
    let calculating = states.iter().position(|s| *s == PairStateView::Calculating);
    let current = states.iter().position(|s| *s == PairStateView::Current);
    assert!(
        calculating.is_some() && calculating < current,
        "C1: the pair must pass through calculating before current; batches: {states:?}"
    );
    let pair = find(&w.pairs, &e.wt_a, None).unwrap();
    assert_eq!(
        pair.level,
        Some(PairLevel::Conflict),
        "C1: wt-a and main change the same line of a.txt"
    );
    assert_eq!(pair.conflict_files, 1, "C1: a.txt is the conflicting file");
}

/// C2: the same when `guard.install` confirms the base (`journal.confirms_base`).
#[test]
fn install_confirming_the_base_replans() {
    let e = env(None, None);
    let _daemon = start_daemon(&e, None);
    let mut w = Watch::start(&e.tp, &e.repo_id);
    w.settle_without_base(&e);

    let answers = developer(&e, "install");
    assert!(
        answers.first().is_some_and(|a| a.get("ok").is_some()),
        "guard.install did not install: {answers:#?}"
    );

    w.pump(
        "C2: the base pair current after the install confirmed the base (the daemon did not replan)",
        |w| w.state_of(&e.wt_a, None) == Some(PairStateView::Current),
    );
    let pair = find(&w.pairs, &e.wt_a, None).unwrap();
    assert_eq!(
        pair.level,
        Some(PairLevel::Conflict),
        "C2: the pair is calculated against the confirmed base main"
    );
}

/// C3: the first confirmation brings the pair to current; confirming the same base again
/// publishes nothing about that pair.
#[test]
fn confirming_the_same_base_twice_publishes_once() {
    let e = env(None, None);
    let _daemon = start_daemon(&e, None);
    let mut w = Watch::start(&e.tp, &e.repo_id);
    w.settle_without_base(&e);

    assert_confirmed(&developer(&e, "confirm"));
    w.pump(
        "C3: the base pair current after the first confirmation (the daemon did not replan)",
        |w| w.state_of(&e.wt_a, None) == Some(PairStateView::Current),
    );
    let mark = w.mark();

    let again = developer(&e, "confirm");
    assert!(
        again.first().is_some_and(|a| a.get("ok").is_some()),
        "the second guard.confirm failed: {again:#?}"
    );
    w.marker(&e, "marker-c3.txt");

    let late: Vec<_> = w.since(mark, &e.wt_a, None);
    assert!(
        late.is_empty(),
        "C3: confirming the same base again published the pair of wt-a with the base: {:?}",
        late.iter().map(|p| (p.state, p.level)).collect::<Vec<_>>()
    );
}

/// C4: confirming another branch (`develop`, which does not conflict with `a`) as base leaves
/// the pair calculated against it; no later batch carries the level against the old base.
/// The profile starts with `main` confirmed: a result against an earlier base is what could
/// arrive late.
#[test]
fn a_new_base_replans_against_it_and_drops_late_results() {
    let e = env(Some("main"), Some("develop"));
    let _daemon = start_daemon(&e, None);
    let mut w = Watch::start(&e.tp, &e.repo_id);
    w.pump(
        "the pair against the confirmed main current with a conflict",
        |w| {
            find(&w.pairs, &e.wt_a, None).is_some_and(|p| {
                p.state == PairStateView::Current && p.level == Some(PairLevel::Conflict)
            })
        },
    );
    let mark = w.mark();

    assert_confirmed(&developer(&e, "confirm"));

    w.pump(
        "C4: the pair of wt-a calculated against develop (no conflict); the daemon kept main",
        |w| {
            find(&w.pairs, &e.wt_a, None).is_some_and(|p| {
                p.state == PairStateView::Current && p.level == Some(PairLevel::Clean)
            })
        },
    );
    // A late result against `main` would arrive before the marker of a later change.
    w.marker(&e, "marker-c4.txt");
    let stale: Vec<_> = w
        .since(mark, &e.wt_a, None)
        .into_iter()
        .filter(|p| p.state == PairStateView::Current && p.level == Some(PairLevel::Conflict))
        .collect();
    assert!(
        stale.is_empty(),
        "C4: a batch carried the level calculated against the old base after confirming develop"
    );
    let last = find(&w.pairs, &e.wt_a, None).unwrap();
    assert_eq!(
        (last.state, last.level),
        (PairStateView::Current, Some(PairLevel::Clean)),
        "C4: the pair ends calculated against develop"
    );
}

/// C5: with the repo dormant the confirmation publishes no calculation; once it is active
/// again the pair comes out current against the confirmed base.
#[test]
fn a_dormant_repo_replans_on_waking() {
    let e = env(None, None);
    let _daemon = start_daemon(&e, Some(500));
    // Nobody subscribes (a subscribed client keeps the repo active) and nobody asks the daemon
    // anything (a request wakes it): the daemon's own log says it went dormant.
    let log = e.tp.dirs().state.join(gitraptor_core::daemon::LOG_FILE);
    let begin = Instant::now();
    while !std::fs::read_to_string(&log).is_ok_and(|text| text.contains("repo_dormant")) {
        assert!(
            begin.elapsed() < DEADLINE,
            "the repo never went dormant; daemon log: {}",
            std::fs::read_to_string(&log).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    assert_confirmed(&developer(&e, "confirm"));

    let mut w = Watch::start(&e.tp, &e.repo_id);
    w.pump(
        "C5: the base pair current against the confirmed base once the repo is active (the daemon did not replan)",
        |w| w.state_of(&e.wt_a, None) == Some(PairStateView::Current),
    );
    let pair = find(&w.pairs, &e.wt_a, None).unwrap();
    assert_eq!(
        pair.level,
        Some(PairLevel::Conflict),
        "C5: the pair is calculated against the confirmed base main"
    );
}
