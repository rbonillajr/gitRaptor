//! The full `mcp.status` end to end (US-MCP-004): what an agent in a worktree is told about
//! itself and the repo, the pages behind a cursor, and the refusals of a repo or worktree that
//! cannot be read, with a real daemon (in-process) and the real client library in another
//! process whose working folder is the worktree, over a temporary profile, a temporary home and
//! testkit repos (NFR-01).
//!
//! No fixed waits: the daemon is waited for by its own snapshot, the client by a signal file or
//! its answers; a deadline is only a ceiling.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::capability;
use gitraptor_api::messages::{ClientKind, RepoView, SessionStateView, WorktreeStatus};
use gitraptor_api::methods::{
    self, CAP_MCP_STATUS_FULL, MCP_UNAVAILABLE, McpBaseState, McpProtectionState, McpStatus,
    McpStatusAction, valid_cursor,
};
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_api::{Actor, PROTOCOL_VERSION};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{Agent, AgentKind, GapCause, Origin, ProfileDirs, WriteOp};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(60);
const CLIENT_ENV: &str = "RAPTOR_TEST_MCP_CLIENT";

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
/// `[method, params]` or one of `["#wait", file]` (waits for the file to exist), `["#signal",
/// file]` (creates it) and `["#cd", folder]`. A string param `"$N:/pointer"` is the value at
/// `/pointer` of the answer to call number N. The answers go to `out` at the end. As a normal
/// test it does nothing.
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
                let (answer, ok) = match client.call::<_, Value>(method, params) {
                    Ok(v) => (json!({ "ok": v }), v),
                    Err(ClientError::Rpc(e)) => {
                        (json!({ "code": e.code, "data": e.data }), Value::Null)
                    }
                    Err(e) => (json!({ "other": e.to_string() }), Value::Null),
                };
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

/// The file `name` appears in the profile's folder: a signal of the client or for it.
fn signal_file(r: &Running, name: &str) -> PathBuf {
    r.tp.root.path().join(name)
}

fn wait_for_file(file: &Path) {
    let start = Instant::now();
    while !file.exists() {
        assert!(
            start.elapsed() < DEADLINE,
            "{} never appeared",
            file.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ----- The daemon, in-process -----------------------------------------------------------------

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    repo_id: String,
    spawned: AtomicUsize,
}

/// What a daemon starts with: the repo, whether the MCP allowlist has it, what is planted in its
/// store before the start, a second observed repo (in the allowlist or not) and the capabilities
/// it serves.
struct Setup<'a> {
    fx: &'a Fixture,
    enabled: bool,
    plant: Vec<WriteOp>,
    second: Option<&'a Path>,
    second_enabled: bool,
    capabilities: Option<Vec<&'static str>>,
}

impl<'a> Setup<'a> {
    fn new(fx: &'a Fixture) -> Self {
        Self {
            fx,
            enabled: true,
            plant: Vec::new(),
            second: None,
            second_enabled: false,
            capabilities: None,
        }
    }

    fn start(self) -> Running {
        let tp = TempProfile::new();
        let repo_id;
        {
            let mut profile = tp.open();
            let add = |profile: &mut gitraptor_core::profile::Profile, repo: &Path| {
                profile
                    .add_repo(&canonical(&repo.join(".git")), None, 1)
                    .unwrap()
                    .0
                    .repo_id
            };
            repo_id = add(&mut profile, &self.fx.repo);
            if self.enabled {
                profile.set_mcp_enabled(&repo_id, true, "test", 2).unwrap();
            }
            if !self.plant.is_empty() {
                let (mut store, _) = profile.open_store(&repo_id).unwrap();
                store.write_batch(&self.plant).unwrap();
            }
            if let Some(second) = self.second {
                let second_id = add(&mut profile, second);
                if self.second_enabled {
                    profile
                        .set_mcp_enabled(&second_id, true, "test", 2)
                        .unwrap();
                }
            }
        }
        // The daemon's `HOME` is the fixture's: a temporary folder with its own Git config.
        let vars = std::env::vars_os()
            .filter(|(key, _)| key != "HOME")
            .chain([("HOME".into(), self.fx.home.clone().into_os_string())]);
        let env = DaemonEnv::from_vars(vars);
        let mut channel = ChannelConfig::default();
        if let Some(capabilities) = self.capabilities {
            channel.capabilities = capabilities;
        }
        let config = DaemonConfig {
            dirs: tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel,
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
        Running {
            tp,
            handle,
            join: Some(join),
            repo_id,
            spawned: AtomicUsize::new(0),
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

impl Running {
    fn connect(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(ClientError::NotRunning) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// Until the repo as the daemon publishes it satisfies `ready`.
    fn wait_repo(&self, ready: impl Fn(&RepoView) -> bool) {
        let mut c = self.connect();
        let start = Instant::now();
        loop {
            let snap: ScopeSnapshot = c
                .call(
                    methods::SCOPE_SNAPSHOT,
                    json!({"scope": Scope::Repo { repo_id: self.repo_id.clone() }}),
                )
                .unwrap();
            if let ScopeSnapshot::Repo(s) = &snap
                && ready(&s.repo)
            {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "never ready: {snap:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// An `mcp` client process with its working folder in `cwd`.
    fn spawn_client(&self, cwd: &Path, calls: Value) -> ClientProc {
        let n = self.spawned.fetch_add(1, Ordering::SeqCst);
        let out = signal_file(self, &format!("answers-{n}.json"));
        let spec = json!({
            "root": self.tp.root.path().join("profile"),
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

    fn run_client(&self, cwd: &Path, calls: Value) -> Vec<Value> {
        self.spawn_client(cwd, calls).answers()
    }
}

// ----- The repos ------------------------------------------------------------------------------

/// The changes a worktree is published with, by root.
fn changes_of(repo: &RepoView, root: &Path) -> Option<u64> {
    repo.worktrees
        .iter()
        .find(|w| Path::new(w.path.raw()) == root)
        .and_then(|w| match &w.status {
            WorktreeStatus::Ready { counts, .. } => Some(counts.total()),
            WorktreeStatus::Unavailable { .. } => None,
        })
}

struct Scenario {
    fx: Fixture,
    wt_b: PathBuf,
    wt_c: PathBuf,
}

/// `main` with a worktree `wt-b` on `feat-b` (two commits ahead, four changes) and `wt-c` on
/// `feat-c`; the main worktree has three changes.
fn scenario() -> Scenario {
    let fx = Fixture::with_commit(&git());
    fx.write("m1.txt", "m\n");
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "m1"]);
    fx.git(&["branch", "feat-b"]);
    fx.git(&["branch", "feat-c"]);
    let wt_b = fx.add_worktree("b", "feat-b");
    let wt_c = fx.add_worktree("c", "feat-c");
    for n in 1..=2 {
        std::fs::write(wt_b.join(format!("b{n}.txt")), "b\n").unwrap();
        fx.git_in(&wt_b, &["add", "."]);
        fx.git_in(&wt_b, &["commit", "-q", "-m", &format!("b{n}")]);
    }
    std::fs::write(wt_b.join("a.txt"), "alpha, changed in b\n").unwrap();
    std::fs::write(wt_b.join("b.txt"), "beta, changed in b\n").unwrap();
    std::fs::write(wt_b.join("u1.txt"), "u\n").unwrap();
    std::fs::write(wt_b.join("u2.txt"), "u\n").unwrap();
    std::fs::write(fx.repo.join("a.txt"), "alpha, changed in main\n").unwrap();
    std::fs::write(fx.repo.join("x1.txt"), "x\n").unwrap();
    std::fs::write(fx.repo.join("x2.txt"), "x\n").unwrap();
    Scenario { fx, wt_b, wt_c }
}

impl Scenario {
    fn wait_ready(&self, r: &Running) {
        let (main, b) = (canonical(&self.fx.repo), canonical(&self.wt_b));
        r.wait_repo(|repo| {
            repo.worktrees.len() == 3
                && changes_of(repo, &main) == Some(3)
                && changes_of(repo, &b) == Some(4)
        });
    }

    /// The `.git` file of `wt-b` pointing somewhere else than its repo: an agent can write it
    /// (SEC-11).
    fn break_the_link_of_b(&self) {
        std::fs::write(self.wt_b.join(".git"), "gitdir: /nonexistent/elsewhere\n").unwrap();
    }
}

/// A repo with one commit and nothing else.
fn plain() -> Fixture {
    Fixture::with_commit(&git())
}

/// A registered agent session, which stays active across the daemon's start.
fn session(id: &str, worktree: &Path, name: &str) -> Vec<WriteOp> {
    vec![
        WriteOp::UpsertWorktree {
            path: worktree.to_owned(),
            admin_name: worktree
                .file_name()
                .filter(|_| worktree.join(".git").is_file())
                .map(|n| n.to_string_lossy().into_owned()),
            seen_ms: 1,
        },
        WriteOp::StartSession {
            session_id: id.to_owned(),
            worktree: worktree.to_owned(),
            agent: Agent {
                kind: AgentKind::Other,
                name: Some(name.to_owned()),
            },
            origin: Origin::Registered,
            detection_key: None,
            started_ms: now_ms() - 60_000,
        },
    ]
}

// ----- What the answers say -------------------------------------------------------------------

fn status(answer: &Value) -> McpStatus {
    serde_json::from_value(answer["ok"].clone())
        .unwrap_or_else(|e| panic!("not a status ({e}): {answer}"))
}

/// The code and the data of an answer that is an error.
fn error(answer: &Value) -> (i64, Option<Value>) {
    let code = answer["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("not an error: {answer}"));
    (code, answer.get("data").filter(|d| !d.is_null()).cloned())
}

fn actor_name(actor: &Actor) -> Option<&str> {
    match actor {
        Actor::Agent {
            name: Some(name), ..
        } => Some(name.raw()),
        _ => None,
    }
}

/// An answer that is a refusal carries nothing of the repo: none of `secrets` is in it.
fn assert_no_data(answer: &Value, secrets: &[String]) {
    let text = answer.to_string();
    for secret in secrets {
        assert!(!text.contains(secret.as_str()), "{secret} in {text}");
    }
}

fn secrets_of(s: &Scenario, repo_id: &str) -> Vec<String> {
    let mut secrets: Vec<String> = [&s.fx.repo, &s.wt_b, &s.wt_c, &s.fx.root]
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    secrets.extend(["wt-b", "wt-c", "feat-b", "feat-c"].map(str::to_owned));
    secrets.push(repo_id.to_owned());
    secrets
}

// ----- Tests ----------------------------------------------------------------------------------

/// BR-MCP-CALC-003: the default answer tells the caller's worktree and the repo; what is a list
/// is a `{total, cursor}`; the other worktrees and the paths of one come in pages.
#[test]
fn status_declares_the_whole_picture() {
    let s = scenario();
    let mut plant = session("s1", &canonical(&s.fx.repo), "claude-1");
    plant.extend(session("s2", &canonical(&s.wt_b), "claude-2"));
    let mut setup = Setup::new(&s.fx);
    setup.plant = plant;
    let r = setup.start();
    s.wait_ready(&r);

    let a = r.run_client(
        &canonical(&s.fx.repo),
        json!([
            ["mcp.status", {}],
            ["mcp.status", {"cursor": "$0:/repo/worktrees/cursor"}],
            ["mcp.status", {"cursor": "$0:/here/changes/cursor"}],
        ]),
    );

    let first = status(&a[0]);
    assert_eq!(first.worktree.raw(), "repo");
    assert_eq!(first.branch.as_ref().map(|b| b.raw()), Some("main"));
    assert!(first.main);
    assert_eq!(first.requester, Actor::Unattributed);
    assert_eq!(first.action, Some(McpStatusAction::RegisterToWrite));
    assert!(first.page.is_none());

    let here = first.here.as_ref().expect("here");
    assert_eq!(here.sessions.len(), 1, "{here:?}");
    assert_eq!(actor_name(&here.sessions[0].actor), Some("claude-1"));
    assert_ne!(here.sessions[0].state, SessionStateView::Ended);
    let changes = here.changes.as_ref().expect("the changes of the caller");
    assert_eq!(changes.total, 3);
    assert!(valid_cursor(&changes.cursor), "{}", changes.cursor);
    assert_eq!(
        (here.ahead, here.behind, here.uncounted),
        (None, None, None)
    );

    let repo = first.repo.as_ref().expect("repo");
    assert_eq!(repo.base.name.as_ref().map(|n| n.raw()), Some("main"));
    assert_eq!(repo.base.state, McpBaseState::Unconfirmed);
    assert_eq!(repo.protection, McpProtectionState::McpOnly);
    assert_eq!(repo.protection_lost, None);
    assert_eq!(repo.engine, None, "an observing engine is not said");
    assert_eq!(repo.fetch_age_s, None, "never fetched");
    assert!(repo.gaps.is_empty() && !repo.sessions_unknown);
    let others = repo.worktrees.as_ref().expect("the other worktrees");
    assert_eq!(others.total, 2);
    assert!(valid_cursor(&others.cursor));

    // The page of the other worktrees: each one with its branch, sessions, changes and ahead.
    let page = status(&a[1]).page.expect("a page of worktrees");
    assert_eq!(page.total, 2);
    assert!(!page.truncated && page.cursor.is_none() && page.paths.is_empty());
    let names: Vec<_> = page.worktrees.iter().map(|w| w.name.raw()).collect();
    assert_eq!(names, ["wt-b", "wt-c"]);
    let (b, c) = (&page.worktrees[0], &page.worktrees[1]);
    assert_eq!(b.branch.as_ref().map(|n| n.raw()), Some("feat-b"));
    assert!(!b.main && b.unavailable.is_none());
    assert_eq!(b.state.sessions.len(), 1, "{b:?}");
    assert_eq!(actor_name(&b.state.sessions[0].actor), Some("claude-2"));
    assert_eq!(b.state.changes.as_ref().map(|c| c.total), Some(4));
    assert!(valid_cursor(&b.state.changes.as_ref().unwrap().cursor));
    assert_eq!((b.state.ahead, b.state.behind), (Some(2), None));
    assert_eq!(c.branch.as_ref().map(|n| n.raw()), Some("feat-c"));
    assert!(c.state.sessions.is_empty() && c.state.changes.is_none());
    assert_eq!((c.state.ahead, c.state.behind), (None, None));

    // The paths of the caller's worktree.
    let paths = status(&a[2]).page.expect("a page of paths");
    assert_eq!(paths.of.as_ref().map(|n| n.raw()), Some("repo"));
    assert_eq!(paths.total, 3);
    let listed: Vec<_> = paths.paths.iter().map(|p| p.raw()).collect();
    assert_eq!(listed, ["a.txt", "x1.txt", "x2.txt"]);
    assert!(!paths.truncated && paths.cursor.is_none() && paths.worktrees.is_empty());
}

/// BR-MCP-EDGE-002: a base the developer did not confirm is declared with its state, and the
/// read is answered all the same.
#[test]
fn an_unconfirmed_base_is_declared_and_answered() {
    let fx = plain();
    let r = Setup::new(&fx).start();
    r.wait_repo(|repo| repo.worktrees.len() == 1);
    let a = r.run_client(&canonical(&fx.repo), json!([["mcp.status", {}]]));
    let answer = status(&a[0]);
    let repo = answer.repo.as_ref().expect("repo");
    assert_eq!(repo.base.name.as_ref().map(|n| n.raw()), Some("main"));
    assert_eq!(repo.base.state, McpBaseState::Unconfirmed);
    assert_eq!(answer.worktree.raw(), "repo");
    assert!(repo.worktrees.is_none(), "no other worktree, no list");
}

/// BR-MCP-CALC-003: a period the engine did not observe in the last 24 hours is declared with
/// when it began and ended, in seconds before the answer.
#[test]
fn an_observation_gap_is_declared() {
    let fx = plain();
    let planted = Instant::now();
    let now = now_ms();
    let mut setup = Setup::new(&fx);
    setup.plant = vec![
        WriteOp::OpenGap {
            gap_id: "g-test".into(),
            started_ms: now - 30 * 60_000,
            cause: GapCause::MachineOff,
            requested_by: None,
        },
        WriteOp::CloseGap {
            gap_id: "g-test".into(),
            ended_ms: now - 10 * 60_000,
        },
    ];
    let r = setup.start();
    r.wait_repo(|repo| repo.worktrees.len() == 1);
    let a = r.run_client(&canonical(&fx.repo), json!([["mcp.status", {}]]));
    let answer = status(&a[0]);
    let repo = answer.repo.as_ref().expect("repo");
    // The time that passed since the gap was planted is the most it can have moved.
    let slack = planted.elapsed().as_secs() + 5;
    let gap = repo
        .gaps
        .iter()
        .find(|g| (1800..=1800 + slack).contains(&g.from_s_ago))
        .unwrap_or_else(|| panic!("no gap of 30 minutes ago: {:?}", repo.gaps));
    let to = gap.to_s_ago.expect("a closed gap has its end");
    assert!((600..=600 + slack).contains(&to), "{gap:?}");
}

/// BR-MCP-CALC-002: 3000 modified files are a total and a cursor; each page of paths has at most
/// 32 unique paths, the total, `truncated` and a cursor that goes on exactly where it stopped.
#[test]
fn three_thousand_changes_page_with_a_cursor() {
    let fx = plain();
    let bulk = fx.repo.join("bulk");
    for i in 0..3000 {
        let dir = bulk.join(format!("d{:02}", i % 30));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("f{i:04}.txt")), "a\n").unwrap();
    }
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "bulk"]);
    // A different size, so Git sees every file modified whatever its timestamps.
    for i in 0..3000 {
        let dir = bulk.join(format!("d{:02}", i % 30));
        std::fs::write(dir.join(format!("f{i:04}.txt")), "bb\n").unwrap();
    }
    let r = Setup::new(&fx).start();
    let main = canonical(&fx.repo);
    r.wait_repo(|repo| changes_of(repo, &main) == Some(3000));

    let pages = 5;
    let mut calls = vec![
        json!(["mcp.status", {}]),
        json!(["mcp.status", {"cursor": "$0:/here/changes/cursor"}]),
    ];
    for n in 2..=pages {
        calls.push(json!(["mcp.status", {"cursor": format!("${}:/page/cursor", n - 1)}]));
    }
    let started = Instant::now();
    let a = r.run_client(&main, Value::Array(calls));
    eprintln!(
        "{pages} pages of paths of 3000 changes: {:?}",
        started.elapsed()
    );

    let first = status(&a[0]);
    let changes = first
        .here
        .as_ref()
        .and_then(|h| h.changes.as_ref())
        .unwrap();
    assert_eq!(changes.total, 3000);
    assert!(valid_cursor(&changes.cursor));

    let mut walked: Vec<String> = Vec::new();
    let mut cursors = vec![changes.cursor.clone()];
    for answer in &a[1..] {
        let page = status(answer).page.expect("a page of paths");
        assert_eq!(page.total, 3000);
        assert_eq!(page.of.as_ref().map(|n| n.raw()), Some("repo"));
        assert!(page.truncated, "there are 3000 paths and 32 per page");
        assert_eq!(page.paths.len(), 32);
        walked.extend(page.paths.iter().map(|p| p.raw().to_owned()));
        let cursor = page.cursor.expect("a cursor to go on");
        assert!(valid_cursor(&cursor));
        assert!(!cursors.contains(&cursor), "a cursor came back");
        cursors.push(cursor);
    }
    // Each page starts where the last one stopped: ascending and without repeats.
    // `a[1..]` holds one answer per page requested; together they are exactly the first paths of
    // the fixture, in order and without repeats.
    assert_eq!(walked.len(), 32 * (a.len() - 1));
    assert!(walked.windows(2).all(|w| w[0] < w[1]), "{walked:?}");
    let mut fixture: Vec<String> = (0..3000)
        .map(|i| format!("bulk/d{:02}/f{i:04}.txt", i % 30))
        .collect();
    fixture.sort();
    fixture.truncate(walked.len());
    assert_eq!(walked, fixture);
}

/// S-08: a cursor means something only on the connection that was given it and for its repo; any
/// other answers as if it did not exist, and a malformed one is a bad parameter.
#[test]
fn a_cursor_from_another_connection_does_not_exist() {
    let s = scenario();
    let other = &s.fx.other_repo;
    s.fx.git_in(other, &["commit", "-q", "--allow-empty", "-m", "x"]);
    let mut setup = Setup::new(&s.fx);
    setup.second = Some(other);
    setup.second_enabled = true;
    let r = setup.start();
    s.wait_ready(&r);

    let main = canonical(&s.fx.repo);
    let a = r.run_client(
        &main,
        json!([
            ["mcp.status", {}],
            ["mcp.status", {"cursor": "$0:/here/changes/cursor"}],
        ]),
    );
    let first = status(&a[0]);
    let cursor = first
        .here
        .as_ref()
        .and_then(|h| h.changes.clone())
        .unwrap()
        .cursor;
    // On its own connection it works.
    assert_eq!(status(&a[1]).page.expect("its own page").total, 3);

    // Another connection, the same folder and the same repo.
    let b = r.run_client(
        &main,
        json!([
            ["mcp.status", {"cursor": cursor}],
            ["mcp.status", {"cursor": "0123456789abcdef"}],
        ]),
    );
    for answer in &b {
        let (c, data) = error(answer);
        assert_eq!(c, code::NOT_FOUND, "{answer}");
        assert_eq!(data, None);
        assert_no_data(answer, &secrets_of(&s, &r.repo_id));
    }

    // The same connection after it moved to another repo, enabled as well.
    let moved = r.run_client(
        &main,
        json!([
            ["mcp.status", {}],
            ["#cd", canonical(other)],
            ["mcp.status", {"cursor": "$0:/here/changes/cursor"}],
        ]),
    );
    assert_eq!(status(&moved[0]).worktree.raw(), "repo");
    let (c, data) = error(&moved[1]);
    assert_eq!(c, code::NOT_FOUND, "{}", moved[1]);
    assert_eq!(data, None);
}

/// NFR-02: a cursor that has no form of a cursor is rejected as a bad parameter; one that has it
/// and is unknown does not exist.
#[test]
fn a_malformed_cursor_is_invalid_params() {
    let fx = plain();
    let r = Setup::new(&fx).start();
    r.wait_repo(|repo| repo.worktrees.len() == 1);
    let bad = [
        json!("xyz"),
        json!(""),
        json!("0123456789ABCDEF"),
        json!("0123456789abcde"),
        json!("0123456789abcdef0"),
        json!(1234567890123456u64),
        json!(["0123456789abcdef"]),
    ];
    let mut calls: Vec<Value> = bad
        .iter()
        .map(|cursor| json!(["mcp.status", {"cursor": cursor}]))
        .collect();
    calls.push(json!(["mcp.status", {"repo": "/elsewhere"}]));
    // Well formed, unknown: it does not exist.
    calls.push(json!(["mcp.status", {"cursor": "0123456789abcdef"}]));
    let a = r.run_client(&canonical(&fx.repo), Value::Array(calls));
    let (unknown, malformed) = a.split_last().unwrap();
    for answer in malformed {
        let (c, data) = error(answer);
        assert_eq!(c, code::INVALID_PARAMS, "{answer}");
        assert_eq!(data, None);
    }
    assert_eq!(error(unknown).0, code::NOT_FOUND, "{unknown}");
}

/// D9: a worktree deleted during the session is refused with its reason and nothing of the repo.
#[test]
fn a_worktree_deleted_during_the_session_is_missing() {
    let s = scenario();
    let r = Setup::new(&s.fx).start();
    s.wait_ready(&r);
    let (first, go) = (signal_file(&r, "first-done"), signal_file(&r, "go"));
    let client = r.spawn_client(
        &canonical(&s.wt_c),
        json!([
            ["mcp.status", {}],
            ["#signal", first],
            ["#wait", go],
            ["mcp.status", {}],
        ]),
    );
    wait_for_file(&first);
    std::fs::remove_dir_all(&s.wt_c).unwrap();
    std::fs::write(&go, b"").unwrap();
    let a = client.answers();

    assert_eq!(status(&a[0]).worktree.raw(), "wt-c");
    let (c, data) = error(&a[1]);
    assert_eq!(c, MCP_UNAVAILABLE.code, "{}", a[1]);
    assert_eq!(data, Some(json!({"reason": "worktree-missing"})));
    assert_no_data(&a[1], &secrets_of(&s, &r.repo_id));
}

/// SEC-11: a linked worktree whose `.git` no longer points back at its repo is refused with its
/// reason and nothing of the repo.
#[test]
fn a_worktree_that_fails_sec11_is_refused_without_data() {
    let s = scenario();
    s.break_the_link_of_b();
    let r = Setup::new(&s.fx).start();
    r.wait_repo(|repo| repo.worktrees.len() == 3);
    let a = r.run_client(&canonical(&s.wt_b), json!([["mcp.status", {}]]));
    let (c, data) = error(&a[0]);
    assert_eq!(c, MCP_UNAVAILABLE.code, "{}", a[0]);
    assert_eq!(data, Some(json!({"reason": "worktree-untrusted"})));
    assert_no_data(&a[0], &secrets_of(&s, &r.repo_id));
}

/// BR-MCP-EDGE-005: for a repo outside the allowlist the answer is "not enabled" and nothing
/// else, though the worktree could not be read either; once enabled, the same worktree is
/// unavailable.
#[test]
fn not_enabled_is_checked_before_unavailable() {
    let s = scenario();
    s.break_the_link_of_b();
    let b = canonical(&s.wt_b);
    {
        let mut setup = Setup::new(&s.fx);
        setup.enabled = false;
        let r = setup.start();
        r.wait_repo(|repo| repo.worktrees.len() == 3);
        let a = r.run_client(&b, json!([["mcp.status", {}]]));
        let (c, data) = error(&a[0]);
        assert_eq!(c, code::SCOPE_REFUSED, "{}", a[0]);
        assert_eq!(data, Some(json!({"reason": "not-allowlisted"})));
        assert_no_data(&a[0], &secrets_of(&s, &r.repo_id));
    }
    let r = Setup::new(&s.fx).start();
    r.wait_repo(|repo| repo.worktrees.len() == 3);
    let a = r.run_client(&b, json!([["mcp.status", {}]]));
    let (c, data) = error(&a[0]);
    assert_eq!(c, MCP_UNAVAILABLE.code, "{}", a[0]);
    assert_eq!(data, Some(json!({"reason": "worktree-untrusted"})));
}

/// Every file under `dir` with its content: the footprint of a home folder.
fn footprint(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let content = std::fs::read(&path).unwrap();
                found.insert(path.strip_prefix(dir).unwrap().to_owned(), content);
            }
        }
    }
    found
}

/// NFR-01: no refusal of `status` writes in the global Git config or anywhere in the daemon's
/// home, whatever it is told to do about a worktree it does not trust.
#[test]
fn refusals_write_nothing_in_the_global_git_config() {
    let s = scenario();
    s.break_the_link_of_b();
    let other = &s.fx.other_repo;
    s.fx.git_in(other, &["commit", "-q", "--allow-empty", "-m", "x"]);
    let mut setup = Setup::new(&s.fx);
    setup.second = Some(other);
    let r = setup.start();
    r.wait_repo(|repo| repo.worktrees.len() == 3);

    let before = footprint(&s.fx.home);
    assert!(before.contains_key(Path::new(".gitconfig")));
    let untrusted = r.run_client(&canonical(&s.wt_b), json!([["mcp.status", {}]]));
    let not_enabled = r.run_client(&canonical(other), json!([["mcp.status", {}]]));
    assert_eq!(
        error(&untrusted[0]).0,
        MCP_UNAVAILABLE.code,
        "{}",
        untrusted[0]
    );
    assert_eq!(error(&not_enabled[0]).0, code::SCOPE_REFUSED);
    assert_eq!(footprint(&s.fx.home), before);
}

/// D1: a connection without `mcp.status-full` gets the status of before, a cursor is a bad
/// parameter for it, and a worktree that cannot be read is "not observed", without data.
#[test]
fn without_the_capability_status_keeps_its_shape() {
    let s = scenario();
    s.break_the_link_of_b();
    let mut setup = Setup::new(&s.fx);
    setup.capabilities = Some(
        capability::all()
            .map(|c| c.name)
            .filter(|name| *name != CAP_MCP_STATUS_FULL.name)
            .collect(),
    );
    let r = setup.start();
    r.wait_repo(|repo| repo.worktrees.len() == 3);

    let a = r.run_client(
        &canonical(&s.fx.repo),
        json!([["mcp.status", {}], ["mcp.status", {"cursor": "0123456789abcdef"}]]),
    );
    let today = status(&a[0]);
    assert!(today.here.is_none() && today.repo.is_none() && today.page.is_none());
    assert_eq!(today.branch.as_ref().map(|b| b.raw()), Some("main"));
    let mut keys: Vec<_> = a[0]["ok"].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "action",
            "branch",
            "main",
            "repo_id",
            "repo_state",
            "requester",
            "worktree"
        ]
    );
    assert_eq!(error(&a[1]).0, code::INVALID_PARAMS, "{}", a[1]);

    let b = r.run_client(&canonical(&s.wt_b), json!([["mcp.status", {}]]));
    let (c, data) = error(&b[0]);
    assert_eq!(c, code::SCOPE_REFUSED, "{}", b[0]);
    assert_eq!(data, Some(json!({"reason": "not-observed"})));
    assert_no_data(&b[0], &secrets_of(&s, &r.repo_id));
}
