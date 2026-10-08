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
