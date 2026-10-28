//! US-GRD-017 in the daemon (D6, D7): an allowed destructive operation whose prior snapshot
//! failed is denied. A connection that granted `guard.prior-snapshot` but not
//! `guard.snapshot-required` reads the same denial as `system.internal-error`, without cause and
//! without `priorSnapshot`; never a permit. A second evaluation of the same `git` denies again.
//!
//! An in-process daemon without a Time Machine engine (every prior is `failed`, `unavailable`)
//! over a temporary profile and a temporary repo (NFR-01); raw connections, so each test says
//! exactly which capabilities it grants. Asserts on the wire JSON.
#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use common::{TempProfile, git};
use gitraptor_api::capability::AcceptResult;
use gitraptor_api::guard::{EvaluateParams, GuardLogResult, Hook, Operation, RefUpdate, RefValue};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause,
};
use gitraptor_core::guardrails::snapshot_required;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::{Value, json};

struct Running {
    profile: TempProfile,
    handle: ShutdownHandle,
    join: Option<std::thread::JoinHandle<gitraptor_core::daemon::StopReport>>,
}

impl Running {
    fn start() -> Self {
        let profile = TempProfile::new();
        let config = DaemonConfig {
            dirs: profile.dirs(),
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
        Self {
            profile,
            handle,
            join: Some(join),
        }
    }

    /// A raw connection that granted exactly `caps` (and nothing else beyond the protocol).
    fn raw(&self, caps: &[&str]) -> Raw {
        let path = gitraptor_core::client::socket_path(&self.profile.dirs()).unwrap();
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(s) => break s,
                Err(e) if start.elapsed() > Duration::from_secs(5) => panic!("connect: {e}"),
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
            next: 1,
        };
        let hello = raw.call(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "other", "client_version": "t"}),
        );
        assert!(hello["result"].is_object(), "{hello}");
        let accepted = raw.call(methods::CONNECTION_ACCEPT, json!({ "capabilities": caps }));
        let granted: AcceptResult = serde_json::from_value(accepted["result"].clone()).unwrap();
        for c in caps {
            assert!(
                granted.capabilities.contains(&(*c).to_owned()),
                "the daemon does not serve {c}: {accepted}"
            );
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
        let id = self.next;
        self.next += 1;
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

    /// `guard.evaluate` of the deletion of `refs/heads/feat-x`: the rules allow it, and it is
    /// an operation that wants a prior.
    fn delete_branch(&mut self, common_dir: &std::path::Path) -> Value {
        let params = EvaluateParams {
            exception_token: None,
            repo_id: "0000-ffff".into(),
            common_dir: common_dir.to_string_lossy().into_owned(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: "refs/heads/feat-x".into(),
                    old: RefValue::Zero,
                    new: RefValue::Zero,
                }],
                orphan_head: None,
            },
            authorship: None,
        };
        let answer = self.call(
            methods::GUARD_EVALUATE,
            serde_json::to_value(params).unwrap(),
        );
        assert!(answer["result"].is_object(), "{answer}");
        answer["result"].clone()
    }
}

fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "a"]);
    let common_dir = repo.path().join(".git").canonicalize().unwrap();
    (repo, common_dir)
}

/// D6: without `guard.snapshot-required` the denial reads as `system.internal-error`, with no
/// cause and no `priorSnapshot`; the operation is denied, not allowed.
#[test]
fn legacy_connection_reads_internal_error() {
    let daemon = Running::start();
    let (_repo, common_dir) = repo();
    let mut client = daemon.raw(&[methods::CAP_GUARD_PRIOR_SNAPSHOT.name]);
    let decision = client.delete_branch(&common_dir);
    assert_eq!(decision["effect"], "deny", "{decision}");
    assert_eq!(decision["appliedEffect"], "deny", "{decision}");
    let reasons = decision["reasons"].as_array().unwrap();
    assert!(
        reasons.iter().any(|r| r["rule"] == "system.internal-error"),
        "{decision}"
    );
    assert!(
        reasons
            .iter()
            .all(|r| r["rule"] != "system.snapshot-failed"),
        "{decision}"
    );
    assert!(
        reasons.iter().all(|r| r.get("cause").is_none()),
        "no cause for a legacy client: {decision}"
    );
    assert!(decision.get("priorSnapshot").is_none(), "{decision}");
}

/// D6: `guard.log` for a connection without the capability shows the same entry as
/// `system.internal-error` without a cause, and it stays a denial.
#[test]
fn legacy_guard_log_reads_internal_error() {
    let mut log: GuardLogResult = serde_json::from_value(json!({
        "sinceMs": 1,
        "summary": {"blocked": 1, "notices": 0, "rateLimited": 0},
        "entries": [{
            "atMs": 10, "utcOffsetS": 0, "lastMs": 10, "count": 1,
            "actor": null,
            "operation": {"kind": "ref-transaction", "refs": [{"name": {"untrusted": "feat-x"}, "change": "delete"}]},
            "kind": "denial", "detail": "full",
            "effect": "deny", "appliedEffect": "deny",
            "reasons": [{"rule": "system.snapshot-failed", "level": "system", "cause": "snapshot-time-limit"}],
            "layer": "hooks", "origin": "daemon", "decisionId": "d-1"
        }],
        "unloggedPeriods": []
    }))
    .unwrap();
    snapshot_required::legacy_log(&mut log);
    let wire = serde_json::to_value(&log).unwrap();
    let entry = &wire["entries"][0];
    assert_eq!(entry["kind"], "denial", "{wire}");
    assert_eq!(entry["appliedEffect"], "deny", "{wire}");
    assert_eq!(
        entry["reasons"][0]["rule"], "system.internal-error",
        "{wire}"
    );
    assert!(entry["reasons"][0].get("cause").is_none(), "{wire}");
    assert_eq!(wire["summary"]["blocked"], 1, "the count stays: {wire}");
}

/// D7: the same `git` evaluated twice (its prior failed, and the failure is reused) is denied
/// both times, with the rule and its cause.
#[test]
fn reused_failure_denies_again() {
    let daemon = Running::start();
    let (_repo, common_dir) = repo();
    let mut client = daemon.raw(&[
        methods::CAP_GUARD_PRIOR_SNAPSHOT.name,
        methods::CAP_GUARD_SNAPSHOT_REQUIRED.name,
    ]);
    for attempt in 1..=2 {
        let decision = client.delete_branch(&common_dir);
        assert_eq!(decision["effect"], "deny", "attempt {attempt}: {decision}");
        assert_eq!(
            decision["appliedEffect"], "deny",
            "attempt {attempt}: {decision}"
        );
        let reason = &decision["reasons"][0];
        assert_eq!(
            reason["rule"], "system.snapshot-failed",
            "attempt {attempt}: {decision}"
        );
        assert_eq!(reason["level"], "system");
        assert_eq!(
            reason["cause"], "snapshot-unavailable",
            "attempt {attempt}: {decision}"
        );
        assert_eq!(decision["priorSnapshot"]["outcome"], "failed", "{decision}");
    }
}
