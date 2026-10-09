//! US-TMC-013 end to end over the channel: an unattributed requester (the CLI) takes back an
//! agent's work with the one-use challenge of ADR-TMC-005 § 3. A real daemon in this process,
//! real Git, temporary repos and profile (NFR-01). Debug builds only: the confirmation seam does
//! not exist in release (`TestConfirmation`).
//!
//! In-process clients descend from the daemon, so they never pass the confirmation checks by
//! themselves: the positive path uses `TestConfirmation::Eligible`, which replaces only that
//! result; resolution, binding to the connection and process, the plan hash, the time limit and
//! the oplog are the real ones.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use std::path::Path;

use gitraptor_api::actor::Actor;
use gitraptor_api::messages::RefusalReason;
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code};
use gitraptor_api::timemachine::{TmRejectReason, TmRejectedData, UndoResult};
use gitraptor_api::{capability, methods};
use gitraptor_core::channel::{ChannelConfig, TestConfirmation};
use gitraptor_core::daemon::DaemonConfig;
use gitraptor_core::timemachine::oplog::{Channel, OperationKind, OperationState, Requester};
use gitraptor_testkit::diff;
use serde_json::{Value, json};
use us_tmc_013_common::*;

const FORGED: &str = "0123456789abcdef0123456789abcdef";

fn undo(
    c: &mut gitraptor_core::client::Client,
    wt: &Path,
    token: Option<&str>,
) -> Result<Value, gitraptor_core::client::ClientError> {
    c.call(methods::TM_UNDO, params(wt, token))
}

/// A running daemon with the agent's work on top of the stack and the repo as it stands then.
fn with_agent_work(seam: Option<TestConfirmation>) -> (Running, AgentWork) {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, seam, None);
    let work = r.agent_work(AGENT_SESSION);
    (r, work)
}

fn assert_intact(r: &Running, before: &gitraptor_testkit::fingerprint::Snapshot) {
    let changes = diff(before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "the repo changed: {changes:#?}");
}

/// Escenario 3: "unattributed" confirms and the agent's work is taken back; the record says
/// "unattributed".
#[test]
fn s3_an_unattributed_requester_confirms_and_the_agents_work_is_undone() {
    let (r, work) = with_agent_work(Some(TestConfirmation::Eligible));
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let first = confirm_data(undo(&mut c, &r.worktree, None));

    assert_eq!(first.reason, TmRejectReason::ConfirmationRequired);
    let challenge = first.challenge.clone().expect("a challenge");
    assert_eq!(challenge.token.len(), 32);
    assert!(challenge.token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(challenge.expires_in_ms, 60_000);
    assert_eq!(first.cannot_confirm, None);
    assert_eq!(first.owners.len(), 1);
    assert!(matches!(first.owners[0], Actor::Agent { .. }));
    assert_eq!(
        first.undone_operation_id.as_deref(),
        Some(work.operation_id.as_str())
    );
    assert_intact(&r, &before);

    let done: UndoResult =
        serde_json::from_value(undo(&mut c, &r.worktree, Some(&challenge.token)).unwrap()).unwrap();

    assert_eq!(done.requester.actor, Actor::Unattributed);
    assert_eq!(done.undone_operation_id, work.operation_id);
    assert_eq!(r.a_rs(), work.dirty, "the agent's reset is taken back");
    let requests = r.requests(OperationKind::Undo);
    assert_eq!(requests.len(), 2, "one rejected request and one undo");
    let (asked, ran) = (&requests[0], &requests[1]);
    assert_eq!(asked.view.state, OperationState::Rejected);
    assert_eq!(asked.reason.as_deref(), Some("confirmation-required"));
    assert!(!asked.view.record.confirmed);
    assert_eq!(ran.view.state, OperationState::Finished);
    assert!(ran.view.record.confirmed);
    assert_eq!(ran.view.record.requester, Requester::Unattributed);
    assert_eq!(ran.view.record.channel, Channel::Cli);
}

/// Escenario 4: no confirmation, no undo.
#[test]
fn s4_without_the_token_there_is_no_undo() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::Eligible));
    let before = r.fx.fingerprint();
    let mut c = r.client();

    for _ in 0..2 {
        let data = confirm_data(undo(&mut c, &r.worktree, None));
        assert_eq!(data.reason, TmRejectReason::ConfirmationRequired);
        assert!(data.challenge.is_some());
    }

    assert_intact(&r, &before);
    let requests = r.requests(OperationKind::Undo);
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|q| q.view.state == OperationState::Rejected)
    );
}

/// Escenario 4: a caller that cannot confirm is told why and gets no challenge. Without the
/// seam the in-process client is the daemon's own process: its ancestry resolves as
/// unattributed and not confirmable, so `confirmation_refusal` answers through its
/// `!confirmable` arm (`agent-ancestry`).
#[test]
fn s4_a_caller_that_cannot_confirm_gets_no_challenge() {
    let cases = [
        (None, RefusalReason::AgentAncestry),
        (
            Some(TestConfirmation::Refused(
                RefusalReason::NoControllingTerminal,
            )),
            RefusalReason::NoControllingTerminal,
        ),
        (
            Some(TestConfirmation::Refused(RefusalReason::AgentAncestry)),
            RefusalReason::AgentAncestry,
        ),
    ];
    for (seam, why) in cases {
        let (r, _work) = with_agent_work(seam);
        let before = r.fx.fingerprint();
        let mut c = r.client();

        let data = confirm_data(undo(&mut c, &r.worktree, None));

        assert_eq!(data.reason, TmRejectReason::ConfirmationRequired, "{why:?}");
        assert_eq!(data.cannot_confirm, Some(why));
        assert_eq!(data.challenge, None, "{why:?}");
        assert!(
            !data.owners.is_empty(),
            "the owners are listed all the same"
        );
        assert_intact(&r, &before);
        let requests = r.requests(OperationKind::Undo);
        assert_eq!(requests.len(), 1, "{why:?}");
        assert_eq!(requests[0].reason.as_deref(), Some("confirmation-required"));
    }
}

/// Escenario 5: where the business rule does not offer confirming another actor's work
/// (Windows, BR-TMC-AUTH-001) the request is rejected without a challenge. Played on every OS
/// with the seam.
#[test]
fn s5_on_windows_the_request_is_rejected_without_asking() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::RuleForbids));
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let (error, data) = rejected(undo(&mut c, &r.worktree, None));

    assert_eq!(error, code::OPERATION_REJECTED);
    let data = data.expect("data");
    assert_eq!(data["reason"], "confirmation-unavailable");
    assert!(data.get("challenge").is_none(), "no challenge: {data}");
    assert_intact(&r, &before);
    let requests = r.requests(OperationKind::Undo);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].reason.as_deref(),
        Some("confirmation-unavailable")
    );
}

/// Escenario 6: over MCP an unattributed requester is always rejected, and `confirmation` is
/// not a parameter MCP has.
#[test]
fn s6_unattributed_over_mcp_is_always_rejected() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    r.reset_hard();
    let before = r.fx.fingerprint();
    let records = r.recorded().len();
    let mut mcp = r.mcp_client();

    let (error, data) = rejected(mcp.call(methods::TM_UNDO, json!({})));

    assert_eq!(error, code::SCOPE_REFUSED);
    let data: ScopeRefusedData = serde_json::from_value(data.expect("data")).unwrap();
    assert_eq!(data.reason, ScopeRefusal::UnattributedOverMcp);
    let (error, _) = rejected(mcp.call(methods::TM_UNDO, json!({ "confirmation": FORGED })));
    assert_eq!(error, code::INVALID_PARAMS);
    assert_intact(&r, &before);
    assert_eq!(r.recorded().len(), records, "no new record");
}

/// A confirmed token cannot be used again.
#[test]
fn sec_a_reused_token_is_rejected() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::Eligible));
    let mut c = r.client();
    let token = token_of(&confirm_data(undo(&mut c, &r.worktree, None)));
    undo(&mut c, &r.worktree, Some(&token)).unwrap();
    let after_undo = r.fx.fingerprint();

    let again = confirm_data(undo(&mut c, &r.worktree, Some(&token)));

    assert_eq!(again.reason, TmRejectReason::ChallengeInvalid);
    assert_eq!(again.challenge, None);
    assert_intact(&r, &after_undo);
    let last = r.requests(OperationKind::Undo).pop().unwrap();
    assert_eq!(last.reason.as_deref(), Some("challenge-invalid"));
}

/// A token is for the connection that got it, and the attempt burns it.
#[test]
fn sec_a_token_of_another_connection_is_rejected() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::Eligible));
    let before = r.fx.fingerprint();
    let mut a = r.client();
    let mut b = r.client();
    let token = token_of(&confirm_data(undo(&mut a, &r.worktree, None)));

    let stolen = confirm_data(undo(&mut b, &r.worktree, Some(&token)));
    assert_eq!(stolen.reason, TmRejectReason::ChallengeInvalid);
    let owner = confirm_data(undo(&mut a, &r.worktree, Some(&token)));
    assert_eq!(owner.reason, TmRejectReason::ChallengeInvalid, "consumed");

    assert_intact(&r, &before);
}

/// What was shown is what runs, or nothing: another operation of the agent on top changes the
/// plan.
#[test]
fn sec_a_changed_plan_invalidates_the_token() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::Eligible));
    let mut c = r.client();
    let token = token_of(&confirm_data(undo(&mut c, &r.worktree, None)));
    r.agent_work(AGENT_SESSION);
    let before = r.fx.fingerprint();

    let data = confirm_data(undo(&mut c, &r.worktree, Some(&token)));

    assert_eq!(data.reason, TmRejectReason::ChallengeInvalid);
    assert_intact(&r, &before);
}

/// Forged tokens never confirm; malformed ones do not even reach the daemon's checks.
#[test]
fn sec_a_forged_or_malformed_token_is_rejected() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::Eligible));
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let forged = confirm_data(undo(&mut c, &r.worktree, Some(FORGED)));
    assert_eq!(forged.reason, TmRejectReason::ChallengeInvalid);
    let records = r.recorded().len();
    let (error, _) = rejected(undo(&mut c, &r.worktree, Some("xyz")));

    assert_eq!(error, code::INVALID_PARAMS);
    assert_eq!(
        r.recorded().len(),
        records,
        "a malformed token leaves no record"
    );
    assert_intact(&r, &before);
}

/// Where no confirmation is needed a token is still an error: the plan changed under it.
#[test]
fn sec_a_token_where_no_confirmation_is_needed_is_rejected() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    r.reset_hard();
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(undo(&mut c, &r.worktree, Some(FORGED)));

    assert_eq!(data.reason, TmRejectReason::ChallengeInvalid);
    assert_intact(&r, &before);
}

/// The challenge's token is never written anywhere the daemon keeps: not the oplog, not the
/// logs, not the profile.
#[test]
fn the_token_never_reaches_the_oplog_or_the_profile() {
    let (r, _work) = with_agent_work(Some(TestConfirmation::Eligible));
    let mut c = r.client();
    let token = token_of(&confirm_data(undo(&mut c, &r.worktree, None)));
    undo(&mut c, &r.worktree, Some(&token)).unwrap();
    let wrong = confirm_data(undo(&mut c, &r.worktree, Some(FORGED)));
    assert_eq!(wrong.reason, TmRejectReason::ChallengeInvalid);

    for q in r.recorded() {
        let log = r.oplog.lock().unwrap();
        let journal = log.journal(&q.view.record.operation_id).unwrap();
        let text = format!("{:?} {journal:?}", q.view);
        assert!(!text.contains(&token), "the token is in the oplog");
        assert!(!text.contains(FORGED), "a presented token is in the oplog");
    }
    let mut files = vec![r.tp.root.path().to_owned()];
    while let Some(dir) = files.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let hits =
                    |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
                assert!(!hits(&token), "the token is in {}", path.display());
            }
        }
    }
}

/// A connection that did not get the capability keeps the old shape and cannot send the token.
#[test]
fn cap_a_connection_without_the_capability_keeps_the_old_shape() {
    let (fx, wt) = repo_with_login();
    let served = capability::all()
        .map(|c| c.name)
        .filter(|n| *n != methods::CAP_TM_CONFIRMATION.name)
        .collect();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), Some(served));
    r.agent_work(AGENT_SESSION);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let (error, data) = rejected(undo(&mut c, &r.worktree, None));

    assert_eq!(error, code::OPERATION_REJECTED);
    let old: TmRejectedData = serde_json::from_value(data.expect("data")).unwrap();
    assert_eq!(old.reason, TmRejectReason::ConfirmationRequired);
    let (error, _) = rejected(undo(&mut c, &r.worktree, Some(FORGED)));
    assert_eq!(error, code::INVALID_PARAMS);
    assert_intact(&r, &before);
}

/// The seam does not exist in production: the daemon the CLI starts leaves it off.
#[test]
fn a3_the_production_config_leaves_the_test_seam_off() {
    assert_eq!(ChannelConfig::default().test_confirmation, None);
    assert_eq!(
        DaemonConfig::for_current_user()
            .unwrap()
            .channel
            .test_confirmation,
        None
    );
}
