//! US-GRD-017 · the pure conversion: a failed prior turns an allowed decision into a denial
//! (D4, D5, D6, D7, D11, D2). Decisions are built by hand. Red until `snapshot_required` has
//! bodies.

use gitraptor_api::guard::{
    Cause, CommitStage, ConfigSource, ConfigStatus, Decision, Effect, ExceptionState, Level,
    Operation, Reason, RefUpdate, RefValue, Rule,
};
use gitraptor_api::timemachine::{HookPriorFailure, HookPriorSnapshot};

use super::snapshot_required::{
    Granted, cause, client_fallback, degraded_reason, enforce, legacy_decision,
};

const ALL_FAILURES: [(HookPriorFailure, Cause); 7] = [
    (HookPriorFailure::TimeLimit, Cause::SnapshotTimeLimit),
    (HookPriorFailure::NoSpace, Cause::SnapshotNoSpace),
    (
        HookPriorFailure::QuotaExceeded,
        Cause::SnapshotQuotaExceeded,
    ),
    (HookPriorFailure::Discarded, Cause::SnapshotDiscarded),
    (HookPriorFailure::NoWorktree, Cause::SnapshotNoWorktree),
    (HookPriorFailure::Unavailable, Cause::SnapshotUnavailable),
    (HookPriorFailure::Internal, Cause::SnapshotInternal),
];

fn allowed(prior: Option<HookPriorSnapshot>) -> Decision {
    Decision {
        decision_id: "d-fixed-id".into(),
        effect: Effect::Allow,
        applied_effect: Effect::Allow,
        reasons: Vec::new(),
        exception: ExceptionState::None,
        config_status: vec![ConfigSource {
            source: Level::Floor,
            status: ConfigStatus::Ok,
        }],
        config_ref: vec!["blob-1".into()],
        notices: Vec::new(),
        prior_snapshot: prior,
    }
}

fn failed(f: HookPriorFailure) -> Option<HookPriorSnapshot> {
    Some(HookPriorSnapshot::Failed { cause: f })
}

fn complete() -> Option<HookPriorSnapshot> {
    Some(HookPriorSnapshot::Complete {
        snapshot_id: "s-1".into(),
        reused: false,
    })
}

fn snapshot_reasons(d: &Decision) -> Vec<&Reason> {
    d.reasons
        .iter()
        .filter(|r| r.rule == Rule::SnapshotFailed)
        .collect()
}

fn branch_delete() -> Operation {
    Operation::RefTransaction {
        updates: vec![RefUpdate {
            refname: "refs/heads/feat-x".into(),
            old: RefValue::Oid("1".repeat(40)),
            new: RefValue::Zero,
        }],
        orphan_head: None,
    }
}

fn commit() -> Operation {
    Operation::Commit {
        stage: CommitStage::PreCommit,
    }
}

#[test]
fn every_failure_cause_denies() {
    for (failure, expected) in ALL_FAILURES {
        assert_eq!(cause(failure), expected, "{failure:?}");
        let mut d = allowed(failed(failure));
        enforce(&mut d);
        assert_eq!(d.effect, Effect::Deny, "{failure:?}");
        assert_eq!(d.applied_effect, Effect::Deny, "{failure:?}");
        let reasons = snapshot_reasons(&d);
        assert_eq!(reasons.len(), 1, "{failure:?}: {:?}", d.reasons);
        assert_eq!(reasons[0].level, Level::System);
        assert_eq!(reasons[0].cause, Some(expected), "{failure:?}");
        assert!(d.notices.is_empty());
    }
}

/// Guard 3.1: the decision is converted in place, never replaced by `system_deny`.
#[test]
fn enforce_keeps_the_decision_id_and_config_status() {
    let mut d = allowed(failed(HookPriorFailure::TimeLimit));
    let before = d.clone();
    enforce(&mut d);
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(d.decision_id, before.decision_id);
    assert_eq!(d.config_status, before.config_status);
    assert_eq!(d.config_ref, before.config_ref);
    assert_eq!(d.prior_snapshot, before.prior_snapshot);
    assert_eq!(
        snapshot_reasons(&d)[0].cause,
        Some(Cause::SnapshotTimeLimit)
    );
}

/// D5: with the exception applied the operation goes ahead, and the skipped rule is recorded.
#[test]
fn exception_keeps_allow_and_records_the_skipped_rule() {
    let mut d = allowed(failed(HookPriorFailure::NoSpace));
    d.exception = ExceptionState::Applied;
    enforce(&mut d);
    assert_eq!(d.effect, Effect::Deny);
    assert_eq!(d.applied_effect, Effect::Allow);
    assert_eq!(d.exception, ExceptionState::Applied);
    let reasons = snapshot_reasons(&d);
    assert_eq!(reasons.len(), 1, "{:?}", d.reasons);
    assert_eq!(reasons[0].cause, Some(Cause::SnapshotNoSpace));
}

#[test]
fn only_an_allowed_failure_converts() {
    // complete, and no field: nothing changes.
    for prior in [complete(), None] {
        let mut d = allowed(prior);
        let before = d.clone();
        enforce(&mut d);
        assert_eq!(d, before);
    }
    // A denial that already stands is not touched, whatever the prior says.
    let mut d = allowed(failed(HookPriorFailure::Internal));
    d.effect = Effect::Deny;
    d.applied_effect = Effect::Deny;
    d.reasons.push(Reason {
        rule: Rule::MinimumBaseBranchDelete,
        level: Level::Minimum,
        cause: Some(Cause::Delete),
        params: Vec::new(),
    });
    let before = d.clone();
    enforce(&mut d);
    assert_eq!(d, before);
}

/// Guard 3.2: the compatibility mapping is a denial without cause and without the prior field.
#[test]
fn legacy_decision_stays_a_denial_without_prior_field() {
    for (failure, _) in ALL_FAILURES {
        let mut d = allowed(failed(failure));
        enforce(&mut d);
        legacy_decision(&mut d);
        assert_eq!(d.effect, Effect::Deny, "{failure:?}");
        assert_eq!(d.applied_effect, Effect::Deny, "{failure:?}");
        assert!(d.prior_snapshot.is_none());
        assert!(snapshot_reasons(&d).is_empty(), "{:?}", d.reasons);
        let internal: Vec<&Reason> = d
            .reasons
            .iter()
            .filter(|r| r.rule == Rule::InternalError)
            .collect();
        assert_eq!(internal.len(), 1, "{:?}", d.reasons);
        assert_eq!(internal[0].cause, None);
        assert_eq!(internal[0].level, Level::System);
    }
}

fn denied_by_client(d: &Decision) -> bool {
    d.effect == Effect::Deny && d.applied_effect == Effect::Deny && snapshot_reasons(d).len() == 1
}

/// D11: what the hook client does by what the daemon granted.
#[test]
fn client_fallback_by_granted_capabilities() {
    let both = Granted {
        policies: true,
        prior_snapshot: true,
        snapshot_required: true,
    };
    let prior_only = Granted {
        snapshot_required: false,
        ..both
    };
    let neither = Granted {
        prior_snapshot: false,
        snapshot_required: false,
        ..both
    };

    // Both: the daemon already decided; nothing to do, even for an allowed failed prior.
    let mut d = allowed(failed(HookPriorFailure::TimeLimit));
    let before = d.clone();
    client_fallback(&branch_delete(), &mut d, both);
    assert_eq!(d, before);

    // Prior only: the client converts a failed prior (with its cause)...
    let mut d = allowed(failed(HookPriorFailure::TimeLimit));
    client_fallback(&branch_delete(), &mut d, prior_only);
    assert!(denied_by_client(&d), "{d:?}");
    assert_eq!(
        snapshot_reasons(&d)[0].cause,
        Some(Cause::SnapshotTimeLimit)
    );
    // ...and leaves a complete prior alone.
    let mut d = allowed(complete());
    let before = d.clone();
    client_fallback(&branch_delete(), &mut d, prior_only);
    assert_eq!(d, before);

    // Neither: an allowed operation that wants a prior is denied, snapshot-unavailable.
    for op in [
        branch_delete(),
        Operation::Rebase {
            upstream: None,
            branch: None,
        },
    ] {
        let mut d = allowed(None);
        client_fallback(&op, &mut d, neither);
        assert!(denied_by_client(&d), "{op:?}: {d:?}");
        assert_eq!(
            snapshot_reasons(&d)[0].cause,
            Some(Cause::SnapshotUnavailable)
        );
    }

    // A non-destructive operation is never touched, with any combination.
    for granted in [both, prior_only, neither] {
        let mut d = allowed(None);
        let before = d.clone();
        client_fallback(&commit(), &mut d, granted);
        assert_eq!(d, before, "{granted:?}");
    }
}

#[test]
fn degraded_reason_is_snapshot_unavailable() {
    let reason = degraded_reason();
    assert_eq!(reason.rule, Rule::SnapshotFailed);
    assert_eq!(reason.level, Level::System);
    assert_eq!(reason.cause, Some(Cause::SnapshotUnavailable));
}
