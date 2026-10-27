//! Who a connection counts for, where the end-to-end tests cannot reach: a process an operation
//! started (its marks exist only inside a running daemon) and the hook client (classified by the
//! installed binary). Neither may take a slot or a budget of its own over MCP.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gitraptor_api::messages::{ClientKind, ConnectionProfile};
use gitraptor_api::timemachine::ResolvedVia;
use gitraptor_api::{Actor, AgentKind, AgentOrigin};

use super::*;
use crate::channel::AgentMatcher;
use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::{ProcError, ProcInfo, ProcSource};
use crate::channel::requester::{Resolution, Who, resolve};
use crate::timemachine::oplog::{Requester, RequesterOrigin};

const UID: u32 = 501;

#[derive(Default)]
struct Tree(HashMap<u32, ProcInfo>);

impl Tree {
    fn add(&mut self, pid: u32, ppid: u32, exe: &str, start_us: u64) {
        self.0.insert(
            pid,
            ProcInfo {
                pid,
                ppid,
                uid: UID,
                start_us,
                exe: Some(PathBuf::from(exe)),
                controlling_terminal: false,
                session: 10,
                pgid: pid,
                desktop_session: None,
            },
        );
    }
}

impl ProcSource for Tree {
    fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
        self.0.get(&pid).cloned().ok_or(ProcError::Gone)
    }
    fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
        None
    }
}

fn agent(session_id: &str) -> Who {
    Who {
        actor: Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        },
        requester: Requester::Agent {
            name: "claude-code".into(),
            origin: RequesterOrigin::Detected,
            session_id: session_id.into(),
        },
    }
}

fn resolution(who: Who, via: ResolvedVia) -> Resolution {
    Resolution {
        who,
        via,
        executor_operation: None,
        confirmable: false,
    }
}

fn not_asked() -> bool {
    panic!("the reserved checks are only asked of an unattributed caller")
}

/// A `raptor-mcp` started by a hook of an operation of agent A, while agent B also runs: it
/// resolves through the marks to A, counts in A's key, and shares A's slots and write budget.
#[test]
fn an_executor_descendant_counts_for_the_plan_requester() {
    let mut t = Tree::default();
    t.add(1, 0, "/sbin/launchd", 1);
    t.add(26, 1, "/Users/u/.local/share/claude/versions/2.1.0", 260);
    // The hook the operation started, reparented, and the client it launched.
    t.add(73, 26, "/repo/.git/hooks/post-commit", 730);
    t.add(74, 73, "/usr/local/bin/raptor-mcp", 740);
    let marks = Arc::new(ExecutorMarks::default());
    let plan_requester = agent("session-a");
    let _open = marks.open("op-9", &plan_requester, 720, None);
    marks.add_child("op-9", 73, 730, 73, Arc::from(Vec::new()));
    let matcher = AgentMatcher::default();
    let checks = Checks {
        uid: UID,
        procs: &t,
        matcher: &matcher,
        daemon: Some((70, 700)),
        marks: Some(&marks),
        terminal_proof: true,
        orphans_marked: true,
    };
    let peer = AcceptedPeer {
        pid: 74,
        start_us: 740,
        accepted_us: 1_000_000,
    };

    let r = resolve(peer, &checks, Some(&marks)).expect("a marked descendant resolves");
    assert_eq!(r.via, ResolvedVia::Executor);
    let key = key_of(Some(&r), false, not_asked);
    assert_eq!(
        key,
        Some(RequesterKey::Agent("session-a".into())),
        "the descendant counts for the plan's requester, not for the agent above it"
    );

    let limits = Arc::new(RequesterLimits::default());
    let own = RequesterKey::Agent("session-a".into());
    let held: Vec<Slot> = (1..MAX_CONNECTIONS_PER_REQUESTER)
        .map(|_| limits.claim(own.clone()).expect("within the cap"))
        .collect();
    let descendant = limits
        .claim(key.expect("a key"))
        .expect("the requester's last slot");
    assert_eq!(
        limits.claim(own.clone()).err(),
        Some(Full),
        "the descendant took one of the requester's slots"
    );
    for _ in 0..gitraptor_api::mcp_view::MCP_WRITE_BURST {
        assert!(limits.take_write(&own));
    }
    assert!(
        !limits.take_write(descendant.key()),
        "the descendant has no write budget beyond its requester's"
    );
    drop(held);
}

/// The developer (an unattributed caller that passes the reserved checks) and the hook client
/// take no slot; the hook client's exemption needs a full connection of kind `other` from the
/// installed binary, so an MCP connection from that binary counts for whoever it resolves to.
#[test]
fn the_developer_and_the_hook_client_take_no_agent_slot() {
    let unattributed = resolution(Who::unattributed(), ResolvedVia::None);
    assert_eq!(key_of(Some(&unattributed), false, || true), None);
    assert_eq!(
        key_of(Some(&unattributed), false, || false),
        Some(RequesterKey::Unattributed),
        "an unattributed caller that fails the reserved checks shares one key"
    );
    assert_eq!(
        key_of(None, false, not_asked),
        Some(RequesterKey::Unattributed),
        "an unverified caller shares the unattributed key"
    );
    let unattributed_plan = resolution(Who::unattributed(), ResolvedVia::Executor);
    assert_eq!(
        key_of(Some(&unattributed_plan), false, not_asked),
        Some(RequesterKey::Unattributed),
        "a descendant of the executor is never the developer"
    );

    // The hook of an agent's commit resolves to the agent, and still takes no slot.
    let agent_r = resolution(agent("session-a"), ResolvedVia::Ancestry);
    assert!(is_hook_client(
        ConnectionProfile::Full,
        ClientKind::Other,
        true
    ));
    assert_eq!(key_of(Some(&agent_r), true, not_asked), None);
    for (profile, client, installed) in [
        (ConnectionProfile::Mcp, ClientKind::Other, true),
        (ConnectionProfile::Full, ClientKind::Cli, true),
        (ConnectionProfile::Mcp, ClientKind::Mcp, true),
        (ConnectionProfile::Full, ClientKind::Other, false),
    ] {
        let hook = is_hook_client(profile, client, installed);
        assert!(
            !hook,
            "{profile:?} {client:?} installed={installed} is not the hook client"
        );
        assert_eq!(
            key_of(Some(&agent_r), hook, not_asked),
            Some(RequesterKey::Agent("session-a".into())),
            "it counts, and spends, in the agent's key"
        );
    }
}

/// A key whose connections closed keeps its spent budget: reconnecting gives no fresh burst.
#[test]
fn a_key_keeps_its_spent_budget_after_its_connections_close() {
    let limits = Arc::new(RequesterLimits::default());
    let key = RequesterKey::Agent("session-a".into());
    let slot = limits.claim(key.clone()).expect("a slot");
    while limits.take_write(&key) {}
    drop(slot);
    let _again = limits.claim(key.clone()).expect("a slot");
    assert!(!limits.take_write(&key));
}
