//! US-TMC-013 for `timemachine.restore`: the same one-use challenge when the point's way forward
//! takes back an agent's work. Real daemon in this process, real Git, temporary repos and
//! profile (NFR-01). The agent's work comes after the point, as in `us_tmc_009.rs`.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use gitraptor_api::actor::Actor;
use gitraptor_api::messages::RefusalReason;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_core::channel::TestConfirmation;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::timemachine::oplog::{OperationKind, OperationState, Requester};
use gitraptor_testkit::diff;
use serde_json::Value;
use us_tmc_013_common::*;

/// A daemon where the agent's work is after `point`.
fn with_agent_after_a_point(seam: TestConfirmation) -> (Running, String, AgentWork) {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(seam), None);
    // The first reset's prior is the point to return to.
    std::fs::write(r.worktree.join("a.rs"), "fn a() { antes_del_punto(); }\n").unwrap();
    let point = r.reset_hard().prior_snapshot_id;
    let work = r.agent_work(AGENT_SESSION);
    (r, point, work)
}

fn restore(
    c: &mut Client,
    r: &Running,
    point: &str,
    token: Option<&str>,
) -> Result<Value, ClientError> {
    c.call(
        methods::TM_RESTORE,
        restore_params(&r.worktree, point, token),
    )
}

#[test]
fn restore_s3_a_confirmed_restore_takes_back_the_agents_work() {
    let (r, point, _work) = with_agent_after_a_point(TestConfirmation::Eligible);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let first = confirm_data(restore(&mut c, &r, &point, None));

    assert_eq!(first.reason, TmRejectReason::ConfirmationRequired);
    let challenge = first.challenge.clone().expect("a challenge");
    assert_eq!(challenge.token.len(), 32);
    assert_eq!(challenge.expires_in_ms, 60_000);
    assert_eq!(first.owners.len(), 1);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());

    let done = restore(&mut c, &r, &point, Some(&challenge.token)).unwrap();

    assert_eq!(
        done["requester"]["actor"],
        serde_json::to_value(Actor::Unattributed).unwrap()
    );
    assert_eq!(r.a_rs(), "fn a() { antes_del_punto(); }\n");
    let requests = r.requests(OperationKind::Restore);
    let ran = requests.last().expect("the restore");
    assert_eq!(ran.view.state, OperationState::Finished);
    assert!(ran.view.record.confirmed);
    assert_eq!(ran.view.record.requester, Requester::Unattributed);
}

#[test]
fn restore_s4_without_the_token_there_is_no_restore() {
    let (r, point, _work) = with_agent_after_a_point(TestConfirmation::Eligible);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(restore(&mut c, &r, &point, None));

    assert_eq!(data.reason, TmRejectReason::ConfirmationRequired);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
    let requests = r.requests(OperationKind::Restore);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].view.state, OperationState::Rejected);
    assert_eq!(requests[0].reason.as_deref(), Some("confirmation-required"));
}

/// Where the rule does not offer it (Windows) the answer is the rule's, with no challenge.
#[test]
fn restore_s5_on_windows_it_is_rejected_without_asking() {
    let (r, point, _work) = with_agent_after_a_point(TestConfirmation::RuleForbids);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let (error, data) = rejected(restore(&mut c, &r, &point, None));

    assert_eq!(error, code::OPERATION_REJECTED);
    let data = data.expect("data");
    assert_eq!(data["reason"], "confirmation-unavailable");
    assert!(data.get("challenge").is_none(), "no challenge: {data}");
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
}

/// A caller refused by the checks is told why and gets no challenge.
#[test]
fn restore_s4_a_caller_that_cannot_confirm_gets_no_challenge() {
    let (r, point, _work) = with_agent_after_a_point(TestConfirmation::Refused(
        RefusalReason::NoControllingTerminal,
    ));
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(restore(&mut c, &r, &point, None));

    assert_eq!(
        data.cannot_confirm,
        Some(RefusalReason::NoControllingTerminal)
    );
    assert_eq!(data.challenge, None);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
}

#[test]
fn restore_sec_a_changed_plan_invalidates_the_token() {
    let (r, point, _work) = with_agent_after_a_point(TestConfirmation::Eligible);
    let mut c = r.client();
    let token = token_of(&confirm_data(restore(&mut c, &r, &point, None)));
    // Another agent's work lands after the point: the plan's owners change.
    r.agent_work("5151:1");
    let before = r.fx.fingerprint();

    let data = confirm_data(restore(&mut c, &r, &point, Some(&token)));

    assert_eq!(data.reason, TmRejectReason::ChallengeInvalid);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
    let last = r.requests(OperationKind::Restore).pop().unwrap();
    assert_eq!(last.reason.as_deref(), Some("challenge-invalid"));
}

/// A token of a restore does not confirm anything on another connection, and burns.
#[test]
fn restore_sec_a_token_of_another_connection_is_rejected() {
    let (r, point, _work) = with_agent_after_a_point(TestConfirmation::Eligible);
    let before = r.fx.fingerprint();
    let mut a = r.client();
    let mut b = r.client();
    let token = token_of(&confirm_data(restore(&mut a, &r, &point, None)));

    let stolen = confirm_data(restore(&mut b, &r, &point, Some(&token)));
    assert_eq!(stolen.reason, TmRejectReason::ChallengeInvalid);
    let owner = confirm_data(restore(&mut a, &r, &point, Some(&token)));
    assert_eq!(owner.reason, TmRejectReason::ChallengeInvalid);

    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
}
