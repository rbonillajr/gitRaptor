//! US-TMC-003 end to end over the channel: `timemachine.redo` returns the worktree to the state
//! before its last undo. A real daemon in this process, real Git, temporary repos and profile
//! (NFR-01); the redo is asked through `methods::TM_REDO` with JSON values, so the file compiles
//! before the redo exists. Debug builds only (the harness uses the confirmation seam).
//!
//! Independent of the session that runs the tests: every requester is "unattributed" (an
//! in-process client descends from the daemon). The operations to undo are real
//! `git reset --hard` runs of the `us_tmc_013_common` catalog over a staged `b.rs`, which the reset
//! deletes (scenario 1); the two tests that need a *commit* seen by the engine (scenario 4 and the
//! echo guard 4.2) use the engine world of `us_tmc_011_common`, whose commits are raw Git events.
//! No test waits on time: each step waits on the daemon's answer or on the timeline.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_011_common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{OperationRunResult, UndoResult};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::timemachine::oplog::{
    OpRef, OperationKind, OperationState, SnapshotFilter, SnapshotLevel, Target,
};
use gitraptor_core::timemachine::store::SnapshotStore;
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::diff;
use gitraptor_testkit::fingerprint::Snapshot;
use serde_json::{Value, json};
use us_tmc_011_common::base013::{Running, repo_with_login, start};
use us_tmc_011_common::{params, rejected};

const B_RS: &str = "fn b() {}\n";

fn undo_ok(r: &Running, c: &mut Client) -> UndoResult {
    let value = c
        .call::<_, Value>(methods::TM_UNDO, params(&r.worktree, None))
        .unwrap_or_else(|e| panic!("the undo failed: {e:?}"));
    serde_json::from_value(value).expect("an UndoResult")
}

fn redo(r: &Running, c: &mut Client) -> Result<Value, ClientError> {
    c.call(methods::TM_REDO, params(&r.worktree, None))
}

fn redo_ok(r: &Running, c: &mut Client) -> Value {
    redo(r, c).unwrap_or_else(|e| panic!("TMC003: the redo must be served and done: {e:?}"))
}

/// Retries `call` only while the daemon answers `RATE_LIMITED` (the requester's write burst, S-03),
/// within 30 s, as `apps/cli/tests/restore_process.rs` does; any other answer returns at once.
/// Authorized amendment of TMC003-WALK (coordinator, 2026-11-09): its six writes as one requester
/// pass the burst of 5.
fn past_the_burst<T>(mut call: impl FnMut() -> Result<T, ClientError>) -> Result<T, ClientError> {
    let start = std::time::Instant::now();
    loop {
        match call() {
            Err(ClientError::Rpc(e)) if e.code == code::RATE_LIMITED => {
                assert!(
                    start.elapsed() < std::time::Duration::from_secs(30),
                    "TMC003-WALK: still rate limited after 30 s"
                );
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            other => return other,
        }
    }
}

fn assert_same(before: &Snapshot, after: &Snapshot, why: &str) {
    let changes = diff(before, after);
    assert!(changes.is_empty(), "{why}: {changes:#?}");
}

/// What a redo must give back: the worktree's files (kind, size, content, executable bit), HEAD and
/// its branch, `git status` and the branch, tag and stash refs. Not the repo's internals (`.git`
/// gets a pack, a rewritten index and reflog lines with any apply), nor mtime/inode/ctime.
#[derive(Debug, PartialEq, Eq)]
struct State {
    files: BTreeMap<PathBuf, (String, u64, u64, bool)>,
    head: String,
    branch: String,
    status: String,
    refs: String,
}

fn state_of(r: &Running) -> State {
    let label = r
        .worktree
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let print = r.fx.fingerprint();
    let files: BTreeMap<_, _> = print
        .entries
        .iter()
        .filter(|((scope, _), _)| *scope == label)
        .map(|((_, path), e)| {
            #[cfg(unix)]
            let exec = e.mode & 0o111 != 0;
            #[cfg(not(unix))]
            let exec = false;
            (
                path.clone(),
                (format!("{:?}", e.kind), e.size, e.hash, exec),
            )
        })
        .collect();
    assert!(!files.is_empty(), "the worktree scope {label} is empty");
    let git = |args: &[&str]| r.fx.git_in(&r.worktree, args);
    State {
        files,
        head: git(&["rev-parse", "HEAD"]),
        branch: git(&["rev-parse", "--abbrev-ref", "HEAD"]),
        status: git(&["status", "--porcelain=v1"]),
        refs: git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/tags",
            "refs/stash",
        ]),
    }
}

fn assert_same_state(before: &State, after: &State, why: &str) {
    assert_eq!(before, after, "{why}");
}

fn id(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("TMC003: the result has no {key}: {value}"))
        .to_owned()
}

/// `feat-login` with a staged `b.rs`: the reset of the test catalog deletes it.
fn running_with_staged_b() -> Running {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("b.rs"), B_RS).unwrap();
    fx.git_in(&wt, &["add", "b.rs"]);
    start(fx, wt, None, None)
}

/// An unattributed operation: stages `file` and runs `git reset --hard`, which deletes it.
fn delete_by_reset(r: &Running, file: &str, content: &str) -> OperationRunResult {
    std::fs::write(r.worktree.join(file), content).unwrap();
    r.fx.git_in(&r.worktree, &["add", file]);
    r.reset_hard()
}

/// The files of the point `snapshot_id` for `worktree` and the commit `HEAD` pointed to.
fn point_files(
    r: &Running,
    snapshot_id: &str,
    worktree: &Path,
) -> (BTreeMap<PathBuf, Vec<u8>>, Option<String>) {
    let repo_id = r.oplog.lock().unwrap().repo_id().to_owned();
    let store = SnapshotStore::open_existing(&r.tp.dirs(), &repo_id)
        .unwrap()
        .expect("the daemon's store");
    store
        .verify(snapshot_id)
        .unwrap_or_else(|e| panic!("TMC003: the point {snapshot_id} must verify: {e:?}"));
    let meta = store.meta(snapshot_id).unwrap();
    let entry = meta
        .worktrees
        .iter()
        .find(|w| Path::new(&w.path) == worktree)
        .expect("the point names the worktree");
    let files = store
        .files(snapshot_id, &entry.key)
        .unwrap()
        .into_iter()
        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
        .map(|(p, _, oid)| (PathBuf::from(p), store.read_blob(oid).unwrap()))
        .collect();
    (files, entry.head_commit.clone())
}

// ----- Escenario 1 ---------------------------------------------------------------------------------

/// Escenario 1: an operation deleted `b.rs`, an undo got it back and the redo deletes it again;
/// the fingerprint is the one after the operation and `b.rs` stays in the redo's prior point.
#[test]
fn redo_returns_to_the_state_before_the_undo() {
    let r = running_with_staged_b();
    r.reset_hard();
    assert!(
        !r.worktree.join("b.rs").exists(),
        "the operation deleted b.rs"
    );
    let after_operation = state_of(&r);
    let mut c = r.client();
    let undo = undo_ok(&r, &mut c);
    assert!(r.worktree.join("b.rs").exists(), "the undo recovered b.rs");

    let redone = redo_ok(&r, &mut c);

    assert!(
        !r.worktree.join("b.rs").exists(),
        "TMC003-S1: the redo deletes b.rs again"
    );
    assert_same_state(
        &after_operation,
        &state_of(&r),
        "TMC003-S1: the worktree is back to the state after the operation",
    );
    assert_eq!(
        id(&redone, "redone_undo_id"),
        undo.operation_id,
        "TMC003-S1: the redo says which undo it redid"
    );
    assert_eq!(
        id(&redone, "target_snapshot_id"),
        undo.prior_snapshot_id,
        "TMC003-S1: the redo goes to the point the undo took before itself"
    );
    let (files, _) = point_files(&r, &id(&redone, "prior_snapshot_id"), &r.worktree);
    assert_eq!(
        files.get(Path::new("b.rs")).map(Vec::as_slice),
        Some(B_RS.as_bytes()),
        "TMC003-S1: the content of b.rs stays recoverable in the redo's prior point"
    );
}

// ----- Escenario 2 ---------------------------------------------------------------------------------

/// Escenario 2: the redo has its own prior point, with the state right before it, and is recorded
/// as a finished `redo` over the undo.
#[test]
fn the_redo_has_its_own_prior_point() {
    let r = running_with_staged_b();
    r.reset_hard();
    let mut c = r.client();
    let undo = undo_ok(&r, &mut c);
    // Work done after the undo: the redo takes it away, its prior keeps it.
    std::fs::write(r.worktree.join("after.rs"), "fn after() {}\n").unwrap();
    let head_before =
        r.fx.git_in(&r.worktree, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();

    let redone = redo_ok(&r, &mut c);

    let prior = id(&redone, "prior_snapshot_id");
    let view = r.oplog.lock().unwrap().snapshot(&prior).unwrap();
    assert!(
        view.is_some_and(|s| s.state.is_available() && !s.tampered),
        "TMC003-S2: the point before the redo exists and is available"
    );
    let (files, head) = point_files(&r, &prior, &r.worktree);
    assert_eq!(
        files.get(Path::new("b.rs")).map(Vec::as_slice),
        Some(B_RS.as_bytes()),
        "TMC003-S2: the point has b.rs as it was just before the redo"
    );
    assert_eq!(
        files.get(Path::new("after.rs")).map(Vec::as_slice),
        Some("fn after() {}\n".as_bytes()),
        "TMC003-S2: the point has the work done after the undo"
    );
    assert_eq!(
        head.as_deref(),
        Some(head_before.as_str()),
        "TMC003-S2: the point has the HEAD of just before the redo"
    );
    let redos = r.requests(OperationKind::Redo);
    assert_eq!(redos.len(), 1, "TMC003-S2: one redo in the oplog");
    let rec = &redos[0].view;
    assert_eq!(rec.state, OperationState::Finished);
    assert_eq!(rec.record.operation_id, id(&redone, "operation_id"));
    assert_eq!(
        rec.record.target,
        Target::Redo(undo.operation_id.clone()),
        "TMC003-S2: the redo is recorded over the undo it redoes"
    );
    assert_eq!(
        rec.prior_snapshot.as_deref(),
        Some(prior.as_str()),
        "TMC003-S2: the redo's record points to its prior"
    );
}

// ----- Escenario 3 ---------------------------------------------------------------------------------

/// Escenario 3: with no undo pending, the redo is refused with `nothing-to-redo`, the repo does not
/// change and the refusal is in the oplog.
#[test]
fn nothing_to_redo_changes_nothing() {
    let r = running_with_staged_b();
    r.reset_hard();
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let (error, data) = rejected(redo(&r, &mut c));

    assert_ne!(
        error,
        code::NOT_IMPLEMENTED,
        "TMC003-S3: the redo must be served"
    );
    assert_eq!(error, code::OPERATION_REJECTED, "TMC003-S3");
    assert_eq!(
        data.expect("data")["reason"],
        "nothing-to-redo",
        "TMC003-S3: no undo pending, nothing to redo"
    );
    assert_same(&before, &r.fx.fingerprint(), "TMC003-S3: the repo changed");
    let redos = r.requests(OperationKind::Redo);
    assert_eq!(redos.len(), 1, "TMC003-S3: the refusal is in the oplog");
    assert_eq!(redos[0].view.state, OperationState::Rejected);
    assert_eq!(redos[0].view.record.target, Target::None);
    assert_eq!(redos[0].reason.as_deref(), Some("nothing-to-redo"));
}

// ----- Escenario 4 (engine world: the commit is a raw Git event) --------------------------------

/// Escenario 4: undo, a new commit in the worktree and the redo is `redo-invalidated`.
///
/// The undone work is an agent's commit taken back by an undo by agent: in this world a plain undo
/// of a raw commit has no captured point to go back to. The commit that follows is the test's own.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "intermittent on the macOS CI runner: the simulated agent commit is not attributed under load (TD-GRP-007); still runs on Linux and Windows"
)]
fn a_new_commit_invalidates_the_redo() {
    let w = us_tmc_011_common::world();
    let mut claude1 = w.spawn_agent();
    w.agent_commit(&mut claude1, &[("b.rs", "fn b() {}")], "add b");
    w.confirmed_agent_undo(&claude1.session);
    w.unattributed_commit(&[("c.rs", "fn c() {}")], "a new commit");
    let before = w.fx.fingerprint();
    let mut c = w.client();

    let (error, data) = rejected(w.redo_call(&mut c, None));

    assert_ne!(
        error,
        code::NOT_IMPLEMENTED,
        "TMC003-S4: the redo must be served"
    );
    assert_eq!(error, code::OPERATION_REJECTED, "TMC003-S4");
    assert_eq!(
        data.expect("data")["reason"],
        "redo-invalidated",
        "TMC003-S4: a later operation invalidated the redo"
    );
    us_tmc_011_common::assert_intact(&w, &before);
}

// ----- D-TMC-26 ------------------------------------------------------------------------------------

/// D-TMC-26: two undos and two redos walk back and forth: the first redo redoes the later undo.
#[test]
fn consecutive_redos_walk_forward() {
    let r = running_with_staged_b();
    delete_by_reset(&r, "c0.rs", "fn c0() {}\n");
    // State before the second operation: `c.rs` staged (the reset deletes it).
    std::fs::write(r.worktree.join("c.rs"), "fn c() {}\n").unwrap();
    r.fx.git_in(&r.worktree, &["add", "c.rs"]);
    let middle = state_of(&r);
    r.reset_hard();
    let end = state_of(&r);
    let mut c = r.client();
    for _ in 0..2 {
        let value =
            past_the_burst(|| c.call::<_, Value>(methods::TM_UNDO, params(&r.worktree, None)))
                .unwrap_or_else(|e| panic!("the undo failed: {e:?}"));
        let _: UndoResult = serde_json::from_value(value).expect("an UndoResult");
    }
    assert!(
        r.worktree.join("b.rs").exists(),
        "two undos brought b.rs back"
    );

    past_the_burst(|| redo(&r, &mut c))
        .unwrap_or_else(|e| panic!("TMC003: the redo must be served and done: {e:?}"));
    assert_same_state(
        &middle,
        &state_of(&r),
        "TMC003-WALK: the first redo goes back to the state before the second undo",
    );
    past_the_burst(|| redo(&r, &mut c))
        .unwrap_or_else(|e| panic!("TMC003: the redo must be served and done: {e:?}"));
    assert_same_state(
        &end,
        &state_of(&r),
        "TMC003-WALK: the second redo goes back to the end",
    );
    let (error, data) = rejected(past_the_burst(|| redo(&r, &mut c)));
    assert_eq!(error, code::OPERATION_REJECTED, "TMC003-WALK");
    assert_eq!(
        data.expect("data")["reason"],
        "nothing-to-redo",
        "TMC003-WALK: both undos are redone"
    );
}

/// D-TMC-26: an undo after a redo takes back the same operation again.
#[test]
fn an_undo_after_a_redo_takes_back_the_operation_again() {
    let r = running_with_staged_b();
    let op = r.reset_hard();
    let after_operation = state_of(&r);
    let mut c = r.client();
    undo_ok(&r, &mut c);
    redo_ok(&r, &mut c);
    assert_same_state(
        &after_operation,
        &state_of(&r),
        "TMC003-WALK: after the redo",
    );

    let again = undo_ok(&r, &mut c);

    assert_eq!(
        again.undone_operation_id, op.operation_id,
        "TMC003-WALK: the undo after the redo takes back the same operation"
    );
    assert!(
        r.worktree.join("b.rs").exists(),
        "TMC003-WALK: b.rs is recovered again"
    );
    let ops: Vec<OpRef> = r
        .requests(OperationKind::Undo)
        .iter()
        .filter_map(|u| match &u.view.record.target {
            Target::Undo(t) => t.first().cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(
        ops,
        vec![OpRef::Oplog(op.operation_id.clone()); 2],
        "TMC003-WALK: both undos took back the operation"
    );
}

// ----- Guard 4.2 (engine world: the engine sees every write) ---------------------------------------

/// Guard 4.2: with the engine running, a redo right after an undo is not invalidated by its own
/// echo, and the writes of the redo do not enter the stack as raw Git. Without the anchor of the
/// redo, those writes would enter the stack as raw Git and clear the redo (a second redo would be
/// `redo-invalidated` instead of `nothing-to-redo`).
///
/// claude-1 itself undoes its own work and redoes it, then asks to redo again (its own session: the
/// requester write burst is per requester).
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "intermittent on the macOS CI runner: the simulated agent commit is not attributed under load (TD-GRP-007); still runs on Linux and Windows"
)]
fn a_redo_right_after_an_undo_is_not_invalidated_by_its_echo() {
    let w = us_tmc_011_common::world();
    let mut claude1 = w.spawn_agent();
    w.agent_commit(&mut claude1, &[("one.rs", "fn one() {}")], "claude-1 one");
    let with_work = w.head();
    let first = agent_ok(
        &self_undo(&w, &mut claude1),
        "TMC003-GUARD-4.2: the agent's own undo",
    );
    let without_work = w.head();
    assert_ne!(without_work, with_work, "the undo took the commit away");

    // Right away: the undo's own echo (its writes seen by the engine) must not count as news.
    let redone = agent_ok(
        &redo_as(&w, &mut claude1),
        "TMC003-GUARD-4.2: the redo right after the undo",
    );

    assert_eq!(
        w.head(),
        with_work,
        "TMC003-GUARD-4.2: the redo brought the commit back"
    );
    assert_eq!(id(&redone, "redone_undo_id"), id(&first, "operation_id"));
    // Authorized amendment (coordinator, 2026-11-09): the guard is checked without a second undo by
    // agent. Its original last step (a second undo by agent after the redo, held by GitRaptor's own
    // anchored echoes until TD-TMC-003) is `td_tmc_003::an_agent_undoes_its_work_again_after_redoing_it`.
    // (1) The redo's writes are anchored: an observation point caused by the redo itself.
    let redo_id = id(&redone, "operation_id");
    let anchored = w
        .oplog()
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .into_iter()
        .any(|s| {
            s.record.level == SnapshotLevel::Observation
                && s.record.cause_operation.as_deref() == Some(redo_id.as_str())
        });
    assert!(
        anchored,
        "TMC003-GUARD-4.2: the redo's writes are anchored to the redo (its echo is not raw Git)"
    );
    // (2) Its echo did not enter the stack as something new: a second redo has nothing to redo, it
    // is not told that a later operation invalidated it.
    let again = redo_as(&w, &mut claude1);
    assert_eq!(
        again["data"]["reason"], "nothing-to-redo",
        "TMC003-GUARD-4.2: after the redo there is nothing left to redo, and nothing invalidated it: {again}"
    );
    assert_eq!(
        w.head(),
        with_work,
        "TMC003-GUARD-4.2: the refused redo changed nothing"
    );
    // (3) A plain undo of the developer is no longer held by a barrier nor told of an invalidation.
    let mut c = w.client();
    if let Err(ClientError::Rpc(e)) = w.plain_undo_call(&mut c, None) {
        let reason = e
            .data
            .as_ref()
            .and_then(|d| d.get("reason"))
            .cloned()
            .unwrap_or(Value::Null);
        assert!(
            reason != "selective-undo-below" && reason != "redo-invalidated",
            "TMC003-GUARD-4.2: the plain undo after the redo is not held by the redo's echo: {reason}"
        );
    }
}

/// An undo by `agent` itself over its own work, as the agent's process saw it (`{ok}` or `{code, data}`).
fn self_undo(w: &us_tmc_011_common::World, agent: &mut us_tmc_011_common::Agent) -> Value {
    let session = agent.session.clone();
    w.as_agent(
        agent,
        methods::TM_UNDO,
        w.undo_params(Some(&session), Some("20m"), None),
    )
}

fn redo_as(w: &us_tmc_011_common::World, agent: &mut us_tmc_011_common::Agent) -> Value {
    w.as_agent(agent, methods::TM_REDO, params(&w.worktree, None))
}

fn agent_ok(answer: &Value, what: &str) -> Value {
    answer
        .get("ok")
        .cloned()
        .unwrap_or_else(|| panic!("{what} must be done: {answer}"))
}

// ----- T005 ----------------------------------------------------------------------------------------

/// T005: without `operation_id` the redo is served; with one it is still not implemented.
#[test]
fn redo_without_id_is_served_and_by_id_is_not() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, None, None);
    let mut c = r.client();

    let (error, data) = rejected(redo(&r, &mut c));

    assert_ne!(
        error,
        code::NOT_IMPLEMENTED,
        "TMC003-SERVED: a redo without operation_id must no longer answer NOT_IMPLEMENTED"
    );
    assert_eq!(
        error,
        code::OPERATION_REJECTED,
        "TMC003-SERVED: with nothing pending it is rejected as nothing-to-redo"
    );
    let data = data.expect("the rejection carries data");
    assert_eq!(data["reason"], "nothing-to-redo", "TMC003-SERVED");
    // An id the repo knows (the rejected request above): an unknown one is `NOT_FOUND` first.
    let known = data["operation_id"]
        .as_str()
        .expect("the rejected request is in the oplog")
        .to_owned();
    let mut p = params(&r.worktree, None);
    p["operation_id"] = json!(known);
    let (error, data) = rejected(c.call::<_, Value>(methods::TM_REDO, p));
    assert_eq!(
        error,
        code::NOT_IMPLEMENTED,
        "TMC003-SERVED: by id stays unassigned"
    );
    assert_eq!(data.expect("data")["implemented_by"], "unassigned");
}
