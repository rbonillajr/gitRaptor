//! Acceptance tests of TS-TMC-002. Every test works in a temporary profile
//! and temporary folders, never in a real repo or profile (NFR-01).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::profile::ProfileDirs;

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";
const WT: &str = "/repo/main";
const WT2: &str = "/repo/feature";

fn profile() -> (tempfile::TempDir, ProfileDirs) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    (tmp, dirs)
}

fn open(dirs: &ProfileDirs) -> (Oplog, OplogOpen) {
    Oplog::open(dirs, REPO, 1_000).unwrap()
}

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn protected(worktrees: &[&str], mark: i64) -> NewOperation {
    NewOperation {
        kind: OperationKind::Protected,
        subtype: Some("reset-hard".into()),
        scope: Scope {
            worktrees: worktrees.iter().map(|w| (*w).to_owned()).collect(),
            refs: vec![],
        },
        requester: agent("s1"),
        channel: Channel::Mcp,
        confirmed: false,
        target: Target::None,
        warnings: vec![],
        engine_mark: mark,
    }
}

fn prior(worktrees: &[&str], op: Option<&str>) -> NewSnapshot {
    NewSnapshot {
        level: SnapshotLevel::GuaranteedPrior,
        worktrees: worktrees.iter().map(|w| (*w).to_owned()).collect(),
        engine_mark: Some(1),
        cause_operation: op.map(str::to_owned),
        cause_event_seq: None,
    }
}

fn complete(log: &mut Oplog, worktrees: &[&str], op: Option<&str>, at: i64) -> String {
    let id = log.begin_snapshot(&prior(worktrees, op), at).unwrap();
    log.complete_snapshot(&id, &CompleteInfo::default(), at)
        .unwrap();
    id
}

/// Drives an operation to `state` through valid transitions.
fn op_in_state(log: &mut Oplog, new: &NewOperation, state: OperationState, at: i64) -> String {
    let id = log.record_operation(new, at).unwrap();
    use OperationState::*;
    if state == Intent {
        return id;
    }
    if state == Rejected {
        log.advance_operation(&id, OperationTransition::Rejected { reason: "overlap" }, at)
            .unwrap();
        return id;
    }
    let snap = complete(
        log,
        &new.scope
            .worktrees
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        Some(&id),
        at,
    );
    log.advance_operation(
        &id,
        OperationTransition::PriorSnapshot { snapshot_id: &snap },
        at,
    )
    .unwrap();
    if state == PriorSnapshot {
        return id;
    }
    log.advance_operation(&id, OperationTransition::Ready, at)
        .unwrap();
    if state == Ready {
        return id;
    }
    log.advance_operation(&id, OperationTransition::Applying { step: 1 }, at)
        .unwrap();
    if state == Applying {
        return id;
    }
    log.advance_operation(&id, OperationTransition::Finished, at)
        .unwrap();
    id
}

/// A snapshot store in memory.
#[derive(Default)]
struct FakeStore {
    refs: HashSet<String>,
    deleted: Vec<String>,
}

impl SnapshotRefs for FakeStore {
    fn snapshot_ids(&self) -> io::Result<Option<Vec<String>>> {
        Ok(Some(self.refs.iter().cloned().collect()))
    }
    fn delete(&mut self, ids: &[String]) -> io::Result<()> {
        for id in ids {
            self.refs.remove(id);
            self.deleted.push(id.clone());
        }
        Ok(())
    }
}

struct Probe(HashSet<u32>);
impl ProcessProbe for Probe {
    fn is_alive(&self, pid: u32) -> bool {
        self.0.contains(&pid)
    }
}

fn options<'a>(git_dir: &'a Path, probe: &'a dyn ProcessProbe) -> RecoveryOptions<'a> {
    RecoveryOptions {
        git_dir,
        deadline: Instant::now() + Duration::from_millis(200),
        poll: Duration::from_millis(10),
        probe,
    }
}

/// Raw access to the file, as another process of the same user would have.
fn raw(dirs: &ProfileDirs) -> rusqlite::Connection {
    rusqlite::Connection::open(repo_dir(dirs, REPO).unwrap().join(OPLOG_FILE)).unwrap()
}

/// SQL is always parameterized (SEC-06), as in the profile stores.
#[test]
fn sql_is_never_built_with_format() {
    let banned = ["format", "!("].concat();
    for (name, source) in [
        ("mod.rs", include_str!("mod.rs")),
        ("chain.rs", include_str!("chain.rs")),
        ("query.rs", include_str!("query.rs")),
        ("recovery.rs", include_str!("recovery.rs")),
        ("schema.rs", include_str!("schema.rs")),
        ("stack.rs", include_str!("stack.rs")),
    ] {
        assert!(!source.contains(&banned), "{name} must bind parameters");
    }
}

// ----- Immutability -------------------------------------------------------

#[test]
fn rows_and_journal_reject_update_and_delete() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 10);
    log.record_notice(NoticeKind::Purge, None, None, &serde_json::json!({}), 11)
        .unwrap();
    let conn = log.conn();
    for sql in [
        "UPDATE operations SET channel = 'cli'",
        "DELETE FROM operations",
        "UPDATE snapshots SET level = 'observation'",
        "DELETE FROM snapshots",
        "UPDATE journal SET state = 'intent'",
        "DELETE FROM journal",
        "UPDATE notices SET kind = 'interruption'",
        "DELETE FROM notices",
        "UPDATE chain SET hash = x'00'",
        "DELETE FROM chain",
    ] {
        let err = conn.execute(sql, []).unwrap_err();
        assert!(err.to_string().contains("append-only"), "{sql}: {err}");
    }
    assert_eq!(
        log.operation(&op).unwrap().unwrap().state,
        OperationState::Finished
    );
    assert!(log.verify_chain().unwrap().is_empty());
}

#[test]
fn invalid_transitions_are_refused() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let op = log.record_operation(&protected(&[WT], 1), 1).unwrap();
    assert!(
        log.advance_operation(&op, OperationTransition::Applying { step: 1 }, 2)
            .is_err()
    );
    let pending = log.begin_snapshot(&prior(&[WT], Some(&op)), 2).unwrap();
    // A prior snapshot that is not complete cannot be the prior.
    assert!(
        log.advance_operation(
            &op,
            OperationTransition::PriorSnapshot {
                snapshot_id: &pending
            },
            3
        )
        .is_err()
    );
    log.complete_snapshot(&pending, &CompleteInfo::default(), 3)
        .unwrap();
    log.advance_operation(
        &op,
        OperationTransition::PriorSnapshot {
            snapshot_id: &pending,
        },
        4,
    )
    .unwrap();
    log.advance_operation(&op, OperationTransition::Ready, 5)
        .unwrap();
    log.advance_operation(&op, OperationTransition::Applying { step: 2 }, 6)
        .unwrap();
    assert!(
        log.advance_operation(&op, OperationTransition::Applying { step: 2 }, 7)
            .is_err()
    );
    log.advance_operation(&op, OperationTransition::Finished, 8)
        .unwrap();
    assert!(
        log.advance_operation(&op, OperationTransition::Interrupted, 9)
            .is_err()
    );
    assert!(
        log.set_snapshot_state(&pending, SnapshotState::Pending, 9)
            .is_err()
    );
    let view = log.operation(&op).unwrap().unwrap();
    assert_eq!((view.state, view.step), (OperationState::Finished, Some(2)));
    assert_eq!(view.prior_snapshot.as_deref(), Some(pending.as_str()));
}

// ----- Frozen requester ---------------------------------------------------

/// The engine's current attribution, with a correction applied later.
struct Attribution(RefCell<HashMap<String, String>>);
impl CurrentAttribution for Attribution {
    type Actor = String;
    fn current_actor(&self, session_id: &str) -> Option<String> {
        self.0.borrow().get(session_id).cloned()
    }
}

#[test]
fn a_later_correction_does_not_change_the_frozen_requester() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 10);
    let attribution = Attribution(RefCell::new(HashMap::from([(
        "s1".to_owned(),
        "claude".to_owned(),
    )])));
    let filter = OperationFilter::default();
    let before = log
        .operations_with_current_actor(&filter, &attribution)
        .unwrap();
    assert_eq!(before[0].1.as_deref(), Some("claude"));

    // The engine publishes a correction of session s1.
    attribution
        .0
        .borrow_mut()
        .insert("s1".into(), "codex".into());
    let after = log
        .operations_with_current_actor(&filter, &attribution)
        .unwrap();
    assert_eq!(
        after[0].1.as_deref(),
        Some("codex"),
        "actor is the current one"
    );
    assert_eq!(
        after[0].0.record.requester,
        agent("s1"),
        "requester is frozen"
    );
    assert_eq!(after[0].0.record.operation_id, op);
    drop(log);
    let (log, _) = open(&dirs);
    assert_eq!(
        log.operation(&op).unwrap().unwrap().record.requester,
        agent("s1")
    );
}

// ----- Recovery -----------------------------------------------------------

#[test]
fn recovery_at_every_state_of_the_journal() {
    let (tmp, dirs) = profile();
    let mut store = FakeStore::default();
    let (mut log, _) = open(&dirs);
    use OperationState::*;
    let ops: HashMap<OperationState, String> =
        [Intent, PriorSnapshot, Ready, Applying, Finished, Rejected]
            .into_iter()
            .map(|s| (s, op_in_state(&mut log, &protected(&[WT, WT2], 1), s, 10)))
            .collect();
    // Snapshots: a pending one with its ref (crash after creating the ref),
    // a pending one without, a valid one, two purges cut in the middle, one
    // complete whose ref vanished, and a ref nobody knows.
    let pending_ref = log.begin_snapshot(&prior(&[WT], None), 20).unwrap();
    let pending_noref = log.begin_snapshot(&prior(&[WT], None), 20).unwrap();
    let valid = complete(&mut log, &[WT], None, 20);
    let purge_kept = complete(&mut log, &[WT], None, 20);
    let purge_gone = complete(&mut log, &[WT], None, 20);
    let lost = complete(&mut log, &[WT], None, 20);
    for id in [&purge_kept, &purge_gone] {
        log.set_snapshot_state(id, SnapshotState::PurgeIntent, 21)
            .unwrap();
    }
    for view in log.snapshots(&SnapshotFilter::default()).unwrap() {
        store.refs.insert(view.record.snapshot_id);
    }
    store.refs.remove(&pending_noref);
    store.refs.remove(&purge_gone);
    store.refs.remove(&lost);
    store.refs.insert("unknown-ref".into());

    // kill -9: the process dies; only what was committed survives.
    drop(log);
    let (mut log, opened) = open(&dirs);
    assert!(opened.new_breaks.is_empty());
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut store, &options(tmp.path(), &probe), 100)
        .unwrap();

    let state = |log: &Oplog, s: OperationState| log.operation(&ops[&s]).unwrap().unwrap().state;
    assert_eq!(state(&log, Intent), Aborted);
    assert_eq!(state(&log, PriorSnapshot), Aborted);
    assert_eq!(state(&log, Ready), Aborted);
    assert_eq!(state(&log, Applying), Interrupted);
    assert_eq!(state(&log, Finished), Finished);
    assert_eq!(state(&log, Rejected), Rejected);
    assert_eq!(report.interrupted_operations, vec![ops[&Applying].clone()]);

    let snap = |id: &str| log.snapshot(id).unwrap().unwrap().state;
    assert_eq!(snap(&pending_ref), SnapshotState::Discarded);
    assert_eq!(snap(&pending_noref), SnapshotState::Discarded);
    assert_eq!(snap(&valid), SnapshotState::Complete);
    assert_eq!(snap(&purge_kept), SnapshotState::PurgeCancelled);
    assert_eq!(snap(&purge_gone), SnapshotState::Purged);
    assert_eq!(report.deleted_refs, vec![pending_ref.clone()]);
    assert_eq!(report.unknown_refs, vec!["unknown-ref".to_owned()]);
    assert!(store.refs.contains("unknown-ref"), "an unknown ref is kept");
    assert_eq!(report.missing_refs, vec![lost.clone()]);

    // Offered: valid and the cancelled purge, with ref and complete row.
    let offered: HashSet<String> = log
        .offerable_snapshots(&SnapshotFilter::default(), &store)
        .unwrap()
        .into_iter()
        .map(|s| s.record.snapshot_id)
        .collect();
    let applying_prior = log
        .operation(&ops[&Applying])
        .unwrap()
        .unwrap()
        .prior_snapshot
        .unwrap();
    assert!(offered.contains(&valid) && offered.contains(&purge_kept));
    assert!(
        offered.contains(&applying_prior),
        "the way back of the interruption"
    );
    assert!(!offered.contains(&pending_ref) && !offered.contains(&lost));

    // One notice per worktree of the interrupted operation, shown once.
    assert_eq!(report.notices.len(), 2);
    let pending = log.pending_notices(Some(WT)).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].operation_id.as_deref(),
        Some(ops[&Applying].as_str())
    );
    log.mark_notice_delivered(&pending[0].notice_id, Channel::Cli, 101)
        .unwrap();
    assert!(log.pending_notices(Some(WT)).unwrap().is_empty());
    assert_eq!(log.pending_notices(Some(WT2)).unwrap().len(), 1);

    // A second start finds nothing left to do and adds no notice.
    drop(log);
    let (mut log, _) = open(&dirs);
    let again = log
        .recover(&mut store, &options(tmp.path(), &probe), 200)
        .unwrap();
    assert!(again.discarded_snapshots.is_empty() && again.aborted_operations.is_empty());
    assert!(again.interrupted_operations.is_empty() && again.notices.is_empty());
    assert!(again.deleted_refs.is_empty());
    assert!(log.verify_chain().unwrap().is_empty());
}

#[test]
fn without_a_store_nothing_about_refs_is_decided() {
    let (tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let valid = complete(&mut log, &[WT], None, 1);
    let purging = complete(&mut log, &[WT], None, 1);
    log.set_snapshot_state(&purging, SnapshotState::PurgeIntent, 2)
        .unwrap();
    let pending = log.begin_snapshot(&prior(&[WT], None), 3).unwrap();
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(tmp.path(), &probe), 4)
        .unwrap();
    assert!(!report.store_available);
    assert_eq!(report.discarded_snapshots, vec![pending]);
    assert!(report.missing_refs.is_empty() && report.purges_completed.is_empty());
    assert_eq!(
        log.snapshot(&valid).unwrap().unwrap().state,
        SnapshotState::Complete
    );
    assert_eq!(
        log.snapshot(&purging).unwrap().unwrap().state,
        SnapshotState::PurgeIntent
    );
}

#[test]
fn a_clean_stop_leaves_nothing_to_recover() {
    let (tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    drop(log);
    let (mut log, _) = open(&dirs);
    let probe = Probe(HashSet::new());
    let mut store = FakeStore::default();
    for s in log.snapshots(&SnapshotFilter::default()).unwrap() {
        store.refs.insert(s.record.snapshot_id);
    }
    let report = log
        .recover(&mut store, &options(tmp.path(), &probe), 2)
        .unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert!(log.notices().unwrap().is_empty());
}

// ----- Locks --------------------------------------------------------------

#[cfg(unix)]
struct Repo {
    _tmp: tempfile::TempDir,
    git_dir: PathBuf,
}

#[cfg(unix)]
fn repo() -> Repo {
    let tmp = tempfile::tempdir().unwrap();
    let git_dir = tmp.path().join("work").join(".git");
    fs::create_dir_all(git_dir.join("worktrees").join("feature")).unwrap();
    let git_dir = git_dir.canonicalize().unwrap();
    Repo { _tmp: tmp, git_dir }
}

#[cfg(unix)]
fn take_lock(log: &mut Oplog, op: &str, path: &Path) -> FileIdentity {
    fs::write(path, b"").unwrap();
    let identity = file_identity(path).unwrap().unwrap();
    log.record_lock_taken(op, path, identity, 5).unwrap();
    identity
}

#[cfg(unix)]
#[test]
fn an_own_annotated_lock_is_released_and_a_foreign_one_stays() {
    let (_tmp, dirs) = profile();
    let repo = repo();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Applying, 1);
    let own = repo.git_dir.join("index.lock");
    let own_wt = repo.git_dir.join("worktrees/feature/index.lock");
    take_lock(&mut log, &op, &own);
    take_lock(&mut log, &op, &own_wt);
    let foreign = repo.git_dir.join("HEAD.lock");
    fs::write(&foreign, b"").unwrap();
    drop(log);

    let (mut log, _) = open(&dirs);
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 10)
        .unwrap();
    assert!(!own.exists() && !own_wt.exists());
    assert!(
        foreign.exists(),
        "a lock not in the journal is never touched"
    );
    assert_eq!(report.released_locks.len(), 2);
    assert!(report.kept_locks.is_empty());
}

#[cfg(unix)]
#[test]
fn a_lock_replaced_since_it_was_annotated_stays() {
    let (_tmp, dirs) = profile();
    let repo = repo();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Applying, 1);
    let lock = repo.git_dir.join("index.lock");
    let ours = take_lock(&mut log, &op, &lock);
    // Someone else's Git took the lock after ours went away. On ext4 the new
    // file usually gets our freed inode; its birth time still differs, once
    // the file system's clock has ticked.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        fs::remove_file(&lock).unwrap();
        fs::write(&lock, b"theirs").unwrap();
        if file_identity(&lock).unwrap().unwrap() != ours || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 10)
        .unwrap();
    assert!(lock.exists());
    assert_eq!(report.kept_locks[0].reason, KeptLockReason::InodeChanged);
}

#[cfg(unix)]
#[test]
fn a_reused_inode_with_another_birth_time_stays() {
    let (_tmp, dirs) = profile();
    let repo = repo();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Applying, 1);
    let lock = repo.git_dir.join("index.lock");
    fs::write(&lock, b"theirs").unwrap();
    let theirs = file_identity(&lock).unwrap().unwrap();
    // The journal says ours had this inode, but was born at another time.
    let ours = FileIdentity {
        inode: theirs.inode,
        birth_ns: theirs.birth_ns.map(|b| b - 1).or(Some(0)),
    };
    log.record_lock_taken(&op, &lock, ours, 5).unwrap();
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 10)
        .unwrap();
    assert!(lock.exists());
    assert_eq!(report.kept_locks[0].reason, KeptLockReason::InodeChanged);
}

#[cfg(unix)]
#[test]
fn a_lock_annotated_without_a_birth_time_stays() {
    let (_tmp, dirs) = profile();
    let repo = repo();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Applying, 1);
    let lock = repo.git_dir.join("index.lock");
    fs::write(&lock, b"").unwrap();
    let identity = FileIdentity {
        birth_ns: None,
        ..file_identity(&lock).unwrap().unwrap()
    };
    log.record_lock_taken(&op, &lock, identity, 5).unwrap();
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 10)
        .unwrap();
    assert!(
        lock.exists(),
        "without a birth time the identity is unknown"
    );
    assert_eq!(report.kept_locks[0].reason, KeptLockReason::Unsupported);
}

#[cfg(unix)]
#[test]
fn a_lock_held_by_a_live_child_waits_and_stays() {
    let (_tmp, dirs) = profile();
    let repo = repo();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Applying, 1);
    let lock = repo.git_dir.join("index.lock");
    take_lock(&mut log, &op, &lock);
    log.record_child_started(&op, 4242, 6).unwrap();
    log.record_child_started(&op, 4343, 6).unwrap();
    log.record_child_ended(&op, 4343, 7).unwrap();
    let probe = Probe(HashSet::from([4242]));
    let started = Instant::now();
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 10)
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "waited for the child"
    );
    assert!(lock.exists());
    assert_eq!(report.kept_locks[0].reason, KeptLockReason::ChildAlive);

    // Once the child is gone, the next start releases it.
    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 20)
        .unwrap();
    assert_eq!(report.released_locks, vec![lock.clone()]);
    assert!(!lock.exists());
}

#[cfg(unix)]
#[test]
fn forged_lock_entries_never_delete_anything_else() {
    let (_tmp, dirs) = profile();
    let repo = repo();
    let outside = tempfile::tempdir().unwrap();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Applying, 1);
    // Outside the Git directory.
    let elsewhere = outside.path().join("index.lock");
    take_lock(&mut log, &op, &elsewhere);
    // Not a lock file.
    let config = repo.git_dir.join("config");
    take_lock(&mut log, &op, &config);
    // A symbolic link named like a lock, pointing outside.
    let target = outside.path().join("precious");
    fs::write(&target, b"data").unwrap();
    let link = repo.git_dir.join("packed-refs.lock");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let identity = file_identity(&link).unwrap().unwrap();
    log.record_lock_taken(&op, &link, identity, 5).unwrap();

    let probe = Probe(HashSet::new());
    let report = log
        .recover(&mut AbsentStore, &options(&repo.git_dir, &probe), 10)
        .unwrap();
    assert!(elsewhere.exists() && config.exists() && target.exists() && link.exists());
    let reasons: Vec<KeptLockReason> = report.kept_locks.iter().map(|k| k.reason).collect();
    assert_eq!(
        reasons,
        vec![
            KeptLockReason::OutsideGitDir,
            KeptLockReason::NotALockFile,
            KeptLockReason::NotALockFile
        ]
    );
}

// ----- Undo / redo stack --------------------------------------------------

fn finish_undo(log: &mut Oplog, kind: OperationKind, target: Target, mark: i64) -> String {
    let mut new = protected(&[WT], mark);
    new.kind = kind;
    new.subtype = None;
    new.target = target;
    op_in_state(log, &new, OperationState::Finished, mark)
}

#[test]
fn undo_undo_redo_and_a_new_operation_clears_the_redo() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let scope = StackScope::Worktree(WT.into());
    let a = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    let b = op_in_state(&mut log, &protected(&[WT], 3), OperationState::Finished, 3);
    // A raw Git event between a and b, and one caused by b (skipped).
    let external = [
        ExternalEvent {
            seq: 2,
            caused_by: None,
        },
        ExternalEvent {
            seq: 3,
            caused_by: Some(b.clone()),
        },
    ];
    let stack = log.undo_stack(&scope, &external).unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::Oplog(b.clone())));

    let u1 = finish_undo(
        &mut log,
        OperationKind::Undo,
        Target::Undo(vec![OpRef::Oplog(b.clone())]),
        4,
    );
    let stack = log.undo_stack(&scope, &external).unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::GitEvent(2)));
    let u2 = finish_undo(
        &mut log,
        OperationKind::Undo,
        Target::Undo(vec![OpRef::GitEvent(2)]),
        5,
    );
    let stack = log.undo_stack(&scope, &external).unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::Oplog(a.clone())));
    assert_eq!(stack.next_redo(), Some(u2.as_str()));

    finish_undo(&mut log, OperationKind::Redo, Target::Redo(u2.clone()), 6);
    let stack = log.undo_stack(&scope, &external).unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::GitEvent(2)));
    assert_eq!(stack.next_redo(), Some(u1.as_str()));

    // A new operation in the scope invalidates the redo.
    let c = op_in_state(&mut log, &protected(&[WT], 7), OperationState::Finished, 7);
    let stack = log.undo_stack(&scope, &external).unwrap();
    assert_eq!(stack.next_redo(), None);
    assert_eq!(stack.last_operation(), Some(&OpRef::Oplog(c)));

    // Other worktrees and the refs stack are separate.
    let other = log
        .undo_stack(&StackScope::Worktree(WT2.into()), &[])
        .unwrap();
    assert_eq!(other.last_operation(), None);
    let refs_op = op_in_state(&mut log, &protected(&[], 8), OperationState::Finished, 8);
    let refs = log.undo_stack(&StackScope::Refs, &[]).unwrap();
    assert_eq!(refs.last_operation(), Some(&OpRef::Oplog(refs_op)));
}

#[test]
fn an_interrupted_undo_is_the_next_thing_to_undo() {
    let (tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let a = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    let mut undo = protected(&[WT], 2);
    undo.kind = OperationKind::Undo;
    undo.target = Target::Undo(vec![OpRef::Oplog(a)]);
    let u = op_in_state(&mut log, &undo, OperationState::Applying, 2);
    let probe = Probe(HashSet::new());
    log.recover(&mut AbsentStore, &options(tmp.path(), &probe), 3)
        .unwrap();
    let stack = log
        .undo_stack(&StackScope::Worktree(WT.into()), &[])
        .unwrap();
    assert_eq!(stack.last_operation(), Some(&OpRef::Oplog(u)));
}

// ----- Isolation from the engine store ------------------------------------

#[test]
fn a_corrupt_engine_store_does_not_stop_listing_snapshots() {
    let (_tmp, dirs) = profile();
    let (mut profile, _) = crate::profile::Profile::open(dirs.clone()).unwrap();
    let common = tempfile::tempdir().unwrap();
    let (entry, _) = profile.add_repo(common.path(), None, 1).unwrap();
    drop(profile.open_store(&entry.repo_id).unwrap());

    let (mut log, _) = Oplog::open(&dirs, &entry.repo_id, 1).unwrap();
    let snap = complete(&mut log, &[WT], None, 2);
    drop(log);

    fs::write(profile.store_path(&entry.repo_id), b"not a database at all").unwrap();
    let (_, status) = profile.open_store(&entry.repo_id).unwrap();
    assert!(matches!(
        status,
        crate::profile::StoreOpen::Recovered { .. }
    ));

    let (log, opened) = Oplog::open(&dirs, &entry.repo_id, 3).unwrap();
    assert_eq!(opened.status, OplogStatus::Existing);
    assert!(opened.new_breaks.is_empty());
    let store = FakeStore {
        refs: HashSet::from([snap.clone()]),
        deleted: vec![],
    };
    let offered = log
        .offerable_snapshots(&SnapshotFilter::default(), &store)
        .unwrap();
    assert_eq!(offered[0].record.snapshot_id, snap);
}

// ----- Hash chain ---------------------------------------------------------

fn drop_triggers(conn: &rusqlite::Connection) {
    for t in ["snapshots", "operations", "journal", "notices", "chain"] {
        for k in ["update", "delete"] {
            let name = [t, "_no_", k].concat();
            conn.execute_batch(&["DROP TRIGGER ", &name].concat())
                .unwrap();
        }
    }
}

#[test]
fn an_edited_row_is_detected_declared_once_and_not_offered() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let snap = complete(&mut log, &[WT], None, 1);
    let op = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 2);
    drop(log);

    // The user edits the requester with sqlite3, bypassing the triggers.
    let conn = raw(&dirs);
    drop_triggers(&conn);
    conn.execute(
        "UPDATE operations SET requester = '{\"variant\":\"unattributed\"}' WHERE operation_id = ?1",
        [&op],
    )
    .unwrap();
    conn.execute(
        "UPDATE snapshots SET level = 'observation' WHERE snapshot_id = ?1",
        [&snap],
    )
    .unwrap();
    drop(conn);

    let (log, opened) = open(&dirs);
    let causes: Vec<BreakCause> = opened.new_breaks.iter().map(|b| b.cause).collect();
    assert_eq!(causes, vec![BreakCause::RowAltered, BreakCause::RowAltered]);
    assert!(log.operation(&op).unwrap().unwrap().tampered);
    assert!(log.snapshot(&snap).unwrap().unwrap().tampered);
    let store = FakeStore {
        refs: HashSet::from([snap.clone()]),
        deleted: vec![],
    };
    assert!(
        log.offerable_snapshots(&SnapshotFilter::default(), &store)
            .unwrap()
            .is_empty()
    );
    drop(log);

    // Declared as a gap once; the next open does not declare it again.
    let (log, opened) = open(&dirs);
    assert!(opened.new_breaks.is_empty());
    assert_eq!(log.breaks().len(), 2);
    assert!(log.snapshot(&snap).unwrap().unwrap().tampered);
}

#[test]
fn a_removed_row_breaks_the_link() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let a = op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    op_in_state(&mut log, &protected(&[WT], 2), OperationState::Finished, 2);
    drop(log);
    let conn = raw(&dirs);
    drop_triggers(&conn);
    let seq: i64 = conn
        .query_row(
            "SELECT seq FROM operations WHERE operation_id = ?1",
            [&a],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute("DELETE FROM operations WHERE seq = ?1", [seq])
        .unwrap();
    conn.execute("DELETE FROM chain WHERE seq = ?1", [seq])
        .unwrap();
    drop(conn);
    let (_, opened) = open(&dirs);
    assert_eq!(
        opened.new_breaks,
        vec![ChainBreak {
            seq: seq + 1,
            cause: BreakCause::LinkBroken
        }]
    );
}

#[test]
fn head_outside_the_oplog_detects_cut_and_lost_heads() {
    let (_tmp, dirs) = profile();
    let head = repo_dir(&dirs, REPO).unwrap().join(HEAD_FILE);
    let (mut log, _) = open(&dirs);
    op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    let tip = fs::read(&head).unwrap();
    op_in_state(&mut log, &protected(&[WT], 2), OperationState::Finished, 2);
    drop(log);

    // The tail is cut off (rows and chain deleted): the head is ahead.
    let conn = raw(&dirs);
    drop_triggers(&conn);
    let cut: i64 = conn
        .query_row("SELECT MAX(seq) FROM chain", [], |r| r.get(0))
        .unwrap();
    for t in ["journal", "chain"] {
        conn.execute(&["DELETE FROM ", t, " WHERE seq = ?1"].concat(), [cut])
            .unwrap();
    }
    drop(conn);
    let (log, opened) = open(&dirs);
    assert_eq!(opened.new_breaks[0].cause, BreakCause::HeadAhead);
    drop(log);

    // The head file is deleted.
    fs::remove_file(&head).unwrap();
    let (log, opened) = open(&dirs);
    assert_eq!(opened.new_breaks[0].cause, BreakCause::HeadMissing);
    drop(log);

    // An old head restored: rows after it are more than one batch.
    fs::write(&head, tip).unwrap();
    let (_, opened) = open(&dirs);
    assert_eq!(opened.new_breaks[0].cause, BreakCause::HeadBehind);
}

#[test]
fn a_crash_between_commit_and_head_write_is_not_a_break() {
    let (_tmp, dirs) = profile();
    let head = repo_dir(&dirs, REPO).unwrap().join(HEAD_FILE);
    let (mut log, _) = open(&dirs);
    let op = log.record_operation(&protected(&[WT], 1), 1).unwrap();
    let before = fs::read(&head).unwrap();
    log.advance_operation(&op, OperationTransition::Rejected { reason: "overlap" }, 2)
        .unwrap();
    drop(log);
    // The last batch committed but its head never reached the disk.
    fs::write(&head, before).unwrap();
    let (_, opened) = open(&dirs);
    assert!(opened.new_breaks.is_empty());
}

#[test]
fn an_oplog_copied_from_another_repo_does_not_verify() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    drop(log);
    let other = "0a1b2c3d-0000-4000-8000-0000000000ff";
    let from = repo_dir(&dirs, REPO).unwrap();
    let to = repo_dir(&dirs, other).unwrap();
    crate::profile::fsperm::ensure_private_dir(&to).unwrap();
    for f in [OPLOG_FILE, HEAD_FILE] {
        fs::copy(from.join(f), to.join(f)).unwrap();
    }
    let (_, opened) = Oplog::open(&dirs, other, 2).unwrap();
    assert_eq!(opened.new_breaks[0].cause, BreakCause::LinkBroken);
}

#[test]
fn a_corrupt_oplog_is_set_aside_and_declared() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    op_in_state(&mut log, &protected(&[WT], 1), OperationState::Finished, 1);
    drop(log);
    let file = repo_dir(&dirs, REPO).unwrap().join(OPLOG_FILE);
    fs::write(&file, vec![0x42; 8192]).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(PathBuf::from([file.to_str().unwrap(), suffix].concat()));
    }
    let (log, opened) = open(&dirs);
    let OplogStatus::Recovered { quarantined } = &opened.status else {
        panic!("{:?}", opened.status);
    };
    assert!(quarantined.exists());
    assert_eq!(
        opened.new_breaks,
        vec![ChainBreak {
            seq: 0,
            cause: BreakCause::Quarantined
        }]
    );
    assert!(
        log.operations(&OperationFilter::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn repo_keys_that_could_escape_are_refused() {
    let (_tmp, dirs) = profile();
    for bad in ["../x", "", "a/b", "abc def"] {
        assert!(Oplog::open(&dirs, bad, 1).is_err(), "{bad}");
    }
}

// ----- Queries ------------------------------------------------------------

#[test]
fn queries_by_worktree_period_operation_snapshot_and_level() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let op = op_in_state(
        &mut log,
        &protected(&[WT], 1),
        OperationState::Finished,
        100,
    );
    let prior_of_op = log.operation(&op).unwrap().unwrap().prior_snapshot.unwrap();
    let obs = log
        .begin_snapshot(
            &NewSnapshot {
                level: SnapshotLevel::Observation,
                worktrees: vec![WT2.into()],
                engine_mark: Some(9),
                cause_operation: None,
                cause_event_seq: Some(9),
            },
            200,
        )
        .unwrap();
    let mut restore = protected(&[WT2], 2);
    restore.kind = OperationKind::Restore;
    restore.target = Target::Snapshot(obs.clone());
    let r = log.record_operation(&restore, 300).unwrap();

    let ids = |v: Vec<SnapshotView>| {
        v.into_iter()
            .map(|s| s.record.snapshot_id)
            .collect::<Vec<_>>()
    };
    let by = |f: SnapshotFilter| ids(log.snapshots(&f).unwrap());
    assert_eq!(
        by(SnapshotFilter {
            worktree: Some(WT2.into()),
            ..Default::default()
        }),
        vec![obs.clone()]
    );
    assert_eq!(
        by(SnapshotFilter {
            level: Some(SnapshotLevel::Observation),
            ..Default::default()
        }),
        vec![obs.clone()]
    );
    assert_eq!(
        by(SnapshotFilter {
            from_ms: Some(150),
            to_ms: Some(250),
            ..Default::default()
        }),
        vec![obs.clone()]
    );
    assert_eq!(
        by(SnapshotFilter {
            operation_id: Some(op.clone()),
            ..Default::default()
        }),
        vec![prior_of_op.clone()]
    );

    let ops = |f: OperationFilter| {
        log.operations(&f)
            .unwrap()
            .into_iter()
            .map(|o| o.record.operation_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ops(OperationFilter {
            snapshot_id: Some(prior_of_op),
            ..Default::default()
        }),
        vec![op.clone()]
    );
    assert_eq!(
        ops(OperationFilter {
            snapshot_id: Some(obs),
            ..Default::default()
        }),
        vec![r.clone()]
    );
    assert_eq!(
        ops(OperationFilter {
            worktree: Some(WT.into()),
            ..Default::default()
        }),
        vec![op.clone()]
    );
    assert_eq!(
        ops(OperationFilter {
            from_ms: Some(250),
            ..Default::default()
        }),
        vec![r]
    );
    assert_eq!(
        ops(OperationFilter {
            requester_session: Some("s1".into()),
            ..Default::default()
        })
        .len(),
        2
    );
}

#[test]
fn the_purge_grace_starts_at_the_first_cli_or_tui_delivery() {
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    let n = log
        .record_notice(
            NoticeKind::Purge,
            None,
            None,
            &serde_json::json!({ "count": 3 }),
            1,
        )
        .unwrap();
    log.mark_notice_delivered(&n, Channel::Mcp, 5).unwrap();
    assert_eq!(log.first_interactive_delivery_ms(&n).unwrap(), None);
    log.mark_notice_delivered(&n, Channel::Tui, 7).unwrap();
    log.mark_notice_delivered(&n, Channel::Cli, 9).unwrap();
    assert_eq!(log.first_interactive_delivery_ms(&n).unwrap(), Some(7));
    assert!(log.mark_notice_delivered("nope", Channel::Cli, 9).is_err());
}

#[cfg(unix)]
#[test]
fn oplog_folder_and_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, dirs) = profile();
    let (mut log, _) = open(&dirs);
    log.record_operation(&protected(&[WT], 1), 1).unwrap();
    let dir = repo_dir(&dirs, REPO).unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dirs.data.join(TM_DIR)), 0o700);
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(OPLOG_FILE)), 0o600);
    assert_eq!(mode(&dir.join(HEAD_FILE)), 0o600);
}
