//! US-GRD-016 (DS T006, T007, T009): `guard.status` serves the four protection states behind
//! `guard.protection-layers` (two states without it), the allowlist transition is a log entry
//! only that connection sees, and `guard.log` filters by layer. In-process daemon over a
//! temporary profile and repo (NFR-01); the real Claude Code session running the suite is not an
//! agent here (the matcher only knows `raptor-fake-agent`).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::*;
use gitraptor_api::capability::Capability;
use gitraptor_api::guard::{
    Decision, Effect, EvaluateParams, ExceptionState, Hook, Level, LogKind, LogLayer, Operation,
    Param, ParamKind, PushUpdate, Reason, RefValue, Rule,
};
use gitraptor_api::{AgentKind, PROTOCOL_VERSION, Untrusted, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::socket_path;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::guardrails::constants::STUB_FILE;
use gitraptor_core::guardrails::install::{GuardCtx, install};
use gitraptor_core::guardrails::log::{LogContext, LogEntry, entry};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::Invoker;
use gitraptor_git::resolve::{Resolution, ResolveConfig, resolve};
use serde_json::{Value, json};

const NOW: i64 = 1_800_000_000_000;
/// What a connection of today accepts about the guard.
const TODAY: [&str; 4] = [
    "guard.protection",
    "guard.pending-action",
    "guard.recovery",
    "guard.log-layers",
];

fn protection_layers() -> &'static str {
    Capability::new("guard.protection-layers").name
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

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os().collect::<Vec<_>>());
        let channel = ChannelConfig {
            agents: AgentMatcher::only(vec!["raptor-fake-agent".into()]),
            ..ChannelConfig::default()
        };
        let config = DaemonConfig {
            dirs: dirs.clone(),
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
            power: Default::default(),
            tm_capture: Default::default(),
            predict: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            dirs,
            handle,
            join: Some(join),
        }
    }

    /// A connection of protocol 9 that accepts exactly `capabilities`.
    fn raw(&self, capabilities: &[&str]) -> Raw {
        let path = socket_path(&self.dirs).unwrap();
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(s) => break s,
                Err(e) if start.elapsed() > Duration::from_secs(10) => panic!("connect: {e}"),
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Raw {
            stream,
            reader,
            next: 0,
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        assert!(hello["result"].is_object(), "{hello}");
        if !capabilities.is_empty() {
            let accept = raw.call(
                methods::CONNECTION_ACCEPT,
                json!({ "capabilities": capabilities }),
            );
            assert!(accept["result"].is_object(), "{accept}");
        }
        raw
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

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

impl Raw {
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
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

    fn result(&mut self, method: &str, params: Value) -> Value {
        let answer = self.call(method, params);
        answer
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{method} must answer a result: {answer}"))
    }
}

fn stand_in_raptor(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    for name in ["raptor", STUB_FILE] {
        let file = dir.join(name);
        std::fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir.join("raptor")
}

/// The repo is observed and has the hooks installed (the hook half of the state is "installed").
/// Returns its repo id.
fn observe_and_protect(tp: &TempProfile, repo: &Path) -> String {
    let common = common_dir(repo);
    let raptor = stand_in_raptor(&tp.root.path().join("bin"));
    let mut profile = tp.open();
    let repo_id = profile.add_repo(&common, None, 1).unwrap().0.repo_id;
    let (mut store, _) = profile.open_store(&repo_id).unwrap();
    let invoker = Invoker::default();
    let git = match resolve(&ResolveConfig::for_current_os(None), &invoker) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git: {diagnostics:?}"),
    };
    let dirs = tp.dirs();
    let ctx = GuardCtx {
        git: &git,
        invoker: &invoker,
        dirs: &dirs,
        instance: profile.instance_id(),
        raptor: &raptor,
    };
    let registry = GuardRegistry::for_profile(dirs.clone());
    install(
        &ctx,
        &repo_id,
        &common,
        &mut store,
        &registry,
        now_ms(),
        false,
    )
    .unwrap_or_else(|e| panic!("install: {e:?}"));
    repo_id
}

/// GRD016-CHANNEL-DOWNGRADE: hooks installed and the repo in the allowlist is `full` for a
/// connection with `guard.protection-layers` and `hooks-only` for one without it, which does not
/// see the allowlist entry either.
#[test]
fn guard_status_without_the_capability_keeps_two_states() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "demo", true);
    let repo_id = observe_and_protect(&tp, &repo);
    // The allowlist mark (`raptor mcp enable`), put in the profile before the daemon reads it:
    // `mcp.enable` is reserved to the developer, never to the daemon's own process.
    tp.open()
        .set_mcp_enabled(&repo_id, true, "cli", now_ms())
        .unwrap()
        .expect("the repo is observed");
    let r = Running::start(tp.dirs());
    let with: Vec<&str> = TODAY.iter().copied().chain([protection_layers()]).collect();
    let mut new = r.raw(&with);
    let mut old = r.raw(&TODAY);
    let params = json!({ "path": repo });

    let status_new = new.result("guard.status", params.clone());
    assert_eq!(
        status_new["state"], "full",
        "a repo with hooks and in the allowlist is `full` with guard.protection-layers: \
         {status_new}"
    );
    let status_old = old.result("guard.status", params.clone());
    assert_eq!(
        status_old["state"], "hooks-only",
        "without guard.protection-layers the state keeps the two values: {status_old}"
    );
    // The list of that connection never carries the allowlist entry (the entry itself is
    // written by the daemon's loop on `raptor mcp enable`; the e2e suite checks it).
    let log_old = old.result("guard.log", params);
    assert!(
        !log_old["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["operation"]["kind"] == "mcp-allowlist"),
        "a connection without guard.protection-layers must not see `mcp-allowlist` entries: \
         {log_old:#}"
    );
}

/// A denied push of `branch`, as the log keeps it, with the layer the executor's engine fixes.
fn denial(branch: &str, layer: LogLayer) -> LogEntry {
    let oid = |c: char| RefValue::Oid(c.to_string().repeat(40));
    let params = EvaluateParams {
        exception_token: None,
        repo_id: "r".into(),
        common_dir: "/tmp/demo/.git".into(),
        hook: Hook::PrePush,
        operation: Operation::Push {
            remote: Untrusted::new("origin"),
            updates: vec![PushUpdate {
                local_ref: Some(format!("refs/heads/{branch}")),
                local: oid('a'),
                remote_ref: format!("refs/heads/{branch}"),
                remote: oid('b'),
            }],
        },
        authorship: None,
    };
    let decision = Decision {
        decision_id: "d1".into(),
        effect: Effect::Deny,
        applied_effect: Effect::Deny,
        reasons: vec![Reason {
            rule: Rule::MinimumForcePush,
            level: Level::Minimum,
            cause: None,
            params: vec![Param::new(ParamKind::Branch, branch)],
        }],
        exception: ExceptionState::None,
        config_status: vec![],
        config_ref: vec![],
        notices: vec![],
        prior_snapshot: None,
    };
    let ctx = LogContext {
        actor: Some(AgentKind::ClaudeCode),
        under_executor: false,
        worktree: Some("/tmp/demo".into()),
        branch: Some(branch.into()),
        authorship_policy: None,
        at_ms: NOW,
        utc_offset_s: 0,
    };
    let mut e = entry(&params, &decision, &ctx).expect("a denial is logged");
    assert_eq!(e.kind, LogKind::Denial);
    e.layer = layer;
    e
}

fn layers(log: &Value) -> Vec<String> {
    let mut layers: Vec<String> = log["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("no entries: {log}"))
        .iter()
        .map(|e| e["layer"].as_str().unwrap().to_owned())
        .collect();
    layers.sort();
    layers
}

/// GRD016-LOG-LAYER: the `layer` filter, and what a connection without `guard.log-layers` gets.
#[test]
fn guard_log_filters_by_layer() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "demo", true);
    {
        let mut profile = tp.open();
        let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        for (branch, layer) in [
            ("a", LogLayer::Hooks),
            ("b", LogLayer::Cockpit),
            ("c", LogLayer::Mcp),
        ] {
            store.record_guard_decision(&denial(branch, layer)).unwrap();
        }
    }
    let r = Running::start(tp.dirs());
    let with_layers: Vec<&str> = TODAY.to_vec();

    let mut seeing = r.raw(&with_layers);
    for layer in ["hooks", "mcp", "cockpit"] {
        let answer = seeing.call(methods::GUARD_LOG, json!({ "path": repo, "layer": layer }));
        let log = answer
            .get("result")
            .unwrap_or_else(|| panic!("guard.log must take `layer: {layer}`: {answer}"));
        assert_eq!(
            layers(log),
            [layer],
            "guard.log with layer {layer} returns only that layer: {log:#}"
        );
    }

    // Without guard.log-layers: the list is hooks only, so a filter on another layer is empty.
    let mut plain = r.raw(&[]);
    for layer in ["mcp", "cockpit"] {
        let answer = plain.call(methods::GUARD_LOG, json!({ "path": repo, "layer": layer }));
        let log = answer
            .get("result")
            .unwrap_or_else(|| panic!("guard.log must take `layer: {layer}`: {answer}"));
        assert_eq!(
            layers(log),
            Vec::<String>::new(),
            "a {layer} filter without guard.log-layers must be empty, never hooks entries: {log:#}"
        );
    }
    let answer = plain.call(
        methods::GUARD_LOG,
        json!({ "path": repo, "layer": "hooks" }),
    );
    let log = answer
        .get("result")
        .unwrap_or_else(|| panic!("guard.log must take `layer: hooks`: {answer}"));
    assert_eq!(layers(log), ["hooks"], "{log:#}");
}

/// The `layer` filter runs in the query, before the page is cut: with more recent `hooks`
/// entries than `limit`, an `mcp` page still holds the older `mcp` entry.
#[test]
fn the_layer_filter_fills_the_page() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "demo", true);
    {
        let mut profile = tp.open();
        let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        // Recorded first: the oldest of the page order.
        store
            .record_guard_decision(&denial("m", LogLayer::Mcp))
            .unwrap();
        for branch in ["h1", "h2", "h3"] {
            store
                .record_guard_decision(&denial(branch, LogLayer::Hooks))
                .unwrap();
        }
    }
    let r = Running::start(tp.dirs());
    let mut seeing = r.raw(TODAY.as_ref());
    let answer = seeing.call(
        methods::GUARD_LOG,
        json!({ "path": repo, "layer": "mcp", "limit": 2 }),
    );
    let log = answer
        .get("result")
        .unwrap_or_else(|| panic!("guard.log must answer: {answer}"));
    assert_eq!(
        layers(log),
        ["mcp"],
        "the mcp page must not be empty: {log:#}"
    );
    let answer = seeing.call(
        methods::GUARD_LOG,
        json!({ "path": repo, "layer": "hooks", "limit": 2 }),
    );
    let log = answer.get("result").unwrap();
    assert_eq!(layers(log), ["hooks", "hooks"], "{log:#}");
}
