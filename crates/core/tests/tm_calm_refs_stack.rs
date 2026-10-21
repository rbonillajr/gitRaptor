//! The late echo of a protected operation stays below the restore that follows it
//! (fix/tm-restore-calm-mark, the `us_tmc_009` undo-after-restore case).
//!
//! A real `RepoMarks` over a testkit fixture (temporary repo, home and profile; never this repo or
//! the real profile, NFR-01). The engine's batches land in an injected order: the protected
//! operation's commit is on disk, a heads-only batch (new `HEAD` reflog size, old branches) is
//! persisted, and only later the batch with its events 2 and 3. The operation's anchor and the
//! restore's intent read the calm at each point with `settle(ZERO)`, as their polling would, so
//! the order is fixed and nothing waits on time.

mod tm_common;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use gitraptor_api::messages::GitEventKind;
use gitraptor_core::timemachine::engine::{RawGitEvent, RepoMarks, git_dir_of};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OpRef, OperationKind, OperationTransition, Requester, Scope,
    SnapshotFilter, SnapshotLevel, StackScope, Target,
};
use gitraptor_core::timemachine::undo::{RawSide, external_events_in};
use gitraptor_testkit::Fixture;
use tm_common::{Env, REPO_ID, git};

fn branches(f: &Fixture) -> BTreeMap<String, String> {
    f.git(&[
        "for-each-ref",
        "--format=%(refname:short) %(objectname)",
        "refs/heads",
    ])
    .lines()
    .filter_map(|l| l.split_once(' '))
    .map(|(n, c)| (n.to_owned(), c.to_owned()))
    .collect()
}

fn publish_head(marks: &RepoMarks, wt: &Path) {
    let git_dir = git_dir_of(wt).unwrap();
    let len = std::fs::metadata(git_dir.join("logs/HEAD")).map_or(0, |m| m.len());
    marks.set_head_logs(REPO_ID, &[(wt.to_path_buf(), len)]);
    marks.set_heads(
        REPO_ID,
        &[(
            wt.to_path_buf(),
            std::fs::read(git_dir.join("HEAD")).unwrap(),
        )],
    );
}

/// Records an operation of `kind` at engine mark `mark` and walks it to `finished`.
fn finished(env: &Env, key: &str, kind: OperationKind, target: Target, mark: i64) -> String {
    let op = env
        .oplog
        .lock()
        .unwrap()
        .record_operation(
            &NewOperation {
                kind,
                subtype: None,
                scope: Scope {
                    worktrees: vec![key.to_owned()],
                    refs: vec![],
                },
                requester: Requester::Unattributed,
                channel: Channel::Cli,
                confirmed: true,
                target,
                warnings: vec![],
                engine_mark: mark,
            },
            10,
        )
        .unwrap();
    let mut req = env.request(SnapshotLevel::GuaranteedPrior, None);
    req.cause_operation = Some(op.clone());
    req.engine_mark = Some(mark);
    let prior = env.store.capture(&env.oplog, &req).unwrap().snapshot_id;
    let mut log = env.oplog.lock().unwrap();
    log.advance_operation(
        &op,
        OperationTransition::PriorSnapshot {
            snapshot_id: &prior,
        },
        11,
    )
    .unwrap();
    log.advance_operation(&op, OperationTransition::Ready, 12)
        .unwrap();
    log.advance_operation(&op, OperationTransition::Applying { step: 1 }, 13)
        .unwrap();
    log.advance_operation(&op, OperationTransition::Finished, 14)
        .unwrap();
    op
}

#[test]
fn a_late_echo_bound_by_a_calm_anchor_stays_below_the_restore() {
    let env = Env::new(Fixture::with_commit(&git()));
    let f = &env.f;
    let wt = f.repo.clone();
    let key = wt.display().to_string();
    let common = gitraptor_core::observe::locate(&wt).unwrap();
    let marks = RepoMarks::default();
    let settle = || marks.settle(REPO_ID, std::slice::from_ref(&wt), Duration::ZERO);

    // The engine persisted the repo up to event 1.
    marks.set_persisted(REPO_ID, 1, &common, branches(f));
    publish_head(&marks, &wt);
    let target = env
        .capture_at(SnapshotLevel::Observation, 1, None)
        .snapshot_id;

    // The protected operation (op 15) starts at mark 1 and commits.
    let op15 = finished(&env, &key, OperationKind::Protected, Target::None, 1);
    f.write("p.txt", "protected\n");
    f.git(&["add", "p.txt"]);
    f.git(&["commit", "-q", "-m", "protected"]);

    // Point A: the heads-only batch is persisted (new `HEAD` reflog size, no events, the
    // branches as before). The operation's anchor, then the restore's intent, read the calm.
    publish_head(&marks, &wt);
    marks.set_seq(REPO_ID, 1);
    let anchor_a = settle();
    let restore_a = settle();

    // Point B: the late batch with events 2 (commit) and 3 (branch-update).
    marks.set_persisted(REPO_ID, 3, &common, branches(f));
    let anchor = anchor_a.or_else(settle).expect("calm after the late batch");
    let restore_mark = restore_a
        .or_else(settle)
        .expect("calm after the late batch");
    assert!(
        anchor >= 3,
        "the anchor of op 15 covers its late echo: {anchor}"
    );
    assert!(
        restore_mark >= 3,
        "the restore's mark covers op 15's late echo: {restore_mark}"
    );

    // The anchor of op 15: the capture of the state it left, at its calm mark.
    let mut req = env.request(SnapshotLevel::Observation, None);
    req.cause_operation = Some(op15.clone());
    req.engine_mark = Some(anchor);
    env.store.capture(&env.oplog, &req).unwrap();
    let restore = finished(
        &env,
        &key,
        OperationKind::Restore,
        Target::Snapshot(target),
        restore_mark,
    );

    let raw = RawSide {
        events: [(2, GitEventKind::Commit), (3, GitEventKind::BranchUpdate)]
            .into_iter()
            .map(|(seq, kind)| RawGitEvent {
                seq,
                kind,
                branch: Some("main".into()),
                actor: Requester::Unattributed,
            })
            .collect(),
        floor: 0,
    };
    let log = env.oplog.lock().unwrap();
    let ops = log.operations(&Default::default()).unwrap();
    let snaps = log.snapshots(&SnapshotFilter::default()).unwrap();
    let external = external_events_in(&ops, &snaps, &raw, &key, "main");
    for e in &external {
        assert_eq!(
            e.caused_by.as_deref(),
            Some(op15.as_str()),
            "event {} is op 15's echo",
            e.seq
        );
    }
    let stack = log
        .undo_stack_in(&StackScope::Worktree(key.clone()), &external, 0)
        .unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::Oplog(restore)));
}
