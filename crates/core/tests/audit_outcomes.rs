//! `audit.outcomes` (M-01 and L-01 of #209): `audit.list` shows every durable row to a client
//! that holds the capability, shows an older client exactly what it always showed, and a page is
//! never short while rows remain. A real daemon (in-process) over a temporary profile (NFR-01).
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::capability::CAPABILITIES_PROTOCOL;
use gitraptor_api::messages::{AuditEntry, AuditListResult, AuditOutcome, ClientKind};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::{AuditRow, ProfileDirs};
use gitraptor_git::resolve::ResolveConfig;
use serde_json::json;

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs) -> Self {
        let config = DaemonConfig {
            dirs: dirs.clone(),
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
            tm_capture: Default::default(),
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

    /// A client of `protocol`. From 9 on the library client accepts every capability it knows;
    /// before 9 there are none to accept.
    fn client(&self, protocol: u32) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, protocol) {
                Ok(client) => return client,
                Err(err) if start.elapsed() < Duration::from_secs(5) => {
                    let _ = err;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
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

/// A row of the reserved audit as the channel writes it (the whole identity of the client).
fn call_row(at_ms: i64, outcome: &str) -> AuditRow {
    let client = json!({
        "pid": 41, "start_us": 7, "exe": null, "agent_ancestor": false,
        "daemon_descendant": false, "controlling_terminal": true, "chain_truncated": false,
    });
    AuditRow {
        at_ms,
        operation: "repo.add".into(),
        repo_id: Some("r1".into()),
        outcome: outcome.into(),
        reason: None,
        client: client.to_string(),
        chain: "[]".into(),
    }
}

/// A row of the end of an announced uninstall, as `Daemon::audit_pending` writes it: the risk
/// text as reason and only the process as client.
fn end_row(at_ms: i64, outcome: &str) -> AuditRow {
    AuditRow {
        at_ms,
        operation: "guard.uninstall".into(),
        repo_id: Some("r1".into()),
        outcome: outcome.into(),
        reason: Some(
            "risk-accepted: ADR-GRD-007 § 2 (2026-10-04); uncovered: planted-code; action a1"
                .into(),
        ),
        client: r#"{"pid":7,"start_us":11}"#.into(),
        chain: r#"{"pid":7,"start_us":1}"#.into(),
    }
}

fn seed(tp: &TempProfile, rows: &[AuditRow]) {
    let mut profile = tp.open();
    for row in rows {
        profile.append_audit(row).unwrap();
    }
}

fn page(client: &mut Client, after_id: i64, limit: u32) -> Vec<AuditEntry> {
    let list: AuditListResult = client
        .call(
            methods::AUDIT_LIST,
            json!({"after_id": after_id, "limit": limit}),
        )
        .unwrap();
    list.entries
}

/// Every page of `limit` entries until the last one, asserting that only the last is short.
fn pages(client: &mut Client, limit: u32) -> Vec<AuditEntry> {
    let mut all = Vec::new();
    let mut after = 0;
    loop {
        let entries = page(client, after, limit);
        let short = entries.len() < limit as usize;
        after = entries.last().map_or(after, |e| e.id);
        all.extend(entries);
        if short {
            return all;
        }
    }
}

fn outcomes(entries: &[AuditEntry]) -> Vec<AuditOutcome> {
    entries.iter().map(|e| e.outcome).collect()
}

#[test]
fn a_client_with_the_capability_lists_the_four_ends_of_an_announced_action() {
    let tp = TempProfile::new();
    seed(
        &tp,
        &[
            call_row(1, "accepted"),
            end_row(2, "applied"),
            call_row(3, "rejected"),
            end_row(4, "cancelled"),
            end_row(5, "failed"),
            call_row(6, "not-implemented"),
            end_row(7, "expired"),
        ],
    );
    let r = Running::start(tp.dirs());
    let mut client = r.client(PROTOCOL_VERSION);
    let entries = page(&mut client, 0, 100);
    use AuditOutcome::*;
    assert_eq!(
        outcomes(&entries),
        [
            Accepted,
            Applied,
            Rejected,
            Cancelled,
            Failed,
            NotImplemented,
            Expired
        ]
    );
    for entry in &entries {
        let ended = entry.outcome.needs_capability();
        assert_eq!(entry.client_partial, ended, "{entry:?}");
        // The end of an action has no refusal reason: the outcome says what happened.
        assert_eq!(entry.reason, None);
        assert_eq!(entry.client.pid, if ended { 7 } else { 41 });
    }
}

#[test]
fn a_client_without_the_capability_sees_what_it_always_saw() {
    let tp = TempProfile::new();
    seed(
        &tp,
        &[
            call_row(1, "accepted"),
            end_row(2, "applied"),
            call_row(3, "rejected"),
            end_row(4, "expired"),
            call_row(5, "not-implemented"),
        ],
    );
    let r = Running::start(tp.dirs());
    use AuditOutcome::*;
    // Before protocol 9 there are no capabilities.
    for protocol in 5..CAPABILITIES_PROTOCOL {
        let mut old = r.client(protocol);
        let entries = page(&mut old, 0, 100);
        assert_eq!(outcomes(&entries), [Accepted, Rejected, NotImplemented]);
        assert!(entries.iter().all(|e| !e.client_partial));
        // On the wire too: the new field and the new outcomes are not there at all.
        let raw: serde_json::Value = old.call(methods::AUDIT_LIST, json!({})).unwrap();
        let text = raw.to_string();
        for new in [
            "client_partial",
            "applied",
            "cancelled",
            "failed",
            "expired",
        ] {
            assert!(!text.contains(new), "{new}: {text}");
        }
    }
}

/// L-01: the limit applied before the filter, so a page came out short or empty.
#[test]
fn a_page_is_never_short_while_rows_remain() {
    let tp = TempProfile::new();
    // Twenty ends first, then three calls: the first page of an old client used to be empty.
    let mut rows: Vec<AuditRow> = (0..20).map(|i| end_row(i, "expired")).collect();
    rows.extend((20..23).map(|i| call_row(i, "accepted")));
    // And ten calls, each followed by three ends.
    for i in 0..10 {
        rows.push(call_row(100 + i * 4, "rejected"));
        rows.extend((1..4).map(|j| end_row(100 + i * 4 + j, "applied")));
    }
    seed(&tp, &rows);
    let r = Running::start(tp.dirs());

    let mut old = r.client(CAPABILITIES_PROTOCOL - 1);
    assert_eq!(page(&mut old, 0, 3).len(), 3, "the first page was short");
    let legacy = pages(&mut old, 3);
    assert_eq!(legacy.len(), 13);
    assert!(legacy.iter().all(|e| !e.outcome.needs_capability()));

    let mut new = r.client(PROTOCOL_VERSION);
    let all = pages(&mut new, 7);
    assert_eq!(all.len(), rows.len());
    let ids: Vec<i64> = all.iter().map(|e| e.id).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
}

/// A row that cannot be read is skipped, and the page is filled from the rows after it.
#[test]
fn an_unreadable_row_does_not_shorten_the_page() {
    let tp = TempProfile::new();
    let mut broken = end_row(2, "applied");
    broken.client = "not json".into();
    let mut rows = vec![call_row(1, "accepted"), broken];
    rows.extend((3..8).map(|i| call_row(i, "accepted")));
    seed(&tp, &rows);
    let r = Running::start(tp.dirs());
    let mut client = r.client(PROTOCOL_VERSION);
    let first = page(&mut client, 0, 4);
    assert_eq!(first.len(), 4, "{first:?}");
    assert!(first.iter().all(|e| e.id != 2));
}

/// Out of reads with rows left, the daemon says so instead of a short page that reads as the end.
#[test]
fn a_run_of_unreadable_rows_is_an_error_not_an_empty_page() {
    let tp = TempProfile::new();
    let mut rows: Vec<AuditRow> = (1..=12)
        .map(|i| {
            let mut broken = end_row(i, "applied");
            broken.client = "not json".into();
            broken
        })
        .collect();
    rows.push(call_row(13, "accepted"));
    seed(&tp, &rows);
    let r = Running::start(tp.dirs());
    let mut client = r.client(PROTOCOL_VERSION);
    let err = client
        .call::<_, AuditListResult>(methods::AUDIT_LIST, json!({"limit": 1}))
        .unwrap_err();
    assert!(
        matches!(err, gitraptor_core::client::ClientError::Rpc(e) if e.code == gitraptor_api::rpc::code::INTERNAL)
    );
    // A page that reaches the readable rows is full.
    assert_eq!(page(&mut client, 0, 100).len(), 1);
}
