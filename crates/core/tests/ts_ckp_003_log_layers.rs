//! TS-CKP-003 (D9, D11): `guard.log` carries the layer of each entry, behind `guard.log-layers`.
//! With the capability the entries say their real layer; without it the list leaves out the ones
//! that are not `hooks` and the summary counts them all (the KPI does not change by layer).
//! In-process daemon over a temporary profile and a temporary repo (NFR-01).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::guard::{
    Decision, Effect, EvaluateParams, ExceptionState, Hook, Level, LogKind, LogLayer, Operation,
    Param, ParamKind, PushUpdate, Reason, RefValue, Rule,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{AgentKind, PROTOCOL_VERSION, Untrusted, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, socket_path};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::guardrails::log::{LogContext, LogEntry, entry};
use gitraptor_core::profile::ProfileDirs;
use serde_json::{Value, json};

const NOW: i64 = 1_800_000_000_000;

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    /// The daemon with the system Git of the environment, so the repo is observed.
    fn start(dirs: ProfileDirs) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os().collect::<Vec<_>>());
        let config = DaemonConfig {
            dirs: dirs.clone(),
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
            power: Default::default(),
            predict: Default::default(),
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        let r = Self {
            dirs,
            handle,
            join: Some(join),
        };
        drop(r.connect());
        r
    }

    /// A library client: it accepts every capability it knows.
    fn connect(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(client) => return client,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    /// A raw connection of protocol 9 that accepts nothing.
    fn raw(&self) -> Raw {
        let stream = UnixStream::connect(socket_path(&self.dirs).unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Raw { stream, reader };
        let hello = raw.call(
            0,
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        assert!(hello["result"].is_object(), "{hello}");
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
}

impl Raw {
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

/// A denied push of `branch`, as a hook would log it, with the layer the executor's engine
/// fixes for its own entries.
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
    // The executor's engine sets the layer on the entry `entry` built for a hook.
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

/// D11 in both directions: the same three entries (hooks, cockpit, mcp) read with and without
/// `guard.log-layers`.
#[test]
fn log_layers_only_with_its_capability() {
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
    let params = json!({ "path": repo });

    // The daemon announces the capability.
    let announced = r.connect().hello().capabilities.clone().unwrap_or_default();
    assert!(
        announced
            .iter()
            .any(|c| c == methods::CAP_GUARD_LOG_LAYERS.name),
        "not announced: {announced:?}"
    );

    // With it: every entry, with its real layer.
    let with: Value = r
        .connect()
        .call(methods::GUARD_LOG, params.clone())
        .expect("guard.log with the capability");
    assert_eq!(layers(&with), ["cockpit", "hooks", "mcp"], "{with:#}");
    assert_eq!(with["summary"]["blocked"], 3);

    // Without it: only the hooks entry in the list, the three in the summary.
    let without = r.raw().call(1, methods::GUARD_LOG, params);
    let without = &without["result"];
    assert_eq!(layers(without), ["hooks"], "{without:#}");
    assert_eq!(without["summary"]["blocked"], 3, "{without:#}");
}
