//! Hardening of the full `mcp.status`: a cursor to another worktree that went away is a cursor
//! that no longer exists, and the page of paths never opens a repo the allowlisted one does not
//! own. Same harness as `mcp_status_full`: a real daemon (in-process), the real client library in
//! another process, temporary profile, home and repos (NFR-01).
//!
//! No fixed waits: the daemon is waited for by its own snapshot, the client by a signal file or
//! its answers; a deadline is only a ceiling.
#![cfg(any(target_os = "macos", target_os = "linux"))]
// The harness is copied whole; each test uses part of it.
#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::messages::{ClientKind, RepoView, WorktreeStatus};
use gitraptor_api::methods::{self, MCP_UNAVAILABLE, McpStatus};
use gitraptor_api::rpc::code;
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use gitraptor_api::{Actor, PROTOCOL_VERSION};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{Agent, AgentKind, Origin, ProfileDirs, WriteOp};
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

/// A cursor to the paths of another worktree that was deleted after the page that gave it is a
/// cursor that does not exist: "mcp-unavailable" would tell the agent its own worktree is gone.
#[test]
fn a_cursor_to_another_worktree_that_went_away_is_not_found() {
    let s = scenario();
    let r = Setup::new(&s.fx).start();
    s.wait_ready(&r);
    let (first, go) = (signal_file(&r, "first-done"), signal_file(&r, "go"));
    let client = r.spawn_client(
        &canonical(&s.wt_c),
        json!([
            ["mcp.status", {}],
            ["mcp.status", {"cursor": "$0:/repo/worktrees/cursor"}],
            ["#signal", first],
            ["#wait", go],
            ["mcp.status", {"cursor": "$1:/page/worktrees/1/changes/cursor"}],
            ["mcp.status", {}],
        ]),
    );
    wait_for_file(&first);
    std::fs::remove_dir_all(&s.wt_b).unwrap();
    std::fs::write(&go, b"").unwrap();
    let a = client.answers();

    let page = status(&a[1]).page.expect("a page of worktrees");
    assert_eq!(page.worktrees[1].name.raw(), "wt-b", "{}", a[1]);
    let (c, data) = error(&a[2]);
    assert_eq!(c, code::NOT_FOUND, "{}", a[2]);
    assert_eq!(data, None);
    assert_no_data(&a[2], &secrets_of(&s, &r.repo_id));
    // The caller's own worktree is fine.
    assert_eq!(status(&a[3]).worktree.raw(), "wt-c");
}

/// The caller's own worktree keeps "mcp-unavailable" when its page of paths cannot be read.
#[test]
fn a_cursor_to_the_own_worktree_that_went_away_is_unavailable() {
    let s = scenario();
    let r = Setup::new(&s.fx).start();
    s.wait_ready(&r);
    let (first, go) = (signal_file(&r, "first-done"), signal_file(&r, "go"));
    let client = r.spawn_client(
        &canonical(&s.wt_b),
        json!([
            ["mcp.status", {}],
            ["#signal", first],
            ["#wait", go],
            ["mcp.status", {"cursor": "$0:/here/changes/cursor"}],
        ]),
    );
    wait_for_file(&first);
    std::fs::remove_dir_all(&s.wt_b).unwrap();
    std::fs::write(&go, b"").unwrap();
    let a = client.answers();
    let (c, data) = error(&a[1]);
    assert_eq!(c, MCP_UNAVAILABLE.code, "{}", a[1]);
    assert_eq!(data, Some(json!({"reason": "worktree-missing"})));
}

// ----- The page of paths never opens a repo that is not the worktree's -----------------------

fn other_repo_with_a_secret() -> Fixture {
    let other = plain();
    other.write("SECRET-other.txt", "x\n");
    other
}

/// Moves the `.git` of `root` aside, so that what the test puts there stands alone.
fn move_git_aside(root: &Path) {
    std::fs::rename(root.join(".git"), root.join(".git-aside")).unwrap();
}

fn refused(
    result: Result<
        (
            gitraptor_api::messages::ChangeCounts,
            Vec<gitraptor_api::messages::FileChangeView>,
        ),
        gitraptor_git::ReadError,
    >,
) {
    match result {
        Err(gitraptor_git::ReadError::Untrusted(message)) => {
            assert!(!message.contains("SECRET"), "{message}");
        }
        other => panic!("expected a refusal as untrusted, got {other:?}"),
    }
}

/// The common directory of the repo of the worktree at `root` and the name it has for it,
/// read before the `.git` is swapped.
fn named(root: &Path) -> (PathBuf, Option<String>) {
    let common = gitraptor_core::observe::locate(root).unwrap();
    let link = std::fs::read_to_string(root.join(".git")).ok();
    let id = link.and_then(|text| {
        let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        Some(target.file_name()?.to_string_lossy().into_owned())
    });
    (common, id)
}

fn all_changes(
    (common, id): &(PathBuf, Option<String>),
    root: &Path,
) -> Result<
    (
        gitraptor_api::messages::ChangeCounts,
        Vec<gitraptor_api::messages::FileChangeView>,
    ),
    gitraptor_git::ReadError,
> {
    let main = id.is_none();
    let root = gitraptor_core::observe::canonical(root);
    gitraptor_core::observe::all_changes(common, &root, main, id.as_deref())
}

#[test]
fn a_main_git_that_is_a_file_pointing_elsewhere_is_refused() {
    let fx = plain();
    let other = other_repo_with_a_secret();
    let named = named(&fx.repo);
    all_changes(&named, &fx.repo).unwrap();
    move_git_aside(&fx.repo);
    std::fs::write(
        fx.repo.join(".git"),
        format!("gitdir: {}\n", other.repo.join(".git").display()),
    )
    .unwrap();
    refused(all_changes(&named, &fx.repo));
}

#[test]
fn a_main_git_that_is_a_symlink_is_refused() {
    let fx = plain();
    let other = other_repo_with_a_secret();
    let named = named(&fx.repo);
    move_git_aside(&fx.repo);
    std::os::unix::fs::symlink(other.repo.join(".git"), fx.repo.join(".git")).unwrap();
    refused(all_changes(&named, &fx.repo));
}

#[test]
fn a_linked_git_that_is_a_symlink_is_refused_and_a_real_one_is_read() {
    let s = scenario();
    let named = named(&s.wt_b);
    assert!(named.1.is_some());
    let counts = all_changes(&named, &s.wt_b).unwrap().0;
    assert_eq!(counts.total(), 4);
    let other = other_repo_with_a_secret();
    move_git_aside(&s.wt_b);
    std::os::unix::fs::symlink(other.repo.join(".git"), s.wt_b.join(".git")).unwrap();
    refused(all_changes(&named, &s.wt_b));
}
