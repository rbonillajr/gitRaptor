//! The `changed` outcome of a confirmation of the team baseline (DS-US-GRD-014 D10) reaches only
//! a connection with `audit.outcomes` and `guard.team-confirmation`: a client older than the
//! outcome, with `audit.outcomes` alone, could not decode it and never gets it from `audit.list`.
//! Real daemon over a temporary profile (NFR-01); raw connections pick their capabilities.
#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::socket_path;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{AuditRow, ProfileDirs};
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

struct Running {
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn start(dirs: ProfileDirs) -> Running {
    let config = DaemonConfig {
        dirs,
        env: DaemonEnv::from_vars(Vec::new()),
        git: ResolveConfig {
            configured_path: None,
            path_env: None,
            known_locations: Vec::new(),
            shim_paths: Vec::new(),
            toolchain_gits: Vec::new(),
        },
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
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    Running {
        handle,
        join: Some(join),
    }
}

struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

impl Raw {
    fn open(dirs: &ProfileDirs, capabilities: &[&str]) -> Self {
        let path = socket_path(dirs).unwrap();
        let begin = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(s) => break s,
                Err(_) if begin.elapsed() < Duration::from_secs(10) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("connect: {e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            next: 0,
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        assert!(hello.get("result").is_some(), "{hello}");
        let accept = raw.call(
            methods::CONNECTION_ACCEPT,
            json!({ "capabilities": capabilities }),
        );
        assert!(accept.get("result").is_some(), "{accept}");
        raw
    }

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

    fn outcomes(&mut self) -> Vec<String> {
        let answer = self.call(methods::AUDIT_LIST, json!({"after_id": 0, "limit": 50}));
        answer["result"]["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("{answer}"))
            .iter()
            .map(|e| e["outcome"].as_str().unwrap().to_owned())
            .collect()
    }
}

fn end_row(at_ms: i64, outcome: &str) -> AuditRow {
    AuditRow {
        at_ms,
        operation: "guard.confirm".into(),
        repo_id: Some("r1".into()),
        outcome: outcome.into(),
        reason: Some("risk-accepted: ADR-GRD-007 § 2; fingerprint ab".into()),
        client: r#"{"pid":7,"start_us":11}"#.into(),
        chain: r#"{"pid":7,"start_us":11}"#.into(),
    }
}

#[test]
fn the_changed_outcome_needs_the_team_confirmation_capability() {
    let tp = TempProfile::new();
    {
        let mut profile = tp.open();
        for (at, outcome) in ["applied", "changed", "cancelled"].iter().enumerate() {
            profile.append_audit(&end_row(at as i64, outcome)).unwrap();
        }
    }
    let _daemon = start(tp.dirs());
    let dirs = tp.dirs();
    let mut older = Raw::open(&dirs, &["audit.outcomes"]);
    let mut newer = Raw::open(&dirs, &["audit.outcomes", "guard.team-confirmation"]);
    assert_eq!(older.outcomes(), ["applied", "cancelled"]);
    assert_eq!(newer.outcomes(), ["applied", "changed", "cancelled"]);
}
