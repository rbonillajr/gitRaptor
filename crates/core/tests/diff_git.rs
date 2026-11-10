//! US-CKP-012 D5 to D7: the semantics of `diff.worktree` over Git (sections, no execution of
//! filters, `textconv` or an external diff, symlinks, binaries, read only), asked of a real
//! daemon (in-process) through the channel, because the reading code has no API of its own yet.
//! A temporary profile and temporary repos (NFR-01); never this repo. No fixed waits.
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
    CommittedDiff, CommittedGap, DiffParams, DiffSection, FileBody, WorktreeDiff,
};
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::methods;
use gitraptor_core::channel::EventBus;
use gitraptor_core::client::socket_path;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::canary::Canary;
use gitraptor_testkit::fingerprint;
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

// ================================================================================== sections

/// `main` (README, a.txt, b.txt, `.gitignore`) and `feat` branched from it; then `main` moves
/// on (`moved.txt`), so the merge-base is not the tip of `main`. `feat` adds `feat.txt` and
/// has work not committed: `a.txt` modified (not staged), `staged.txt` (staged), `new.txt`
/// (untracked) and `noise.log` (ignored).
fn sections_env(confirm: bool) -> (Fixture, Env) {
    let fx = Fixture::with_commit(&git());
    fx.write("a.txt", "alpha\n");
    fx.write("b.txt", "beta\n");
    fx.write(".gitignore", "*.log\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "base"]);
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    fx.write("moved.txt", "main moved\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "main moves"]);
    std::fs::write(wt.join("feat.txt"), "feat\n").unwrap();
    commit_in(&fx, &wt, "feat work");
    std::fs::write(wt.join("a.txt"), "alpha edited\n").unwrap();
    std::fs::write(wt.join("staged.txt"), "staged\n").unwrap();
    fx.git_in(&wt, &["add", "staged.txt"]);
    std::fs::write(wt.join("new.txt"), "new\n").unwrap();
    std::fs::write(wt.join("noise.log"), "noise\n").unwrap();
    let e = env_with(&fx, wt, confirm);
    (fx, e)
}

#[test]
fn sections_committed_starts_at_the_merge_base_and_uncommitted_has_all_three_kinds() {
    let (_fx, e) = sections_env(true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let diff = diff_of(&ask_diff(&mut raw, &e));
    let committed = ready(&diff);
    assert_eq!(
        paths(committed),
        ["feat.txt"],
        "the committed section must start at the merge-base with the confirmed base (not at the tip of main)"
    );
    assert_eq!(committed.files_total, 1);
    match &diff.committed {
        CommittedDiff::Ready { base, .. } => assert_eq!(base.raw(), "main"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        paths(&diff.uncommitted),
        ["a.txt", "new.txt", "staged.txt"],
        "uncommitted = staged + unstaged + untracked not ignored (noise.log is ignored)"
    );
    assert_eq!(diff.uncommitted.files_total, 3);
}

#[test]
fn sections_unconfirmed_base_makes_committed_not_computable_and_keeps_uncommitted() {
    let (_fx, e) = sections_env(false);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let diff = diff_of(&ask_diff(&mut raw, &e));
    assert!(
        matches!(
            diff.committed,
            CommittedDiff::NotComputable {
                reason: CommittedGap::BaseUnconfirmed
            }
        ),
        "an unconfirmed base makes the committed section not-computable: {:?}",
        diff.committed
    );
    assert_eq!(
        paths(&diff.uncommitted),
        ["a.txt", "new.txt", "staged.txt"],
        "the uncommitted section is computed all the same"
    );
}

// ================================================================================== no exec

/// What the daemon sends back never holds the planted secret.
const SECRET: &str = "SECRET-OUTSIDE-6d1f";

#[test]
fn no_exec_filters_textconv_and_external_diff_never_run() {
    // `Fixture::busy` + a canary: clean filter, textconv and `diff.external` point to scripts
    // that leave a marker. `feat` carries the attributes, so the surface is armed there too.
    let fx = Fixture::busy(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    std::fs::write(wt.join(".gitattributes"), "*.txt filter=evil diff=evil\n").unwrap();
    std::fs::write(wt.join("x.txt"), "one\n").unwrap();
    commit_in(&fx, &wt, "attributes and x");
    std::fs::write(wt.join("x.txt"), "one\ntwo\n").unwrap();
    std::fs::write(wt.join("y.txt"), "untracked\n").unwrap();
    let canary = Canary::arm(fx);
    // R7 of DS-US-DSK-009: `diff.external` without a subsection is armed, so the check below
    // also proves that it is never consulted (DS-TS-CKP-006 T006).
    assert_eq!(
        canary.f.git(&["config", "--get", "diff.external"]).trim(),
        canary.bin.join("diff-external").display().to_string(),
        "the canary must arm diff.external without a subsection"
    );
    let e = env_with(&canary.f, wt, true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let diff = diff_of(&ask_diff(&mut raw, &e));
    // The diff was computed (so the check is not vacuous) ...
    assert!(
        paths(ready(&diff)).contains(&".gitattributes".to_owned())
            && paths(&diff.uncommitted).contains(&"x.txt".to_owned()),
        "{diff:?}"
    );
    // ... and no program of the repo ran.
    assert!(
        canary.fired().is_empty(),
        "the diff ran a program of the repo: {:?}",
        canary.fired()
    );
}

#[test]
fn no_exec_a_symlink_as_a_file_is_not_followed() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    let outside = fx.root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("target.txt"), format!("{SECRET}\n")).unwrap();
    // One link in a commit, one still not committed.
    std::os::unix::fs::symlink(outside.join("target.txt"), wt.join("committed-link")).unwrap();
    commit_in(&fx, &wt, "link");
    std::os::unix::fs::symlink(outside.join("target.txt"), wt.join("new-link")).unwrap();
    let e = env_with(&fx, wt, true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let answer = ask_diff(&mut raw, &e);
    let diff = diff_of(&answer);
    for (section, name) in [
        (ready(&diff), "committed-link"),
        (&diff.uncommitted, "new-link"),
    ] {
        let file = section
            .files
            .iter()
            .find(|f| f.path.raw() == name)
            .unwrap_or_else(|| panic!("no entry for {name}: {section:?}"));
        assert!(
            matches!(file.body, FileBody::Symlink {}),
            "{name} must come out as a symlink: {:?}",
            file.body
        );
    }
    assert!(
        !answer.to_string().contains(SECRET),
        "the content of a link target went out"
    );
}

#[test]
fn no_exec_a_symlink_as_the_parent_directory_is_not_followed() {
    let fx = Fixture::with_commit(&git());
    std::fs::create_dir_all(fx.repo.join("sub")).unwrap();
    fx.write("sub/f.txt", "inside\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "sub"]);
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    let outside = fx.root.join("outside-dir");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("f.txt"), format!("{SECRET}\n")).unwrap();
    // `sub/` becomes a link to a folder outside the worktree that has its own `f.txt`.
    std::fs::remove_dir_all(wt.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside, wt.join("sub")).unwrap();
    let e = env_with(&fx, wt, true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let answer = ask_diff(&mut raw, &e);
    let diff = diff_of(&answer);
    assert!(
        !answer.to_string().contains(SECRET),
        "a link used as a parent directory was followed: {diff:?}"
    );
    let sub = diff
        .uncommitted
        .files
        .iter()
        .find(|f| f.path.raw() == "sub")
        .unwrap_or_else(|| panic!("the link `sub` has no entry: {:?}", diff.uncommitted));
    assert!(
        matches!(sub.body, FileBody::Symlink {}),
        "`sub` must come out as a symlink: {:?}",
        sub.body
    );
}

#[test]
fn no_exec_nul_in_the_first_8000_bytes_is_binary() {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    // NUL as the byte 7.999 (the last one looked at) and as the byte 9.000 (not looked at).
    let mut inside = vec![b'a'; 7_999];
    inside.push(0);
    inside.extend_from_slice(b"\ntail\n");
    let mut beyond = b"line\n".repeat(1_800);
    beyond.push(0);
    beyond.extend_from_slice(b"\ntail\n");
    std::fs::write(wt.join("inside.bin"), &inside).unwrap();
    std::fs::write(wt.join("beyond.dat"), &beyond).unwrap();
    commit_in(&fx, &wt, "files");
    let e = env_with(&fx, wt, true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    let diff = diff_of(&ask_diff(&mut raw, &e));
    let body = |name: &str| {
        ready(&diff)
            .files
            .iter()
            .find(|f| f.path.raw() == name)
            .unwrap_or_else(|| panic!("no entry for {name}"))
            .body
            .clone()
    };
    assert!(
        matches!(body("inside.bin"), FileBody::Binary {}),
        "a NUL in the first 8.000 bytes is binary: {:?}",
        body("inside.bin")
    );
    assert!(
        !matches!(body("beyond.dat"), FileBody::Binary {}),
        "a NUL after the first 8.000 bytes is not looked at"
    );
}

// ================================================================================== read only

#[test]
fn writes_nothing_to_the_repo_when_the_diff_is_requested() {
    let (fx, e) = sections_env(true);
    let _d = Running::start(&e.tp);
    let mut raw = Raw::cli(&e.tp);
    // The first answer proves the engine has observed the repo: whatever it writes on its own
    // is done by now. The footprint of everything but the engine's profile is the reference.
    let first = diff_of(&ask_diff(&mut raw, &e));
    assert!(first.uncommitted.files_total > 0);
    let before = fx.fingerprint();
    fingerprint::wait_for_timestamp_tick();
    for _ in 0..3 {
        let _ = diff_of(&raw.call(methods::DIFF_WORKTREE, params(&e)));
    }
    let changes: Vec<_> = fingerprint::diff(&before, &fx.fingerprint())
        .into_iter()
        .filter(|c| c.scope != "profile")
        .collect();
    assert!(
        changes.is_empty(),
        "requesting the diff wrote in the repo (index, refs, config, objects or the worktree): {changes:#?}"
    );
}
