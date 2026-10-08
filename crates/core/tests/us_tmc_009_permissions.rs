//! US-TMC-009, scenario outline on permissions: the base rule of
//! ADR-TMC-005 § 2 folded over every actor whose work a restore takes back.
//! Pure, so it runs on every system, Windows included: there an
//! unattributed requester over an agent's work is refused the same way,
//! since no confirmation exists yet (US-TMC-013).

use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_core::timemachine::oplog::{Channel, Requester, RequesterOrigin};
use gitraptor_core::timemachine::restore::restore_permission;

fn agent(name: &str, session: &str) -> Requester {
    Requester::Agent {
        name: name.into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn claude1() -> Requester {
    agent("claude-1", "101:1")
}

fn claude2() -> Requester {
    agent("claude-2", "202:1")
}

/// claude-2 over claude-1's work: `other-actor` on every channel, never a
/// confirmation; and `other-actor` wins over any confirmation.
#[test]
fn another_agent_is_rejected_without_confirmation() {
    for channel in [Channel::Cli, Channel::Mcp] {
        assert_eq!(
            restore_permission(&claude2(), channel, &[claude1()]),
            Err(TmRejectReason::OtherActor),
            "{channel:?}"
        );
        assert_eq!(
            restore_permission(&claude2(), channel, &[Requester::Unattributed, claude1()]),
            Err(TmRejectReason::OtherActor),
            "{channel:?}"
        );
    }
}

/// An unattributed requester over an agent's work needs a confirmation
/// (rejected until it exists); over MCP an unattributed requester is
/// refused even with nothing to take back.
#[test]
fn unattributed_over_agent_work_needs_a_confirmation() {
    assert_eq!(
        restore_permission(
            &Requester::Unattributed,
            Channel::Cli,
            &[Requester::Unattributed, claude1()]
        ),
        Err(TmRejectReason::ConfirmationRequired)
    );
    assert_eq!(
        restore_permission(&Requester::Unattributed, Channel::Mcp, &[]),
        Err(TmRejectReason::OtherActor)
    );
}

/// Taking back one's own work needs nothing; neither does a restore that
/// takes back nobody's work.
#[test]
fn own_work_needs_no_confirmation() {
    let un = Requester::Unattributed;
    assert_eq!(
        restore_permission(&un, Channel::Cli, &[un.clone(), un.clone(), un.clone()]),
        Ok(())
    );
    assert_eq!(
        restore_permission(&claude1(), Channel::Mcp, &[claude1(), claude1()]),
        Ok(())
    );
    for who in [un.clone(), claude1(), claude2()] {
        assert_eq!(
            restore_permission(&who, Channel::Cli, &[]),
            Ok(()),
            "{who:?}"
        );
    }
}
