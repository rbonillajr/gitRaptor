use std::path::Path;

use serde_json::json;

use super::*;
use crate::daemon::LogLimits;
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::{
    CompleteInfo, NewOperation, NewSnapshot, OperationTransition, Requester, RequesterOrigin,
    Scope, SnapshotLevel, Target,
};

const REPO: &str = "0123456789abcdef0123456789abcdef";
const WT: &str = "/repo/feat-login";
const WT2: &str = "/repo/feat-pagos";

struct Fixture {
    _tmp: tempfile::TempDir,
    log: Oplog,
    logger: Logger,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (log, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let logger = Logger::open(&state, LogLimits::default()).unwrap();
    Fixture {
        _tmp: tmp,
        log,
        logger,
    }
}

/// An undo of `worktrees` with its prior snapshot, cut half-way: the notice the recovery
/// records for each worktree (or the whole repo when there are none).
fn interrupted_undo(log: &mut Oplog, worktrees: &[&str]) -> (String, String) {
    let keys: Vec<String> = worktrees.iter().map(|w| (*w).to_owned()).collect();
    let op = log
        .record_operation(
            &NewOperation {
                kind: OperationKind::Undo,
                subtype: None,
                scope: Scope {
                    worktrees: keys.clone(),
                    refs: vec![],
                },
                requester: Requester::Agent {
                    name: "claude".into(),
                    origin: RequesterOrigin::Detected,
                    session_id: "s1".into(),
                },
                channel: Channel::Cli,
                confirmed: false,
                target: Target::Undo(vec![]),
                warnings: vec![],
                engine_mark: 1,
            },
            10,
        )
        .unwrap();
    let prior = log
        .begin_snapshot(
            &NewSnapshot {
                level: SnapshotLevel::GuaranteedPrior,
                worktrees: keys.clone(),
                engine_mark: Some(1),
                cause_operation: Some(op.clone()),
                cause_event_seq: None,
            },
            11,
        )
        .unwrap();
    log.complete_snapshot(&prior, &CompleteInfo::default(), 12)
        .unwrap();
    log.advance_operation(
        &op,
        OperationTransition::PriorSnapshot {
            snapshot_id: &prior,
        },
        13,
    )
    .unwrap();
    let detail = json!({ "state": "interrupted" });
    if keys.is_empty() {
        log.record_notice(NoticeKind::Interruption, None, Some(&op), &detail, 20)
            .unwrap();
    }
    for key in &keys {
        log.record_notice(NoticeKind::Interruption, Some(key), Some(&op), &detail, 20)
            .unwrap();
    }
    (op, prior)
}

fn take(f: &mut Fixture, worktree: &str) -> Vec<TmNotice> {
    take_interruptions(
        &mut f.log,
        Path::new(worktree),
        Channel::Cli,
        100,
        &f.logger,
    )
    .unwrap()
}

#[test]
fn a_notice_is_delivered_once_and_never_again() {
    let mut f = fixture();
    let (op, prior) = interrupted_undo(&mut f.log, &[WT]);

    let first = take(&mut f, WT);
    assert_eq!(first.len(), 1, "{first:#?}");
    let notice = &first[0];
    assert_eq!(notice.kind, TmNoticeKind::Interruption);
    assert_eq!(notice.operation_id.as_deref(), Some(op.as_str()));
    assert_eq!(notice.operation_kind, Some(TimelineOperationKind::Undo));
    assert_eq!(notice.prior_snapshot_id.as_deref(), Some(prior.as_str()));
    assert_eq!(notice.recorded_utc_ms, 20);

    assert!(take(&mut f, WT).is_empty());
    assert!(f.log.pending_notices(Some(WT)).unwrap().is_empty());
}

#[test]
fn another_worktree_receives_nothing() {
    let mut f = fixture();
    interrupted_undo(&mut f.log, &[WT]);

    assert!(take(&mut f, WT2).is_empty());
    // Still waiting for its own worktree.
    assert_eq!(take(&mut f, WT).len(), 1);
}

#[test]
fn a_whole_repo_notice_reaches_the_first_client_of_any_worktree_only() {
    let mut f = fixture();
    let (op, _) = interrupted_undo(&mut f.log, &[]);

    let first = take(&mut f, WT2);
    assert_eq!(first.len(), 1, "{first:#?}");
    assert_eq!(first[0].operation_id.as_deref(), Some(op.as_str()));
    assert!(take(&mut f, WT).is_empty());
    assert!(take(&mut f, WT2).is_empty());
}

#[test]
fn a_purge_notice_is_never_delivered() {
    let mut f = fixture();
    let purge = f
        .log
        .record_notice(NoticeKind::Purge, None, None, &json!({}), 5)
        .unwrap();

    assert!(take(&mut f, WT).is_empty());
    // Untouched: its grace period has not started.
    let pending = f.log.pending_notices(Some(WT)).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].notice_id, purge);
    assert_eq!(f.log.first_interactive_delivery_ms(&purge).unwrap(), None);
}
