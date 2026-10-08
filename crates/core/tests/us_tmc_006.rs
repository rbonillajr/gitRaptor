//! US-TMC-006 end to end (DS-US-TMC-006 § 9): `timemachine.timeline` over a
//! real daemon (in-process) observing a testkit fixture (temporary repo,
//! worktrees and home) with a separate temporary profile; never this repo or
//! the real profile (NFR-01). No test waits a fixed time: each step waits for
//! a state, with a deadline.
//!
//! Written test-first against the raw method name and `serde_json::Value`:
//! the result types are the contract of DS-US-TMC-006 "Tipos y datos
//! compartidos" (`TimelineResult`, `TimelineEntry`, `ChangedFiles`, ...), and
//! the field names below are the wire form that spec defines.
//!
//! In-process clients and the test's own `git` resolve as "unattributed"; the
//! agent cases (detected, registered) are the end-to-end tests of the CLI
//! (`apps/cli/tests/timeline_process.rs`).
//!
//! macOS and Linux, like the other channel tests.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LOG_FILE, LogLimits, ShutdownHandle, StopCause, StopReport,
    TmCapture,
};
use gitraptor_core::timemachine::continuous::{CaptureConfig, CaptureLayer};
use gitraptor_core::timemachine::store::{CaptureError, SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

/// The method under test (DS-US-TMC-006 T004), by its wire name: the result
/// types do not exist yet.
const TIMELINE: &str = "timemachine.timeline";
const DEADLINE: Duration = Duration::from_secs(20);

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Running {
    fx: Fixture,
    tp: TempProfile,
    repo_id: String,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

fn connect(tp: &TempProfile) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Observes `fx`'s repo in a fresh profile and starts the daemon with a
/// short quiet time; `layer` makes every continuous capture fail.
fn start(fx: Fixture, layer: Option<CaptureLayer>) -> Running {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    let (entry, _) = profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
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
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: true,
                quiet: Duration::from_millis(150),
                max_interval: Duration::from_millis(600),
            },
            layer,
            no_free_space_floor: true,
        },
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let r = Running {
        fx,
        tp,
        repo_id: entry.repo_id,
        handle,
        join: Some(join),
    };
    // Serving: the channel answers.
    drop(connect(&r.tp));
    r
}

impl Running {
    /// The raw answer of `timemachine.timeline` for `params`.
    fn timeline_raw(&self, params: Value) -> Result<Value, ClientError> {
        connect(&self.tp).call(TIMELINE, params)
    }

    /// The timeline anchored at `worktree`; panics with the daemon's error
    /// (today, "not implemented") when it does not answer.
    fn timeline(&self, worktree: &Path) -> Value {
        self.timeline_with(json!({ "worktree": worktree.to_str().unwrap() }))
    }

    fn timeline_with(&self, params: Value) -> Value {
        match self.timeline_raw(params) {
            Ok(v) => v,
            Err(e) => panic!("{TIMELINE} did not answer: {e:?}"),
        }
    }

    fn undo(&self, worktree: &Path) -> Value {
        connect(&self.tp)
            .call(
                methods::TM_UNDO,
                json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
            )
            .unwrap()
    }

    fn store(&self) -> Option<SnapshotStore> {
        SnapshotStore::open_existing(&self.tp.dirs(), &self.repo_id)
            .ok()
            .flatten()
    }

    /// `path → bytes` of worktree `key` in snapshot `id`.
    fn files(&self, store: &SnapshotStore, id: &str, key: &str) -> Vec<(String, Vec<u8>)> {
        store
            .files(id, key)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
            .map(|(p, _, oid)| (p, store.read_blob(oid).unwrap()))
            .collect()
    }

    /// Waits for a snapshot of worktree `key` holding `path` with `content`.
    fn captured(&self, key: &str, path: &str, content: &[u8]) -> String {
        let start = Instant::now();
        loop {
            if let Some(store) = self.store() {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    if self
                        .files(&store, id, key)
                        .iter()
                        .any(|(p, b)| p == path && b == content)
                    {
                        return id.clone();
                    }
                }
            }
            assert!(start.elapsed() < DEADLINE, "{path} never captured");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.tp.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// Waits until the daemon log has `event`.
    fn logged(&self, event: &str) {
        let log = self.tp.dirs().state.join(LOG_FILE);
        let start = Instant::now();
        while !std::fs::read_to_string(&log).is_ok_and(|t| t.contains(event)) {
            assert!(start.elapsed() < DEADLINE, "{event} never logged");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits until `events.history` has an event whose `new_commit` is the
    /// `HEAD` of `worktree`; the event.
    fn event_at_head(&self, worktree: &Path) -> Value {
        let head = self.fx.git_in(worktree, &["rev-parse", "HEAD"]);
        let head = head.trim();
        let start = Instant::now();
        loop {
            let page: Value = connect(&self.tp)
                .call(methods::EVENTS_HISTORY, json!({ "repo_id": self.repo_id }))
                .unwrap();
            let events = page["events"].as_array().cloned().unwrap_or_default();
            if let Some(e) = events
                .iter()
                .find(|e| e["details"]["new_commit"].as_str() == Some(head))
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no event for {head}: {events:#?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits until `events.history` has an event of `kind` that is not in `seen`; the event.
    fn event_of_kind(&self, kind: &str, seen: &[i64]) -> Value {
        let start = Instant::now();
        loop {
            let page: Value = connect(&self.tp)
                .call(methods::EVENTS_HISTORY, json!({ "repo_id": self.repo_id }))
                .unwrap();
            let events = page["events"].as_array().cloned().unwrap_or_default();
            if let Some(e) = events
                .iter()
                .find(|e| e["kind"] == kind && !seen.contains(&seq_of(e)))
            {
                return e.clone();
            }
            assert!(start.elapsed() < DEADLINE, "no {kind} event: {events:#?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Commits `files` (path → content) in `worktree` with `message`, raw
    /// Git, and waits for the engine to see it; its history event.
    fn commit(&self, worktree: &Path, files: &[(String, String)], message: &str) -> Value {
        for (path, content) in files {
            std::fs::write(worktree.join(path), content).unwrap();
        }
        self.fx.git_in(worktree, &["add", "-A"]);
        self.fx.git_in(worktree, &["commit", "-q", "-m", message]);
        self.event_at_head(worktree)
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

/// A repo with `api.rs` committed and a linked worktree "feat-login" on its
/// own branch. The worktree's key in a snapshot is `wt-wt-feat-login`.
fn repo_with_login() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.write("api.rs", "fn api() {}\n");
    fx.git(&["add", "api.rs"]);
    fx.git(&["commit", "-q", "-m", "api"]);
    fx.git(&["branch", "feat-login"]);
    let wt = fx.add_worktree("feat-login", "feat-login");
    (fx, canonical(&wt))
}

const KEY: &str = "wt-wt-feat-login";

fn files_of(names: &[&str]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|n| ((*n).to_owned(), format!("content of {n}\n")))
        .collect()
}

fn numbered(prefix: &str, n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| (format!("{prefix}{i:02}.txt"), format!("content {i}\n")))
        .collect()
}

// ----- Reading the wire form ------------------------------------------------------

fn entries(timeline: &Value) -> &Vec<Value> {
    timeline["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("no `entries` array: {timeline:#}"))
}

/// The entry of the Git event with `seq` (`"event:<seq>"`).
fn event_entry(timeline: &Value, seq: i64) -> &Value {
    let id = format!("event:{seq}");
    entries(timeline)
        .iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no entry {id}: {timeline:#}"))
}

/// The paths of an `available` `files`, as plain strings (`Untrusted`
/// travels as `{"untrusted": "<text>"}`).
fn paths(entry: &Value) -> Vec<String> {
    let files = &entry["files"];
    assert_eq!(files["state"], "available", "{entry:#}");
    files["paths"]
        .as_array()
        .unwrap_or_else(|| panic!("no paths: {entry:#}"))
        .iter()
        .map(|p| p["untrusted"].as_str().unwrap().to_owned())
        .collect()
}

fn seq_of(event: &Value) -> i64 {
    event["seq"].as_i64().unwrap()
}

// ----- Scenarios ----------------------------------------------------------------

/// Escenario 1: a raw commit shows when, where (worktree) and which files.
#[test]
fn a_raw_commit_shows_when_where_and_which_files() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let before = now_ms();
    let event = r.commit(&wt, &files_of(&["login.rs", "util.rs"]), "add login");
    let after = now_ms();

    let timeline = r.timeline(&wt);
    assert_eq!(timeline["repo_id"], r.repo_id.as_str());
    assert_eq!(timeline["truncated"], false);
    assert_eq!(timeline["unavailable"], json!([]));

    let entry = event_entry(&timeline, seq_of(&event));
    // What happened: a Git event of kind commit.
    assert_eq!(entry["origin"]["entry"], "git-event", "{entry:#}");
    assert_eq!(entry["origin"]["kind"], "commit", "{entry:#}");
    assert_eq!(entry["origin"]["seq"], event["seq"]);
    // When: UTC milliseconds within the test's window, and the local offset.
    let at = entry["occurred_utc_ms"].as_i64().unwrap();
    assert!(
        (before - 1_000..=after + 1_000).contains(&at),
        "{at} not in {before}..{after}"
    );
    assert!(entry["utc_offset_s"].is_i64(), "{entry:#}");
    // Where: the worktree the commit happened in.
    let worktrees: Vec<&str> = entry["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["untrusted"].as_str().unwrap())
        .collect();
    assert_eq!(worktrees, vec![wt.to_str().unwrap()], "{entry:#}");
    // Which files: the paths of the commit, with their real total.
    assert_eq!(paths(entry), vec!["login.rs", "util.rs"]);
    assert_eq!(entry["files"]["total"], 2);
    assert_eq!(entry["files"]["first_parent"], false, "{entry:#}");
    // Never a human: nobody was detected or registered.
    assert_eq!(entry["actor"]["actor"], "unattributed", "{entry:#}");
    assert_eq!(entry["attribution"], "current", "{entry:#}");
}

/// Escenario 3: what is not attributed is never "human", on the wire or in
/// the actor of any entry.
#[test]
fn nothing_is_ever_attributed_to_a_human() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    r.commit(&wt, &files_of(&["one.rs"]), "one");
    r.commit(&wt, &files_of(&["two.rs"]), "two");
    r.fx.git_in(&wt, &["reset", "-q", "--hard", "HEAD~1"]);

    let timeline = r.timeline(&wt);
    let all = entries(&timeline);
    assert!(all.len() >= 2, "{timeline:#}");
    for entry in all {
        let who = entry["actor"]["actor"].as_str().unwrap();
        assert!(matches!(who, "agent" | "unattributed"), "{entry:#}");
    }
    let wire = serde_json::to_string(&timeline).unwrap().to_lowercase();
    assert!(!wire.contains("human"), "{wire}");
    assert!(!wire.contains("humano"), "{wire}");
}

/// Escenario 4: an event with no recoverable point before it is shown
/// "unprotected" (level `none`), never as protected.
#[test]
fn an_event_without_a_point_is_shown_unprotected() {
    let (fx, wt) = repo_with_login();
    // Every capture fails, so no point exists for the worktree.
    let fail: CaptureLayer =
        Arc::new(|_| Some(CaptureError::Io(std::io::Error::from_raw_os_error(28))));
    let r = start(fx, Some(fail));
    std::fs::write(wt.join("api.rs"), "fn api() { login(); }\n").unwrap();
    r.logged("tm_capture_failed");
    let event = r.commit(&wt, &[], "api");

    let timeline = r.timeline(&wt);
    let entry = event_entry(&timeline, seq_of(&event));
    assert_eq!(entry["protection"]["level"], "none", "{entry:#}");
    assert!(
        entry["protection"]["snapshot_id"].is_null(),
        "no point may be named: {entry:#}"
    );
}

/// Escenario 5: nothing happened yet, and both sources could be read: an
/// empty timeline, with nothing declared unavailable.
#[test]
fn an_empty_repo_gives_an_empty_timeline_with_sources_available() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let timeline = r.timeline(&wt);
    assert_eq!(timeline["entries"], json!([]), "{timeline:#}");
    assert_eq!(timeline["unavailable"], json!([]), "{timeline:#}");
    assert_eq!(timeline["truncated"], false, "{timeline:#}");
    assert!(timeline["detection_available"].is_boolean(), "{timeline:#}");
}

/// Tope de rutas: a commit that changed 25 files shows 20 paths and the real
/// total (the CLI prints "+5 more"); no content and no message.
#[test]
fn files_are_capped_and_the_real_total_is_reported() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let event = r.commit(&wt, &numbered("f", 25), "many files");

    let timeline = r.timeline(&wt);
    let entry = event_entry(&timeline, seq_of(&event));
    let shown = paths(entry);
    let expected: Vec<String> = (0..20).map(|i| format!("f{i:02}.txt")).collect();
    assert_eq!(shown, expected);
    assert_eq!(entry["files"]["total"], 25, "{entry:#}");
}

/// D12: more than 20 paths is never cut in silence: the paths are capped at
/// 20 and the total says there are more (one over the cap is enough).
#[test]
fn a_commit_touching_more_than_20_paths_is_capped_and_marked_partial_never_silently() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let at_cap = r.commit(&wt, &numbered("a", 20), "exactly twenty");
    let over_cap = r.commit(&wt, &numbered("b", 21), "twenty one");

    let timeline = r.timeline(&wt);
    let exact = event_entry(&timeline, seq_of(&at_cap));
    assert_eq!(paths(exact).len(), 20);
    assert_eq!(exact["files"]["total"], 20, "no false partial: {exact:#}");

    let over = event_entry(&timeline, seq_of(&over_cap));
    assert_eq!(paths(over).len(), 20, "{over:#}");
    let total = over["files"]["total"].as_u64().unwrap();
    assert_eq!(total, 21, "the cut must be visible in the total: {over:#}");
    assert!(total > paths(over).len() as u64);
}

/// D10: a merge commit shows the diff against its first parent, not the
/// files that came in only through the second one.
#[test]
fn a_merge_commit_uses_first_parent_diff() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    // `side` forks from here and adds side.rs; the branch adds main.rs; the
    // merge brings side.rs into the branch.
    r.fx.git_in(&wt, &["checkout", "-q", "-b", "side"]);
    r.commit(&wt, &files_of(&["side.rs"]), "side");
    r.fx.git_in(&wt, &["checkout", "-q", "feat-login"]);
    r.commit(&wt, &files_of(&["main.rs"]), "main");
    r.fx.git_in(&wt, &["merge", "--no-ff", "-q", "-m", "merge side", "side"]);
    let event = r.event_at_head(&wt);

    let timeline = r.timeline(&wt);
    let entry = event_entry(&timeline, seq_of(&event));
    assert_eq!(entry["origin"]["kind"], "merge", "{entry:#}");
    // First parent: what the branch gained with the merge, not what it
    // already had (main.rs).
    assert_eq!(paths(entry), vec!["side.rs"], "{entry:#}");
    assert_eq!(entry["files"]["total"], 1, "{entry:#}");
    // The output says so: the paths are against the first parent.
    assert_eq!(entry["files"]["first_parent"], true, "{entry:#}");
}

// ----- Undo -----------------------------------------------------------------------

/// Entrada de un undo: the undo is an entry with the requester as recorded
/// and what it acted on; the raw event it undid is its target.
#[test]
fn an_undo_is_an_entry_with_what_it_acted_on() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let edited = "fn api() { login(); }\n";
    std::fs::write(wt.join("api.rs"), edited).unwrap();
    r.captured(KEY, "api.rs", edited.as_bytes());
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    let undo = r.undo(&wt);
    let operation_id = undo["operation_id"].as_str().unwrap().to_owned();
    let undone_seq: i64 = undo["undone_operation_id"]
        .as_str()
        .unwrap()
        .trim_start_matches("git-event-")
        .parse()
        .unwrap();

    let timeline = r.timeline(&wt);
    let entry = entries(&timeline)
        .iter()
        .find(|e| e["id"] == format!("operation:{operation_id}"))
        .unwrap_or_else(|| panic!("no entry for the undo: {timeline:#}"));
    assert_eq!(entry["origin"]["entry"], "operation", "{entry:#}");
    assert_eq!(entry["origin"]["kind"], "undo", "{entry:#}");
    assert_eq!(entry["origin"]["state"], "finished", "{entry:#}");
    assert_eq!(entry["origin"]["operation_id"], operation_id.as_str());
    // What it acted on: the raw reset's event.
    let acted_on = entry["origin"]["acted_on"].as_array().unwrap();
    assert!(
        acted_on
            .iter()
            .any(|a| a["target"] == "git-event" && a["id"] == undone_seq),
        "{acted_on:#?}"
    );
    // The requester as recorded (D-TMC-18), not the current attribution.
    assert_eq!(entry["attribution"], "recorded", "{entry:#}");
    assert_eq!(entry["actor"]["actor"], "unattributed", "{entry:#}");
    // The undo's own echo is not a second entry: the reset is one entry and
    // the undo another.
    let resets = entries(&timeline)
        .iter()
        .filter(|e| e["origin"]["entry"] == "git-event" && e["origin"]["kind"] == "reset")
        .count();
    assert_eq!(resets, 1, "{timeline:#}");
}

// ----- Privacy (Security) ----------------------------------------------------------

const SENTINEL_MESSAGE: &str = "SENTINEL-MESSAGE-7f3a91c2";
const SENTINEL_CONTENT: &str = "SENTINEL-CONTENT-9c1d44be";

/// No commit message and no file content leaves the daemon: neither in the
/// response nor in its log.
#[test]
fn no_commit_message_or_file_content_leaves_the_daemon() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let event = r.commit(
        &wt,
        &[("secret.rs".to_owned(), format!("{SENTINEL_CONTENT}\n"))],
        SENTINEL_MESSAGE,
    );

    let timeline = r.timeline(&wt);
    // Not vacuous: the commit is there, with its path.
    assert_eq!(paths(event_entry(&timeline, seq_of(&event))), ["secret.rs"]);
    let wire = serde_json::to_string(&timeline).unwrap();
    assert!(!wire.contains(SENTINEL_MESSAGE), "{wire}");
    assert!(!wire.contains(SENTINEL_CONTENT), "{wire}");
    let log = r.log();
    assert!(!log.contains(SENTINEL_MESSAGE), "message in the log");
    assert!(!log.contains(SENTINEL_CONTENT), "content in the log");
    assert!(!log.contains("secret.rs"), "paths do not enter the log");
}

// ----- Protection ---------------------------------------------------------------------

/// Escenario 4: what GitRaptor itself did shows the snapshot it took before (level
/// `guaranteed-prior`, the point a redo returns to).
#[test]
fn a_gitraptor_operation_shows_its_prior_snapshot_level() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let edited = "fn api() { login(); }\n";
    std::fs::write(wt.join("api.rs"), edited).unwrap();
    r.captured(KEY, "api.rs", edited.as_bytes());
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    let undo = r.undo(&wt);

    let timeline = r.timeline(&wt);
    let id = format!("operation:{}", undo["operation_id"].as_str().unwrap());
    let entry = entries(&timeline).iter().find(|e| e["id"] == id).unwrap();
    assert_eq!(
        entry["protection"]["level"], "guaranteed-prior",
        "{entry:#}"
    );
    assert_eq!(
        entry["protection"]["snapshot_id"],
        undo["prior_snapshot_id"]
    );
    // An operation's files are not read here: never a made-up zero.
    assert_eq!(entry["files"]["state"], "unavailable", "{entry:#}");
}

/// Escenario 4: a raw Git change shows the observation taken before it.
#[test]
fn a_raw_git_change_shows_observation() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let edited = "fn api() { login(); }\n";
    std::fs::write(wt.join("api.rs"), edited).unwrap();
    let point = r.captured(KEY, "api.rs", edited.as_bytes());
    let event = r.commit(&wt, &[], "api");

    let timeline = r.timeline(&wt);
    let entry = event_entry(&timeline, seq_of(&event));
    assert_eq!(entry["protection"]["level"], "observation", "{entry:#}");
    let shown = entry["protection"]["snapshot_id"].as_str().unwrap();
    // The point holds the work before the commit, whichever capture it was.
    let store = r.store().unwrap();
    assert!(
        shown == point
            || r.files(&store, shown, KEY)
                .iter()
                .any(|(p, b)| p == "api.rs" && b == edited.as_bytes()),
        "{entry:#}"
    );
}

/// ADR-TMC-003 § 4: the protection of an event is what an undo of it returns to.
#[test]
fn the_protection_of_an_event_is_the_undo_target() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let edited = "fn api() { login(); }\n";
    std::fs::write(wt.join("api.rs"), edited).unwrap();
    r.captured(KEY, "api.rs", edited.as_bytes());
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    let reset = r.event_of_kind("reset", &[]);
    let before = r.timeline(&wt);
    let protection = event_entry(&before, seq_of(&reset))["protection"].clone();

    let undo = r.undo(&wt);
    assert_eq!(protection["snapshot_id"], undo["target_snapshot_id"]);
    assert_ne!(protection["level"], "none", "{protection:#}");
}

/// The echo of an operation of GitRaptor is the operation, not a second entry: a raw reset
/// and its undo are two entries, nothing else.
#[test]
fn the_echo_of_a_gitraptor_operation_is_not_a_second_entry() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let edited = "fn api() { login(); }\n";
    std::fs::write(wt.join("api.rs"), edited).unwrap();
    r.captured(KEY, "api.rs", edited.as_bytes());
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    let undo = r.undo(&wt);

    let timeline = r.timeline(&wt);
    let ids: Vec<&str> = entries(&timeline)
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2, "{timeline:#}");
    assert!(ids[0].starts_with("event:"), "{ids:?}");
    assert_eq!(
        ids[1],
        format!("operation:{}", undo["operation_id"].as_str().unwrap())
    );
}

// ----- Files, filters and sources -------------------------------------------------------

/// D2: commits that cannot be read give "unavailable", never a zero.
#[test]
fn files_are_unavailable_when_commits_cannot_be_read() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let event = r.commit(&wt, &files_of(&["lost.rs"]), "lost");
    let new = event["details"]["new_commit"].as_str().unwrap().to_owned();
    // Loose object of the commit: gone, as after a damaged repo.
    let loose =
        r.fx.repo
            .join(".git/objects")
            .join(&new[..2])
            .join(&new[2..]);
    std::fs::remove_file(&loose).unwrap();

    let timeline = r.timeline(&wt);
    let entry = event_entry(&timeline, seq_of(&event));
    assert_eq!(
        entry["files"],
        json!({ "state": "unavailable" }),
        "{entry:#}"
    );
    // The rest of the entry is there: when, who, protection.
    assert_eq!(entry["origin"]["kind"], "commit");
    assert!(entry["occurred_utc_ms"].is_i64());
}

/// `only_worktree`, `since` and `agent` narrow the answer; the whole repo is the default.
#[test]
fn the_filters_narrow_by_worktree_period_and_agent() {
    let (fx, wt) = repo_with_login();
    fx.git(&["branch", "feat-pagos"]);
    let wt2 = canonical(&fx.add_worktree("feat-pagos", "feat-pagos"));
    let r = start(fx, None);
    let login = r.commit(&wt, &files_of(&["login.rs"]), "login");
    let pagos = r.commit(&wt2, &files_of(&["pagos.rs"]), "pagos");
    let wt_arg = |w: &Path| w.to_str().unwrap().to_owned();
    let ids_of = |t: &Value| -> Vec<String> {
        entries(t)
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let login_id = format!("event:{}", seq_of(&login));
    let pagos_id = format!("event:{}", seq_of(&pagos));

    // The whole repo from either anchor.
    let whole = r.timeline(&wt);
    assert!(ids_of(&whole).contains(&login_id) && ids_of(&whole).contains(&pagos_id));
    assert_eq!(ids_of(&r.timeline(&wt2)), ids_of(&whole));
    // One worktree.
    let only = r.timeline_with(json!({ "worktree": wt_arg(&wt), "only_worktree": wt_arg(&wt2) }));
    assert_eq!(ids_of(&only), vec![pagos_id.clone()]);
    // Nobody was detected: unattributed keeps all, a named agent none.
    let un = r.timeline_with(json!({ "worktree": wt_arg(&wt), "agent": "unattributed" }));
    assert_eq!(ids_of(&un), ids_of(&whole));
    let named = r.timeline_with(json!({ "worktree": wt_arg(&wt), "agent": "claude-code" }));
    assert_eq!(ids_of(&named), Vec::<String>::new());
    // A period.
    let since = r.timeline_with(json!({ "worktree": wt_arg(&wt), "since": "1h" }));
    assert_eq!(ids_of(&since), ids_of(&whole));
    // Bad parameters are refused before anything is read.
    for bad in [
        json!({ "since": "0m" }),
        json!({ "agent": "a b" }),
        json!({ "limit": 0 }),
        json!({ "limit": 201 }),
    ] {
        let mut params = bad.clone();
        params["worktree"] = json!(wt_arg(&wt));
        match r.timeline_raw(params) {
            Err(ClientError::Rpc(e)) => assert_eq!(e.code, code::INVALID_PARAMS, "{bad}"),
            other => panic!("{bad}: {other:?}"),
        }
    }
}

/// `limit` keeps the latest entries, still oldest first, and says it cut.
#[test]
fn limit_keeps_the_latest_entries_in_order() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let first = r.commit(&wt, &files_of(&["one.rs"]), "one");
    let second = r.commit(&wt, &files_of(&["two.rs"]), "two");
    let third = r.commit(&wt, &files_of(&["three.rs"]), "three");
    let _ = first;

    let t = r.timeline_with(json!({ "worktree": wt.to_str().unwrap(), "limit": 2 }));
    let ids: Vec<&str> = entries(&t)
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            format!("event:{}", seq_of(&second)),
            format!("event:{}", seq_of(&third))
        ]
    );
    assert_eq!(t["truncated"], true);
}

/// A folder no repo observes is refused, as an anchor and as `only_worktree`.
#[test]
fn an_unobserved_folder_is_refused() {
    let (fx, wt) = repo_with_login();
    let outside = canonical(&fx.other_repo);
    let r = start(fx, None);
    for params in [
        json!({ "worktree": outside.to_str().unwrap() }),
        json!({ "worktree": wt.to_str().unwrap(), "only_worktree": outside.to_str().unwrap() }),
    ] {
        match r.timeline_raw(params.clone()) {
            Err(ClientError::Rpc(e)) => assert_eq!(e.code, code::SCOPE_REFUSED, "{params}"),
            other => panic!("{params}: {other:?}"),
        }
    }
}

/// A client of a protocol that predates `events.git-reset` is not sent `reset` entries.
#[test]
fn a_reset_entry_is_not_sent_without_the_capability() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, None);
    let edited = "fn api() { login(); }\n";
    std::fs::write(wt.join("api.rs"), edited).unwrap();
    r.captured(KEY, "api.rs", edited.as_bytes());
    r.fx.git_in(&wt, &["reset", "-q", "--hard"]);
    let reset = r.event_of_kind("reset", &[]);
    let params = json!({ "worktree": wt.to_str().unwrap() });

    let current = r.timeline(&wt);
    event_entry(&current, seq_of(&reset));
    let mut older = Client::connect(&r.tp.dirs(), ClientKind::Cli, 7).unwrap();
    let t: Value = older.call(TIMELINE, params).unwrap();
    let kinds: Vec<&Value> = entries(&t).iter().map(|e| &e["origin"]["kind"]).collect();
    assert!(!kinds.contains(&&json!("reset")), "{t:#}");
}
