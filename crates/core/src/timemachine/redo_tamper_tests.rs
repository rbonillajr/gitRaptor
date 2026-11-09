//! Security review M-01 of US-TMC-003: an undone operation whose oplog row was edited behind the
//! daemon's back (a declared break of the chain) does not lend its requester to the redo's owners.
//! Temporary profile only (NFR-01).

use super::*;
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::{
    Channel, NewOperation, OPLOG_FILE, OpRef, OperationKind, Oplog, Requester, RequesterOrigin,
    Scope, Target, repo_dir,
};
use crate::timemachine::undo::RawSide;

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abce";
const WT: &str = "/repo/feat-login";

fn agent(name: &str) -> Requester {
    Requester::Agent {
        name: name.into(),
        origin: RequesterOrigin::Detected,
        session_id: format!("{name}-session"),
    }
}

fn record(log: &mut Oplog, kind: OperationKind, who: Requester, target: Target) -> String {
    log.record_operation(
        &NewOperation {
            kind,
            subtype: None,
            scope: Scope {
                worktrees: vec![WT.into()],
                refs: vec!["refs/heads/feat-login".into()],
            },
            requester: who,
            channel: Channel::Cli,
            confirmed: false,
            target,
            warnings: vec![],
            engine_mark: 1,
        },
        2_000,
    )
    .unwrap()
}

#[test]
fn a_tampered_undone_operation_counts_as_unattributed() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (mut log, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
    let work = record(
        &mut log,
        OperationKind::Protected,
        agent("claude-2"),
        Target::None,
    );
    let undo = record(
        &mut log,
        OperationKind::Undo,
        Requester::Unattributed,
        Target::Undo(vec![OpRef::Oplog(work.clone())]),
    );
    drop(log);

    // claude-1 rewrites the requester of claude-2's work with sqlite3, bypassing the triggers.
    let conn = rusqlite::Connection::open(repo_dir(&dirs, REPO).unwrap().join(OPLOG_FILE)).unwrap();
    for k in ["update", "delete"] {
        conn.execute_batch(&["DROP TRIGGER operations_no_", k].concat())
            .unwrap();
    }
    let forged = serde_json::to_string(&agent("claude-1")).unwrap();
    conn.execute(
        "UPDATE operations SET requester = ?1 WHERE operation_id = ?2",
        [&forged, &work],
    )
    .unwrap();
    drop(conn);

    let (log, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
    assert!(
        log.operation(&work).unwrap().unwrap().tampered,
        "the edited row is declared tampered"
    );
    let undo_view = log.operation(&undo).unwrap().unwrap();

    let owners = redo_owners(&undo_view, &RawSide::default(), &log);

    assert!(
        !owners.contains(&agent("claude-1")),
        "M-01: a tampered row never lends its requester to the owners: {owners:?}"
    );
    assert!(
        owners.contains(&Requester::Unattributed),
        "M-01: the tampered operation counts as unattributed: {owners:?}"
    );
}
