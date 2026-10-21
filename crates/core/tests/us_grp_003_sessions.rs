// US-GRP-003, scenario 3: the sessions of a worktree that Git removes, with a
// real daemon (in-process), a temporary profile and testkit repos (NFR-01).
// "Codex" is always registered (`registration.register`, US-GRP-009), never
// detected. No fixed waits: each step waits for the notification or answer
// that proves it, with a 10 s ceiling; a repo is polled with
// `TierTestOp::PollNow`.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::event::{REPO_OBSERVATION, SESSION_STATE};
use gitraptor_api::messages::{
    ClientKind, EventNotification, RepoObservationData, RepoStateView, SessionStateView,
    SessionView, SessionsListResult, UnavailableReason, WorktreeStatus,
};
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, TierTestOp,
};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(10);

/// `registration.register` takes the caller's working folder: the process
/// folder is borrowed one test at a time.
static CWD: Mutex<()> = Mutex::new(());

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<gitraptor_core::daemon::StopReport>>,
    repo_id: String,
}

fn launch(tp: &TempProfile) -> (ShutdownHandle, JoinHandle<gitraptor_core::daemon::StopReport>) {
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
    (handle, std::thread::spawn(move || daemon.run()))
}

impl Running {
    fn start(root: &Path) -> Self {
        let tp = TempProfile::new();
        let (entry, _) = tp
            .open()
            .add_repo(&canonical(&root.join(".git")), None, 1)
            .unwrap();
        let (handle, join) = launch(&tp);
        let r = Self {
            tp,
            handle,
            join: Some(join),
            repo_id: entry.repo_id,
        };
        r.wait_worktrees(2);
        r
    }

    fn stop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }

    fn restart(&mut self) {
        self.stop();
        let (handle, join) = launch(&self.tp);
        self.handle = handle;
        self.join = Some(join);
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

    fn subscribed(&self) -> Client {
        let mut c = self.client();
        let _: Value = c.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
        c
    }

    fn repo(&self, c: &mut Client) -> gitraptor_api::messages::RepoView {
        let scope = Scope::Repo {
            repo_id: self.repo_id.clone(),
        };
        match c
            .call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
            .unwrap()
        {
            ScopeSnapshot::Repo(s) => s.repo,
            other => panic!("{other:?}"),
        }
    }

    fn wait_until(&self, what: &str, mut done: impl FnMut(&mut Client) -> bool) {
        let mut c = self.client();
        let start = Instant::now();
        while !done(&mut c) {
            assert!(start.elapsed() < DEADLINE, "never: {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_worktrees(&self, n: usize) {
        self.wait_until("worktrees listed", |c| self.repo(c).worktrees.len() == n);
    }

    fn sessions(&self, c: &mut Client) -> Vec<SessionView> {
        let list: SessionsListResult = c
            .call(
                methods::SESSIONS_LIST,
                json!({"repo_id": self.repo_id, "include_ended": true}),
            )
            .unwrap();
        list.sessions
    }

    fn wait_session(&self) -> SessionView {
        let mut found = None;
        self.wait_until("the registered session", |c| {
            found = self.sessions(c).into_iter().next();
            found.is_some()
        });
        found.unwrap()
    }

    /// Whether the (only) session is still open now.
    fn is_open(&self) -> bool {
        let sessions = self.sessions(&mut self.client());
        assert_eq!(sessions.len(), 1, "{sessions:#?}");
        sessions[0].state != SessionStateView::Ended
    }

    /// Polls now until `repo.observation` carries `state`.
    fn poll_until(&self, c: &mut Client, state: RepoStateView) {
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
                    if d.repo_id == self.repo_id && d.state == state {
                        return;
                    }
                }
            }
        }
        panic!("no repo.observation with {state:?}");
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A repo with a linked worktree `feat`.
fn repo() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let feat = canonical(&fx.add_worktree("feat", "feat"));
    (fx, feat)
}

fn register_codex(c: &mut Client, folder: &Path) {
    let _folder = CWD.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(folder).unwrap();
    let result: Result<Value, _> = c.call(
        methods::REGISTRATION_REGISTER,
        json!({"agent": {"kind": "other", "name": "Codex"}}),
    );
    std::env::set_current_dir(previous).unwrap();
    result.expect("the registration");
}

fn cause(s: &SessionView) -> Value {
    serde_json::to_value(s.end_cause).unwrap()
}

fn remove(fx: &Fixture, feat: &Path) {
    fx.git(&["worktree", "remove", "--force", feat.to_str().unwrap()]);
}

#[test]
fn a_registered_agent_ends_when_its_worktree_is_removed() {
    let (fx, feat) = repo();
    let r = Running::start(&fx.repo);
    register_codex(&mut r.client(), &feat);
    let open = r.wait_session();
    assert_ne!(open.state, SessionStateView::Ended);
    let mut watcher = r.subscribed();

    remove(&fx, &feat);

    let start = Instant::now();
    let ended = loop {
        assert!(start.elapsed() < DEADLINE, "no session.state with ended");
        let Some(n) = watcher.next_notification(Duration::from_millis(200)).unwrap() else {
            continue;
        };
        if n.method != methods::NOTIFY_EVENT {
            continue;
        }
        let e: EventNotification = serde_json::from_value(n.params).unwrap();
        if e.event.kind == SESSION_STATE && e.event.data["state"] == "ended" {
            break e.event.data;
        }
    };
    assert_eq!(ended["session_id"], json!(open.session_id));
    assert_eq!(ended["end_cause"], "worktree-removed", "{ended}");
    assert!(ended["ended_utc_ms"].is_i64(), "a live end has its time: {ended}");
    let listed = r.sessions(&mut r.client());
    assert_eq!(cause(&listed[0]), "worktree-removed");
}

#[test]
fn a_missing_folder_keeps_its_session() {
    let (fx, feat) = repo();
    let r = Running::start(&fx.repo);
    register_codex(&mut r.client(), &feat);
    r.wait_session();

    std::fs::remove_dir_all(&feat).unwrap();
    r.wait_until("the folder is missing", |c| {
        r.repo(c).worktrees.iter().any(|w| {
            Path::new(w.path.raw()) == feat
                && w.status
                    == WorktreeStatus::Unavailable {
                        reason: UnavailableReason::Missing,
                    }
        })
    });
    // A later batch of the same repo has been processed: nothing ended.
    let root = canonical(&fx.repo);
    std::fs::write(root.join("later.txt"), "x\n").unwrap();
    r.wait_until("a later change", |c| {
        r.repo(c).worktrees.iter().any(|w| {
            Path::new(w.path.raw()) == root
                && matches!(&w.status, WorktreeStatus::Ready { changes, .. }
                    if changes.iter().any(|c| c.path.raw() == "later.txt"))
        })
    });

    assert!(r.is_open(), "a missing folder never ends a session");
}

#[test]
fn a_lost_repo_ends_no_sessions() {
    let (fx, feat) = repo();
    let r = Running::start(&fx.repo);
    register_codex(&mut r.client(), &feat);
    r.wait_session();
    let mut c = r.subscribed();
    let moved = fx.root.join("repo-moved");

    std::fs::rename(&fx.repo, &moved).unwrap();
    r.poll_until(&mut c, RepoStateView::Unavailable);
    assert!(r.is_open(), "a lost repo ends no session");

    std::fs::rename(&moved, &fx.repo).unwrap();
    r.poll_until(&mut c, RepoStateView::Observed);
    assert!(r.is_open(), "the session is still open when the repo is back");
}

#[test]
fn a_session_of_a_worktree_removed_while_stopped_ends_at_start() {
    let (fx, feat) = repo();
    let mut r = Running::start(&fx.repo);
    register_codex(&mut r.client(), &feat);
    r.wait_session();
    r.stop();

    remove(&fx, &feat);
    r.restart();

    r.wait_until("the session ends at start", |c| {
        r.sessions(c)
            .first()
            .is_some_and(|s| s.state == SessionStateView::Ended)
    });
    let s = r.sessions(&mut r.client()).remove(0);
    assert_eq!(cause(&s), "worktree-removed");
    assert_eq!(s.ended_utc_ms, None, "the end time is unknown");
}

// ------------------------------------------------------------ old client

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Raw {
    /// A connection that accepts no capability.
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

#[test]
fn an_old_client_sees_the_session_ended_without_a_cause() {
    let (fx, feat) = repo();
    let r = Running::start(&fx.repo);
    register_codex(&mut r.client(), &feat);
    r.wait_session();
    let mut old = Raw::open(&r);

    remove(&fx, &feat);

    // The same session, for a client that knows the cause.
    r.wait_until("the session ends", |c| {
        r.sessions(c)
            .first()
            .is_some_and(|s| s.state == SessionStateView::Ended)
    });
    assert_eq!(cause(&r.sessions(&mut r.client())[0]), "worktree-removed");

    let answer = old.call(
        1,
        methods::SESSIONS_LIST,
        json!({"repo_id": r.repo_id, "include_ended": true}),
    );
    let sessions = answer["result"]["sessions"].as_array().expect("sessions");
    assert_eq!(sessions.len(), 1, "{answer}");
    assert_eq!(sessions[0]["state"], "ended", "{answer}");
    assert!(
        sessions[0].get("end_cause").is_none(),
        "never process-gone nor any cause: {answer}"
    );
}
