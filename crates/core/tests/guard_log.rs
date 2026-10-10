//! US-GRD-005 (DS-US-GRD-005 T001 and T002): the Guardrails decision log in the repo store —
//! what is logged, aggregation, the insert cap, retention and the KPI — with the clock injected,
//! over a temporary profile (NFR-01).

mod common;

use common::*;
use gitraptor_api::AgentKind;
use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    CommitStage, Decision, Effect, EvaluateParams, ExceptionState, Hook, Level, LogDetail, LogKind,
    LoggedOperation, Operation, Param, ParamKind, PushUpdate, Reason, RefChange, RefValue, Rule,
};
use gitraptor_core::guardrails::log::{LogContext, LogEntry, entry, sanitize_remote};
use gitraptor_core::profile::RepoStore;

const MINUTE: i64 = 60_000;
const DAY: i64 = 24 * 60 * MINUTE;
const NOW: i64 = 1_800_000_000_000;

fn store() -> (TempProfile, tempfile::TempDir, RepoStore) {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "demo", true);
    let mut profile = tp.open();
    let (repo_entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (store, _) = profile.open_store(&repo_entry.repo_id).unwrap();
    (tp, repos, store)
}

fn oid(c: char) -> RefValue {
    RefValue::Oid(c.to_string().repeat(40))
}

fn force_push(branch: &str, remote: &str) -> EvaluateParams {
    EvaluateParams {
        repo_id: "r".into(),
        common_dir: "/tmp/demo/.git".into(),
        hook: Hook::PrePush,
        operation: Operation::Push {
            remote: Untrusted::new(remote),
            updates: vec![
                PushUpdate {
                    local_ref: Some(format!("refs/heads/{branch}")),
                    local: oid('a'),
                    remote_ref: format!("refs/heads/{branch}"),
                    remote: oid('b'),
                },
                PushUpdate {
                    local_ref: Some("refs/heads/other".into()),
                    local: oid('c'),
                    remote_ref: "refs/heads/other".into(),
                    remote: RefValue::Zero,
                },
            ],
        },
        authorship: None,
    }
}

fn denied(rule: Rule, params: Vec<Param>) -> Decision {
    Decision {
        decision_id: "d1".into(),
        effect: Effect::Deny,
        applied_effect: Effect::Deny,
        reasons: vec![Reason {
            rule,
            level: Level::Minimum,
            cause: None,
            params,
        }],
        exception: ExceptionState::None,
        config_status: vec![],
        config_ref: vec![],
        notices: vec![],
        prior_snapshot: None,
    }
}

fn ctx(at_ms: i64, branch: &str) -> LogContext {
    LogContext {
        actor: Some(AgentKind::ClaudeCode),
        under_executor: false,
        worktree: Some("/tmp/demo".into()),
        branch: Some(branch.into()),
        authorship_policy: None,
        at_ms,
        utc_offset_s: 0,
    }
}

fn denial_at(at_ms: i64, branch: &str) -> LogEntry {
    entry(
        &force_push(branch, "origin"),
        &denied(
            Rule::MinimumForcePush,
            vec![Param::new(ParamKind::Branch, branch)],
        ),
        &ctx(at_ms, branch),
    )
    .expect("a denial is logged")
}

#[test]
fn identical_denials_aggregate_into_one_row() {
    let (_tp, _repos, mut store) = store();
    for i in 0..1000 {
        store
            .record_guard_decision(&denial_at(NOW + i * 30, "main"))
            .unwrap();
    }
    let log = store
        .guard_log(NOW - DAY, 500, NOW + MINUTE, false)
        .unwrap();
    assert_eq!(log.entries.len(), 1);
    assert_eq!(log.entries[0].count, 1000);
    assert_eq!(log.entries[0].at_ms, NOW);
    assert_eq!(log.summary.blocked, 1000);
}

#[test]
fn excess_goes_to_rate_limited_rows_and_still_counts() {
    let (_tp, _repos, mut store) = store();
    for i in 0..500 {
        store
            .record_guard_decision(&denial_at(NOW + i, &format!("b{i}")))
            .unwrap();
    }
    let log = store
        .guard_log(NOW - DAY, 500, NOW + MINUTE, false)
        .unwrap();
    let full = log
        .entries
        .iter()
        .filter(|e| e.detail == LogDetail::Full)
        .count();
    assert_eq!(full, 100, "100 new rows per minute");
    assert!(
        log.entries
            .iter()
            .any(|e| e.detail == LogDetail::RateLimited)
    );
    assert_eq!(log.summary.blocked, 500, "the KPI keeps every occurrence");
    assert_eq!(log.summary.rate_limited, 400);
}

#[test]
fn entries_expire_after_90_days() {
    let (_tp, _repos, mut store) = store();
    store
        .record_guard_decision(&denial_at(NOW - 91 * DAY, "old"))
        .unwrap();
    store
        .record_guard_decision(&denial_at(NOW - 89 * DAY, "recent"))
        .unwrap();
    let log = store.guard_log(0, 500, NOW, false).unwrap();
    assert_eq!(log.entries.len(), 1, "the query filters before the purge");
    assert_eq!(log.entries[0].branch.as_ref().unwrap().raw(), "recent");
    assert_eq!(store.purge_guard_log(NOW).unwrap(), 1);
    assert_eq!(
        store.guard_log(0, 500, NOW, false).unwrap().entries.len(),
        1
    );
}

#[test]
fn kpi_counts_the_period_only() {
    let (_tp, _repos, mut store) = store();
    for (i, at) in [NOW - DAY, NOW - 2 * DAY, NOW - 3 * DAY, NOW - 8 * DAY]
        .into_iter()
        .enumerate()
    {
        store
            .record_guard_decision(&denial_at(at, &format!("b{i}")))
            .unwrap();
    }
    let week = store.guard_log(NOW - 7 * DAY, 500, NOW, false).unwrap();
    assert_eq!(week.summary.blocked, 3);
    assert_eq!(week.entries.len(), 3);
}

#[test]
fn notices_do_not_count() {
    let (_tp, _repos, mut store) = store();
    let params = EvaluateParams {
        hook: Hook::CommitMsg,
        operation: Operation::Commit {
            stage: CommitStage::CommitMsg,
        },
        authorship: Some(gitraptor_api::guard::AuthorshipFacts {
            coauthors: vec![Some(AgentKind::ClaudeCode), None],
            trailer_table: 1,
            unreadable: false,
        }),
        ..force_push("main", "origin")
    };
    let mut decision = denied(Rule::AuthorshipHumanAuthor, vec![]);
    decision.effect = Effect::Allow;
    decision.applied_effect = Effect::Allow;
    decision.notices = std::mem::take(&mut decision.reasons);
    let e = entry(&params, &decision, &ctx(NOW, "main")).expect("a warning is logged");
    assert_eq!(e.kind, LogKind::Notice);
    let authorship = e.authorship.clone().expect("commit facts");
    assert_eq!(
        authorship.coauthors,
        vec![Some(AgentKind::ClaudeCode), None]
    );
    assert!(authorship.agent_trailer);
    store.record_guard_decision(&e).unwrap();
    let log = store.guard_log(NOW - DAY, 500, NOW, false).unwrap();
    assert_eq!(log.summary.blocked, 0);
    assert_eq!(log.summary.notices, 1);

    // `flexible` with an agent: logged as a notice with no reason.
    let allowed = Decision {
        notices: vec![],
        ..decision
    };
    let flexible = LogContext {
        authorship_policy: Some("flexible".into()),
        ..ctx(NOW, "main")
    };
    let e = entry(&params, &allowed, &flexible).expect("flexible records the agent's commit");
    assert_eq!(e.kind, LogKind::Notice);
    assert!(e.reasons.is_empty());
}

#[test]
fn allowed_without_rule_is_not_logged() {
    let mut decision = denied(Rule::MinimumForcePush, vec![]);
    decision.effect = Effect::Allow;
    decision.applied_effect = Effect::Allow;
    decision.reasons.clear();
    assert!(entry(&force_push("main", "origin"), &decision, &ctx(NOW, "main")).is_none());
    // An unattributed commit under `flexible` is not an agent's: nothing either.
    let flexible = LogContext {
        actor: None,
        authorship_policy: Some("flexible".into()),
        ..ctx(NOW, "main")
    };
    let commit = EvaluateParams {
        hook: Hook::CommitMsg,
        operation: Operation::Commit {
            stage: CommitStage::CommitMsg,
        },
        ..force_push("main", "origin")
    };
    assert!(entry(&commit, &decision, &flexible).is_none());
}

#[test]
fn executor_operations_are_not_logged() {
    let under = LogContext {
        under_executor: true,
        ..ctx(NOW, "main")
    };
    let decision = denied(
        Rule::MinimumForcePush,
        vec![Param::new(ParamKind::Branch, "main")],
    );
    assert!(entry(&force_push("main", "origin"), &decision, &under).is_none());
}

#[test]
fn remote_loses_userinfo_query_and_fragment() {
    assert_eq!(
        sanitize_remote("https://user:s3cret@example.com/r.git?token=abc#frag"),
        "https://example.com/r.git"
    );
    assert_eq!(sanitize_remote("origin"), "origin");
    assert_eq!(
        sanitize_remote("ssh://git@example.com:22/r.git"),
        "ssh://example.com:22/r.git"
    );
    let e = entry(
        &force_push("main", "https://u:tok3n@example.com/r.git?x=tok3n"),
        &denied(
            Rule::MinimumForcePush,
            vec![Param::new(ParamKind::Branch, "main")],
        ),
        &ctx(NOW, "main"),
    )
    .unwrap();
    let text = serde_json::to_string(&e.operation).unwrap();
    assert!(!text.contains("tok3n"), "{text}");
}

#[test]
fn operation_keeps_only_the_refs_the_reasons_name() {
    let e = denial_at(NOW, "main");
    let LoggedOperation::Push { refs, remote } = &e.operation else {
        panic!("{:?}", e.operation);
    };
    assert_eq!(remote.as_ref().unwrap().raw(), "origin");
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!(refs[0].name.raw(), "main");
    assert_eq!(refs[0].change, RefChange::Force);
}

/// The hook never waits on the log: past the in-flight cap an entry is not queued but added to
/// the shared overflow counter, and that count reaches the KPI (coordinator's adjustment 2).
#[test]
fn a_full_sink_never_blocks_and_keeps_the_count() {
    use gitraptor_core::guardrails::log::LogSink;
    let sink = LogSink::with_capacity(2);
    assert!(sink.try_reserve());
    assert!(sink.try_reserve());
    assert!(!sink.try_reserve(), "the cap is reached: no queueing");
    for i in 0..500 {
        sink.overflow(&denial_at(NOW + i, &format!("b{i}")));
    }
    sink.release();
    assert!(sink.try_reserve(), "a released slot is reusable");
    let rows = sink.drain();
    assert_eq!(rows.iter().map(|r| r.count).sum::<u64>(), 500);
    assert!(sink.drain().is_empty(), "drained once");
    let (_tp, _repos, mut store) = store();
    for row in &rows {
        store.record_guard_overflow(row).unwrap();
    }
    let log = store
        .guard_log(NOW - DAY, 500, NOW + MINUTE, false)
        .unwrap();
    assert_eq!(log.summary.blocked, 500);
    assert_eq!(log.summary.rate_limited, 500);
}

/// A period the engine was down is reported, never a silent 0 (coordinator's adjustment 4).
#[test]
fn engine_down_periods_are_reported() {
    use gitraptor_core::profile::{GapCause, WriteOp};
    let (_tp, _repos, mut store) = store();
    store
        .write_batch(&[
            WriteOp::OpenGap {
                gap_id: "g1".into(),
                started_ms: NOW - 2 * DAY,
                cause: GapCause::DaemonStopped,
                requested_by: None,
            },
            WriteOp::CloseGap {
                gap_id: "g1".into(),
                ended_ms: NOW - DAY,
            },
            WriteOp::OpenGap {
                gap_id: "g0".into(),
                started_ms: NOW - 20 * DAY,
                cause: GapCause::MachineOff,
                requested_by: None,
            },
            WriteOp::CloseGap {
                gap_id: "g0".into(),
                ended_ms: NOW - 19 * DAY,
            },
        ])
        .unwrap();
    let log = store.guard_log(NOW - 7 * DAY, 500, NOW, false).unwrap();
    assert_eq!(log.unlogged_periods.len(), 1, "{:?}", log.unlogged_periods);
    assert_eq!(log.unlogged_periods[0].from_ms, NOW - 2 * DAY);
    assert_eq!(log.unlogged_periods[0].to_ms, Some(NOW - DAY));
}

#[test]
fn hiding_the_relax_notices_fills_the_page_and_keeps_the_totals_of_the_same_set() {
    use gitraptor_core::guardrails::config_guard::relax_entry;
    use gitraptor_policy::layers::{IgnoredRelaxation, RelaxKey};

    let (_tp, _repos, mut store) = store();
    // Three older denials, then more notices than the page holds, each its own entry.
    for (i, branch) in ["a", "b", "c"].into_iter().enumerate() {
        store
            .record_guard_decision(&denial_at(NOW + i as i64, branch))
            .unwrap();
    }
    let ignored = [IgnoredRelaxation {
        level: Level::Local,
        key: RelaxKey::BaseBranch,
    }];
    for (i, branch) in ["n1", "n2", "n3", "n4", "n5", "n6"].into_iter().enumerate() {
        let params = force_push(branch, "origin");
        let decision = denied(Rule::MinimumForcePush, vec![]);
        let notice = relax_entry(
            &params,
            &decision,
            &ignored,
            &ctx(NOW + 10 + i as i64, branch),
        )
        .expect("a notice");
        store.record_guard_decision(&notice).unwrap();
    }
    let all = store.guard_log(NOW - DAY, 3, NOW + MINUTE, false).unwrap();
    assert_eq!(all.entries.len(), 3);
    assert!(all.entries.iter().all(|e| e.kind == LogKind::Notice));
    assert_eq!(all.summary.notices, 6);

    let page = store.guard_log(NOW - DAY, 3, NOW + MINUTE, true).unwrap();
    assert_eq!(page.entries.len(), 3, "the page fills to the limit");
    assert!(
        page.entries
            .iter()
            .all(|e| e.reasons.iter().all(|r| r.rule != Rule::RelaxIgnored))
    );
    assert_eq!(page.summary.notices, 0);
    assert_eq!(page.summary.blocked, 3);
}
