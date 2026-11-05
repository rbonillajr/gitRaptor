//! A repo added while the daemon is already serving gets its pairs published without any file
//! event: the first read of the added repo plans it. Real daemon in-process
//! (`PredictMerger::InProcess`), temporary profile and repo with two linked worktrees whose
//! commits conflict (NFR-01). Every wait is on the notification that proves the fact; the
//! deadline is only a ceiling.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::methods;
use gitraptor_api::prediction::{PairLevel, PairSideView, PairStateView, PairSummary};
use gitraptor_api::scope::{Scope, ScopeEventNotification, ScopeSnapshot};
use gitraptor_core::client::socket_path;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, PowerConfig, StopCause, StopReport,
};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(30);

fn commit_in(fx: &Fixture, wt: &Path, content: &str) {
    std::fs::write(wt.join("a.txt"), content).unwrap();
    fx.git_in(wt, &["add", "."]);
    fx.git_in(wt, &["commit", "-q", "-m", "work"]);
}

/// A JSON-RPC line out, a line in; notifications that arrive before an answer wait in `notes`.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    notes: Vec<Value>,
    next_id: u64,
}

impl Raw {
    fn open(dirs: &gitraptor_core::profile::ProfileDirs) -> Self {
        let path = socket_path(dirs).unwrap();
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(stream) => break stream,
                Err(err) => {
                    assert!(
                        start.elapsed() < DEADLINE,
                        "the daemon never listened: {err}"
                    );
                    std::thread::yield_now();
                }
            }
        };
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            notes: Vec::new(),
            next_id: 1,
        };
        raw.result(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        raw.result(
            methods::CONNECTION_ACCEPT,
            json!({"capabilities": [methods::CAP_PREDICTION_PAIRS.name]}),
        );
        raw
    }

    fn read_value(&mut self, timeout: Duration) -> Option<Value> {
        self.stream.set_read_timeout(Some(timeout)).unwrap();
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => panic!("the daemon closed the connection"),
            Ok(_) => Some(serde_json::from_str(&line).unwrap()),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                None
            }
            Err(e) => panic!("read: {e}"),
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
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
                assert!(value.get("error").is_none(), "{method}: {value}");
                return value["result"].clone();
            }
            self.notes.push(value);
        }
    }

    fn next_event(&mut self, left: Duration) -> Option<ScopeEventNotification> {
        loop {
            let note = if self.notes.is_empty() {
                self.read_value(left)?
            } else {
                self.notes.remove(0)
            };
            if note["method"] == methods::NOTIFY_SCOPE_EVENT {
                return Some(serde_json::from_value(note["params"].clone()).unwrap());
            }
        }
    }
}

fn root_is(side: &PairSideView, want: &Path) -> bool {
    matches!(side, PairSideView::Worktree { root } if Path::new(root.raw()) == want)
}

const ROLE: &str = "GITRAPTOR_PRA_ROLE";
const ROOT: &str = "GITRAPTOR_PRA_ROOT";
const REPO: &str = "GITRAPTOR_PRA_REPO";
const WT_A: &str = "GITRAPTOR_PRA_WT_A";
const WT_B: &str = "GITRAPTOR_PRA_WT_B";

fn var(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap())
}

/// The daemon's role: it serves the profile at `ROOT` until its stdin closes. A process of its
/// own, so the developer's terminal is the client's, not the daemon's.
#[test]
fn the_daemon_role() {
    if std::env::var(ROLE).as_deref() != Ok("daemon") {
        return;
    }
    let dirs = gitraptor_core::profile::ProfileDirs::under_root(var(ROOT).join("profile"));
    let env = DaemonEnv::from_vars(std::env::vars_os());
    // A session of Claude Code running the suite must not make the developer an agent.
    let mut channel = gitraptor_core::channel::ChannelConfig::default();
    channel.agents = gitraptor_core::channel::AgentMatcher::only(vec!["fake-agent".to_owned()]);
    let config = DaemonConfig {
        dirs,
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
        power: PowerConfig::default(),
        predict: PredictConfig {
            merger: PredictMerger::InProcess,
        },
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let _stop = StopOnDrop(Some((handle, join)));
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
}

/// The developer's role, under a pseudo-terminal: `repo.add` is reserved to a caller with one.
#[test]
fn the_developer_role() {
    if std::env::var(ROLE).as_deref() != Ok("developer") {
        return;
    }
    let (wt_a, wt_b) = (var(WT_A), var(WT_B));
    let tp_dirs = gitraptor_core::profile::ProfileDirs::under_root(var(ROOT).join("profile"));
    let mut raw = Raw::open(&tp_dirs);
    let added = raw.result(
        methods::REPO_ADD,
        json!({"path": var(REPO).to_string_lossy()}),
    );
    let repo_id = added["repo"]["repo_id"].as_str().unwrap().to_owned();
    let scope = Scope::Repo { repo_id };
    let snap: ScopeSnapshot =
        serde_json::from_value(raw.result(methods::SCOPE_SNAPSHOT, json!({"scope": scope})))
            .unwrap();
    let ScopeSnapshot::Repo(repo) = &snap else {
        panic!("{snap:?}")
    };
    let mut pairs: BTreeMap<String, PairSummary> = repo
        .repo
        .prediction
        .clone()
        .expect("RepoView.prediction is missing for a connection with prediction.pairs")
        .pairs
        .into_iter()
        .map(|p| (p.pair_id.clone(), p))
        .collect();
    raw.result(
        methods::SCOPE_SUBSCRIBE,
        json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
    );

    let conflicts = |pairs: &BTreeMap<String, PairSummary>| {
        pairs.values().any(|p| {
            root_is(&p.a, &wt_a.clone().min(wt_b.clone()))
                && p.state == PairStateView::Current
                && p.level == Some(PairLevel::Conflict)
                && !matches!(p.b, PairSideView::Base {})
        })
    };
    let start = Instant::now();
    while !conflicts(&pairs) {
        let left = DEADLINE.saturating_sub(start.elapsed());
        let event = raw.next_event(left).unwrap_or_else(|| {
            panic!(
                "no conflicting pair was published for the added repo: {:?}",
                pairs
                    .values()
                    .map(|p| (p.state, p.level))
                    .collect::<Vec<_>>()
            )
        });
        if event.event.kind == methods::PREDICTION_CHANGED {
            let changed: gitraptor_api::prediction::PredictionChanged =
                serde_json::from_value(event.event.data).unwrap();
            for id in changed.removed {
                pairs.remove(&id);
            }
            for p in changed.pairs {
                pairs.insert(p.pair_id.clone(), p);
            }
        }
    }
}

/// Stops the daemon at the end of the test, whatever happens.
struct StopOnDrop(
    Option<(
        gitraptor_core::daemon::ShutdownHandle,
        std::thread::JoinHandle<StopReport>,
    )>,
);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if let Some((handle, join)) = self.0.take() {
            handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

/// Runs the two roles as children of this test binary: the daemon, and the developer under
/// `script` (a pty). The agent classifier is replaced so a Claude Code session running the suite
/// does not make the developer an agent.
/// Linux: Pendiente: etapa de validación multiplataforma.
#[cfg(target_os = "macos")]
#[test]
fn a_repo_added_while_serving_gets_its_pairs_without_a_file_event() {
    use std::process::{Command, Stdio};
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.git(&["branch", "a"]);
    fx.git(&["branch", "b"]);
    let wt_a = fx.add_worktree("a", "a").canonicalize().unwrap();
    let wt_b = fx.add_worktree("b", "b").canonicalize().unwrap();
    commit_in(&fx, &wt_a, "alpha-a\n");
    commit_in(&fx, &wt_b, "alpha-b\n");
    // The profile starts empty: the repo is added only after the daemon serves.
    let tp = TempProfile::new();
    let exe = std::env::current_exe().unwrap();
    let base = |role: &str, test: &str| {
        let mut cmd = Command::new(&exe);
        cmd.args(["--exact", test, "--nocapture"])
            .env_clear()
            .env(ROLE, role)
            .env(ROOT, tp.root.path())
            .env(REPO, &fx.repo)
            .env(WT_A, &wt_a)
            .env(WT_B, &wt_b)
            .envs(std::env::vars_os().filter(|(k, _)| k == "PATH" || k == "TMPDIR"));
        cmd
    };
    let mut daemon = base("daemon", "the_daemon_role")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let developer = base("developer", "the_developer_role");
    let mut script = Command::new("/usr/bin/script");
    script
        .args(["-q", "/dev/null"])
        .arg(developer.get_program())
        .args(developer.get_args())
        .env_clear()
        .envs(developer.get_envs().filter_map(|(k, v)| Some((k, v?))))
        .stdin(Stdio::null());
    let out = script.output().unwrap();
    // Closing its stdin ends the daemon; either way it is waited for.
    drop(daemon.stdin.take());
    let _ = daemon.wait();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
