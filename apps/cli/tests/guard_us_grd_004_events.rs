//! US-GRD-004: the stream of events tells a client that the protection of a repo stopped being
//! active (`guard.protection-lost`) and that it is active again (`guard.protection-restored`),
//! and a client without the capability `guard.protection` hears of neither. Real `raptor` as
//! daemon over the temporary machine of `guard_machine`. Unix only (Pendiente: etapa de
//! validación multiplataforma).
#![cfg(unix)]

mod guard_machine;

use std::time::{Duration, Instant};

use gitraptor_api::event::{GUARD_PROTECTION_LOST, GUARD_PROTECTION_RESTORED};
use gitraptor_api::guard::ProtectionLostData;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::scope::{Scope, ScopeEventNotification, ScopeSnapshot, ScopeSubscribeResult};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use guard_machine::{Machine, text};
use serde_json::json;

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const DEADLINE: Duration = Duration::from_secs(30);

/// A client subscribed to the repo scope of "demo".
fn subscribed(m: &Machine, repo_id: &str) -> Client {
    let mut c = Client::connect(&m.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let scope = Scope::Repo {
        repo_id: repo_id.to_owned(),
    };
    let snap: ScopeSnapshot = c
        .call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
        .unwrap();
    let _: ScopeSubscribeResult = c
        .call(
            methods::SCOPE_SUBSCRIBE,
            json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
        )
        .unwrap();
    c
}

/// The next scoped event of `kind`, skipping the others.
fn next_of(c: &mut Client, kind: &str) -> ScopeEventNotification {
    let start = Instant::now();
    loop {
        let left = DEADLINE.saturating_sub(start.elapsed());
        assert!(!left.is_zero(), "no {kind} event");
        if let Some(n) = c.next_notification(left).unwrap()
            && n.method == methods::NOTIFY_SCOPE_EVENT
        {
            let e: ScopeEventNotification = serde_json::from_value(n.params).unwrap();
            if e.event.kind == kind {
                return e;
            }
        }
    }
}

#[test]
fn a_loss_and_its_repair_are_published() {
    let m = Machine::new();
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    let repo_id = m.status(&m.f.repo).repo_id;
    let mut c = subscribed(&m, &repo_id);

    std::fs::remove_file(m.common().join("gitraptor/hooks/pre-push")).unwrap();
    let lost = next_of(&mut c, GUARD_PROTECTION_LOST);
    let data: ProtectionLostData = serde_json::from_value(lost.event.data).unwrap();
    assert_eq!(data.repo_id, repo_id);
    assert_eq!(
        serde_json::to_value(data.hooks.cause).unwrap(),
        "dispatcher-missing"
    );

    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    let back = next_of(&mut c, GUARD_PROTECTION_RESTORED);
    let data: ProtectionLostData = serde_json::from_value(back.event.data).unwrap();
    assert_eq!(serde_json::to_value(data.hooks.status).unwrap(), "active");
}

#[test]
fn guardrails_own_removal_publishes_no_loss() {
    let m = Machine::new();
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    let repo_id = m.status(&m.f.repo).repo_id;
    let mut c = subscribed(&m, &repo_id);
    let out = m.uninstall(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    // The daemon checks every 100 ms here: a loss would have come by now. The signal that the
    // removal was fully seen is a later event of the same stream (the repo scope's own).
    std::fs::write(m.f.repo.join("later.txt"), "x\n").unwrap();
    m.git_ok(&m.f.repo, &["add", "later.txt"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "later"]);
    let start = Instant::now();
    loop {
        let left = DEADLINE.saturating_sub(start.elapsed());
        assert!(!left.is_zero(), "no git.event after the commit");
        let Some(n) = c.next_notification(left).unwrap() else {
            continue;
        };
        if n.method != methods::NOTIFY_SCOPE_EVENT {
            continue;
        }
        let e: ScopeEventNotification = serde_json::from_value(n.params).unwrap();
        assert_ne!(e.event.kind, GUARD_PROTECTION_LOST, "{e:?}");
        if e.event.kind == gitraptor_api::event::GIT_EVENT {
            break;
        }
    }
}

/// A raw connection: a line out, a line in.
struct Raw {
    stream: std::os::unix::net::UnixStream,
    reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    next: u64,
}

impl Raw {
    fn open(m: &Machine) -> Self {
        let path = gitraptor_core::client::socket_path(&m.dirs()).unwrap();
        let stream = std::os::unix::net::UnixStream::connect(path).unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        let reader = std::io::BufReader::new(stream.try_clone().unwrap());
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
        raw
    }

    fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        use std::io::{BufRead, Write};
        self.next += 1;
        let id = self.next;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        loop {
            let mut answer = String::new();
            self.reader.read_line(&mut answer).unwrap();
            let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
            if answer["id"] == id {
                return answer;
            }
        }
    }
}

// A client that does not know `guard.protection` is never sent what it would not understand:
// not the new fields of the status and not the entries of the new kind of the log.
#[test]
fn a_client_without_the_capability_gets_none_of_it() {
    let m = Machine::new();
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    std::fs::remove_file(m.common().join("gitraptor/hooks/pre-push")).unwrap();
    // The loss is in the log once the daemon has seen it.
    let repo = m.f.repo.to_str().unwrap().to_owned();
    let params = json!({ "path": repo });
    let start = Instant::now();
    let mut new = Raw::open(&m);
    let granted = new.call(
        methods::CONNECTION_ACCEPT,
        json!({"capabilities": ["guard.protection"]}),
    );
    assert!(
        granted["result"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "guard.protection"),
        "{granted}"
    );
    loop {
        let log = new.call(methods::GUARD_LOG, params.clone());
        let seen = log["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "protection-state");
        if seen {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "no transition: {log}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = new.call(methods::GUARD_STATUS, params.clone());
    assert_eq!(status["result"]["hooks"]["status"], "inactive", "{status}");
    assert!(status["result"].get("minimum_set").is_some(), "{status}");

    // Without it: neither the fields nor the entry.
    let mut old = Raw::open(&m);
    let status = old.call(methods::GUARD_STATUS, params.clone());
    for field in ["hooks", "diagnostics", "minimum_set"] {
        assert!(status["result"].get(field).is_none(), "{field}: {status}");
    }
    let log = old.call(methods::GUARD_LOG, params);
    assert!(
        log["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] != "protection-state"),
        "{log}"
    );
}
