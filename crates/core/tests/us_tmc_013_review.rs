//! Review findings of US-TMC-013 over the channel: what a restore's challenge is bound to.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use gitraptor_api::methods;
use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_core::channel::TestConfirmation;
use gitraptor_core::timemachine::oplog::OperationKind;
use gitraptor_testkit::diff;
use us_tmc_013_common::*;

/// The same agent adds one more operation in the same scope after the challenge: the plan the
/// token was issued for is not the plan that would run, so the restore does not discard work
/// the human was not shown.
#[test]
fn a_restore_token_dies_when_the_same_agent_adds_an_operation_after_the_point() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    std::fs::write(r.worktree.join("a.rs"), "fn a() { antes_del_punto(); }\n").unwrap();
    let point = r.reset_hard().prior_snapshot_id;
    r.agent_work(AGENT_SESSION);
    let mut c = r.client();
    let token = token_of(&confirm_data(c.call(
        methods::TM_RESTORE,
        restore_params(&r.worktree, &point, None),
    )));
    // Same owner, same scope: only the set of operations the restore takes back grew.
    r.agent_work(AGENT_SESSION);
    let before = r.fx.fingerprint();

    let data = confirm_data(c.call(
        methods::TM_RESTORE,
        restore_params(&r.worktree, &point, Some(&token)),
    ));

    assert_eq!(data.reason, TmRejectReason::ChallengeInvalid);
    assert!(diff(&before, &r.fx.fingerprint()).is_empty());
    let last = r.requests(OperationKind::Restore).pop().unwrap();
    assert_eq!(last.reason.as_deref(), Some("challenge-invalid"));
}
