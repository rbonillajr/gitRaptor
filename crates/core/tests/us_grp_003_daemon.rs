// US-GRP-003, scenarios 2, 4 and 5 and the wire without the capability, with a
// real daemon (in-process), the real client library, a temporary profile and
// testkit repos (NFR-01). No fixed waits: each step waits for the notification
// or snapshot that proves it, with a 10 s ceiling. A repo is "polled now" with
// `TierTestOp::PollNow` instead of waiting for the 30 s backup poll.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::event::REPO_OBSERVATION;
use gitraptor_api::messages::{
    ClientKind, EventNotification, EventsHistoryResult, GitEventKind, RepoObservationData,
    RepoStateView, RepoView, UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, TierTestOp,
};
use gitraptor_core::profile::GapCause;
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(10);

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<gitraptor_core::daemon::StopReport>>,
    ids: Vec<String>,
}

impl Running {
    /// A daemon observing the repos whose main worktrees are `roots`.
    fn start(roots: &[&Path]) -> Self {
        let tp = TempProfile::new();
        let ids = roots
            .iter()
            .map(|root| {
                let mut profile = tp.open();
                let (entry, _) = profile
                    .add_repo(&canonical(&root.join(".git")), None, 1)
                    .unwrap();
                entry.repo_id
            })
            .collect();
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
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        let r = Self {
            tp,
            handle,
            join: Some(join),
            ids,
        };
        for i in 0..r.ids.len() {
            r.wait_repo(i, |v| !v.worktrees.is_empty());
        }
        r
    }

    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Err(ClientError::NotRunning) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                other => return other.unwrap(),
            }
        }
    }

    fn view(&self, c: &mut Client, i: usize) -> RepoView {
        let scope = Scope::Repo {
            repo_id: self.ids[i].clone(),
        };
        match c
            .call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
            .unwrap()
        {
            ScopeSnapshot::Repo(s) => s.repo,
            other => panic!("{other:?}"),
        }
    }

    /// The signal: a snapshot of repo `i` that satisfies `done`.
    fn wait_repo(&self, i: usize, done: impl Fn(&RepoView) -> bool) -> RepoView {
        let mut c = self.client();
        let start = Instant::now();
        loop {
            let v = self.view(&mut c, i);
            if done(&v) {
                return v;
            }
            assert!(start.elapsed() < DEADLINE, "never reached: {v:#?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn history(&self, i: usize) -> EventsHistoryResult {
        self.client()
            .call(
                methods::EVENTS_HISTORY,
                json!({"repo_id": self.ids[i], "limit": 500}),
            )
            .unwrap()
    }

    /// Polls the repos now until `repo.observation` of repo `i` carries
    /// `state`; the events of the other kinds seen on the way are kept.
    fn poll_until_observation(
        &self,
        c: &mut Client,
        i: usize,
        state: RepoStateView,
        seen: &mut Vec<Value>,
    ) {
        let start = Instant::now();
        while start.elapsed() < DEADLINE {
            assert!(self.handle.tier_test(TierTestOp::PollNow));
            while let Some(n) = c.next_notification(Duration::from_millis(150)).unwrap() {
                if n.method != methods::NOTIFY_EVENT {
                    continue;
                }
                let e: EventNotification = serde_json::from_value(n.params).unwrap();
                if e.event.kind == REPO_OBSERVATION {
                    let d: RepoObservationData = serde_json::from_value(e.event.data).unwrap();
                    if d.repo_id == self.ids[i] && d.state == state {
                        assert!(d.observed, "an unavailable repo is still observed");
                        return;
                    }
                } else {
                    seen.push(json!({"kind": e.event.kind, "data": e.event.data}));
                }
            }
        }
        panic!("no repo.observation with {state:?}; other events: {seen:#?}");
    }

    fn stop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

fn subscribed(r: &Running) -> Client {
    let mut c = r.client();
    let _: Value = c.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    c
}

fn wt<'a>(v: &'a RepoView, path: &Path) -> Option<&'a WorktreeView> {
    v.worktrees.iter().find(|w| Path::new(w.path.raw()) == path)
}

fn shows(v: &RepoView, path: &Path, file: &str) -> bool {
    wt(v, path).is_some_and(|w| {
        matches!(&w.status, WorktreeStatus::Ready { changes, .. }
            if changes.iter().any(|c| c.path.raw() == file))
    })
}

/// A repo with `main` and nine linked worktrees (ten in all).
fn ten() -> (Fixture, Vec<PathBuf>) {
    let fx = Fixture::with_commit(&git());
    let mut linked = Vec::new();
    for n in 1..=9 {
        let branch = format!("feat-{n}");
        fx.git(&["branch", &branch]);
        linked.push(canonical(&fx.add_worktree(&branch, &branch)));
    }
    (fx, linked)
}

/// Every worktree but `except` changes and the engine publishes each change.
fn nine_keep_changing(r: &Running, fx: &Fixture, linked: &[PathBuf], except: &Path) {
    let mut roots: Vec<PathBuf> = linked.to_vec();
    roots.push(canonical(&fx.repo));
    roots.retain(|p| p != except);
    assert_eq!(roots.len(), 9);
    for (n, root) in roots.iter().enumerate() {
        std::fs::write(root.join(format!("still-{n}.txt")), "x\n").unwrap();
    }
    for (n, root) in roots.iter().enumerate() {
        let file = format!("still-{n}.txt");
        r.wait_repo(0, |v| shows(v, root, &file));
    }
}

#[test]
fn a_deleted_worktree_folder_is_missing_and_nine_keep_changing() {
    let (fx, linked) = ten();
    let r = Running::start(&[&fx.repo]);
    r.wait_repo(0, |v| v.worktrees.len() == 10);
    let gone = linked[0].clone();

    std::fs::remove_dir_all(&gone).unwrap();

    let v = r.wait_repo(0, |v| {
        wt(v, &gone).is_some_and(|w| {
            w.status
                == WorktreeStatus::Unavailable {
                    reason: UnavailableReason::Missing,
                }
        })
    });
    assert_eq!(v.worktrees.len(), 10, "Git still registers it");
    nine_keep_changing(&r, &fx, &linked, &gone);
}

#[test]
fn a_removed_worktree_leaves_the_list_and_nine_keep_changing() {
    let (fx, linked) = ten();
    let r = Running::start(&[&fx.repo]);
    r.wait_repo(0, |v| v.worktrees.len() == 10);
    let removed = linked[0].clone();

    fx.git(&["worktree", "remove", "--force", removed.to_str().unwrap()]);

    let v = r.wait_repo(0, |v| wt(v, &removed).is_none());
    assert_eq!(v.worktrees.len(), 9);
    nine_keep_changing(&r, &fx, &linked, &removed);
}

#[test]
fn a_removed_worktree_is_recorded_in_the_history() {
    let (fx, linked) = ten();
    let r = Running::start(&[&fx.repo]);
    r.wait_repo(0, |v| v.worktrees.len() == 10);
    let removed = linked[0].clone();

    fx.git(&["worktree", "remove", "--force", removed.to_str().unwrap()]);
    r.wait_repo(0, |v| wt(v, &removed).is_none());

    let start = Instant::now();
    loop {
        let found = r.history(0).events.into_iter().any(|e| {
            e.kind == GitEventKind::WorktreeDelete && Path::new(e.worktree.raw()) == removed
        });
        if found {
            return;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "no worktree-delete in the history"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// "demo" with a linked worktree `feat`, and "other" with a commit.
fn two_repos() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let feat = canonical(&fx.add_worktree("feat", "feat"));
    std::fs::write(fx.other_repo.join("o.txt"), "o\n").unwrap();
    fx.git_in(&fx.other_repo, &["add", "o.txt"]);
    fx.git_in(&fx.other_repo, &["commit", "-q", "-m", "o"]);
    (fx, feat)
}

fn moved(fx: &Fixture) -> PathBuf {
    fx.root.join("repo-moved")
}

#[test]
fn a_moved_repo_is_unavailable_and_the_other_keeps_going() {
    let (fx, _feat) = two_repos();
    let r = Running::start(&[&fx.repo, &fx.other_repo]);
    let mut c = subscribed(&r);

    std::fs::rename(&fx.repo, moved(&fx)).unwrap();
    r.poll_until_observation(&mut c, 0, RepoStateView::Unavailable, &mut Vec::new());

    let mut c = r.client();
    assert_eq!(r.view(&mut c, 0).state, RepoStateView::Unavailable);
    assert_eq!(r.view(&mut c, 1).state, RepoStateView::Observed);
    let other = canonical(&fx.other_repo);
    std::fs::write(other.join("later.txt"), "x\n").unwrap();
    r.wait_repo(1, |v| shows(v, &other, "later.txt"));
}

#[test]
fn a_lost_repo_emits_no_worktree_events() {
    let (fx, _feat) = two_repos();
    let r = Running::start(&[&fx.repo, &fx.other_repo]);
    let mut c = subscribed(&r);
    let mut seen = Vec::new();

    std::fs::rename(&fx.repo, moved(&fx)).unwrap();
    r.poll_until_observation(&mut c, 0, RepoStateView::Unavailable, &mut seen);
    std::fs::rename(moved(&fx), &fx.repo).unwrap();
    r.poll_until_observation(&mut c, 0, RepoStateView::Observed, &mut seen);

    let fake = |kind: &str| {
        seen.iter()
            .any(|e| e["kind"] == "git.event" && e["data"]["kind"] == kind)
    };
    assert!(!fake("worktree-delete"), "live: {seen:#?}");
    assert!(!fake("worktree-create"), "live: {seen:#?}");
    let history = r.history(0);
    for e in &history.events {
        assert!(
            !matches!(
                e.kind,
                GitEventKind::WorktreeDelete | GitEventKind::WorktreeCreate
            ),
            "{e:#?}"
        );
    }
    assert_eq!(
        r.wait_repo(0, |v| v.worktrees.len() == 2).worktrees.len(),
        2
    );
}

#[test]
fn a_repo_moved_back_is_observed_again() {
    let (fx, _feat) = two_repos();
    let mut r = Running::start(&[&fx.repo, &fx.other_repo]);
    let mut c = subscribed(&r);
    let lost_at = now_ms();

    std::fs::rename(&fx.repo, moved(&fx)).unwrap();
    r.poll_until_observation(&mut c, 0, RepoStateView::Unavailable, &mut Vec::new());
    std::fs::rename(moved(&fx), &fx.repo).unwrap();
    r.poll_until_observation(&mut c, 0, RepoStateView::Observed, &mut Vec::new());

    let root = canonical(&fx.repo);
    std::fs::write(root.join("after.txt"), "x\n").unwrap();
    r.wait_repo(0, |v| shows(v, &root, "after.txt"));

    r.stop();
    let id = r.ids[0].clone();
    let (store, _) = r.tp.open().open_store(&id).unwrap();
    let gaps = store.gaps().unwrap();
    assert!(
        gaps.iter()
            .any(|g| g.cause == GapCause::PeriodicReconciliation
                && g.ended_ms.is_some_and(|ended| ended >= lost_at)),
        "no periodic-reconciliation gap closed after the loss: {gaps:#?}"
    );
}

// ------------------------------------------------------------ wire, old client

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Raw {
    fn open(r: &Running) -> Self {
        let path = gitraptor_core::client::socket_path(&r.tp.dirs()).unwrap();
        let stream = UnixStream::connect(path).unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self { stream, reader };
        let hello = raw.call(
            0,
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        assert!(hello["result"].is_object(), "{hello}");
        raw
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

/// The worktrees of the first repo in the three snapshots.
fn snapshot_worktrees(c: &mut Raw, repo_id: &str) -> Vec<Vec<Value>> {
    let engine = c.call(1, methods::ENGINE_SNAPSHOT, json!({}));
    let scope = c.call(
        2,
        methods::SCOPE_SNAPSHOT,
        json!({"scope": {"scope": "repo", "repo_id": repo_id}}),
    );
    let repo_add = &engine["result"]["repos"][0]["worktrees"];
    let by_scope = &scope["result"]["repo"]["worktrees"];
    [repo_add, by_scope]
        .iter()
        .map(|w| {
            w.as_array()
                .unwrap_or_else(|| panic!("{engine} {scope}"))
                .clone()
        })
        .collect()
}

#[test]
fn without_the_capability_the_worktree_has_no_operation() {
    let fx = Fixture::new(&git());
    for name in ["a.txt", "b.txt"] {
        fx.write(name, "base\n");
    }
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "base"]);
    fx.git(&["checkout", "-q", "-b", "topic"]);
    for name in ["a.txt", "b.txt"] {
        fx.write(name, "topic\n");
    }
    fx.git(&["commit", "-q", "-am", "topic"]);
    fx.git(&["checkout", "-q", "main"]);
    for name in ["a.txt", "b.txt"] {
        fx.write(name, "main\n");
    }
    fx.git(&["commit", "-q", "-am", "main"]);
    let out = fx
        .git_command(&fx.repo, &["merge", "topic"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let marker = fx.repo.join(".git/MERGE_HEAD");

    let r = Running::start(&[&fx.repo]);
    let id = r.ids[0].clone();

    // With the capability (the library client accepts what it knows) the
    // merge is published.
    let mut c = r.client();
    let v = serde_json::to_value(r.view(&mut c, 0).worktrees[0].clone()).unwrap();
    assert_eq!(v["operation"], "merge", "{v}");
    assert_eq!(v["unmerged_paths"], 2, "{v}");

    // A raw connection accepts nothing: the wire is the one of today.
    let mut old = Raw::open(&r);
    for worktrees in snapshot_worktrees(&mut old, &id) {
        assert!(!worktrees.is_empty());
        for w in worktrees {
            assert!(w.get("operation").is_none(), "{w}");
            assert!(w.get("unmerged_paths").is_none(), "{w}");
        }
    }
    assert!(marker.is_file(), "the merge was touched");
}
