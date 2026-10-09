//! What the plan hash covers beyond the fields of `PlanFacts`.

use super::*;
use crate::timemachine::oplog::RequesterOrigin;

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

/// A restore plan: no single undone operation, the owners and what it takes back apart.
fn restore<'a>(
    requester: &'a Requester,
    scope: &'a Scope,
    owners: &'a [Requester],
) -> PlanFacts<'a> {
    PlanFacts {
        kind: PlanKind::Restore,
        worktree: "/repo/feat-login",
        requester,
        channel: Channel::Cli,
        target_snapshot: "snap-1",
        undone_id: None,
        undone_subtype: None,
        scope,
        owners,
    }
}

#[test]
fn two_restores_differing_only_in_an_operation_after_the_point_hash_differently() {
    let who = Requester::Unattributed;
    let scope = Scope {
        worktrees: vec!["/repo/feat-login".into()],
        refs: Vec::new(),
    };
    // The same agent, the same scope: the only difference is one more operation of its own.
    let owners = [agent("4242:1")];
    let plan = restore(&who, &scope, &owners);

    let shown = plan.hash_over(&["op-1"]);
    let grown = plan.hash_over(&["op-1", "op-2"]);

    assert_ne!(shown, grown);
    assert_ne!(shown, plan.hash(), "what is taken back is part of the plan");
    // The same set said differently is the same plan.
    assert_eq!(grown, plan.hash_over(&["op-2", "op-1", "op-2"]));
}
