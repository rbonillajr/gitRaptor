//! US-TMC-012 end to end over the daemon channel: an undo never overwrites later work of another
//! actor. A real daemon in this process, real Git, temporary repos and profile (NFR-01). Debug
//! builds only (the confirmation seam).
//!
//! The agent's operation comes from `agent_work` (with the anchor T005 adds). "A change nobody
//! attributed" is a write by the test followed by an explicit capture, so nothing waits on time.
//! The tests keep to the wire contract (JSON over the channel) except the applier's last-instant
//! test, which needs the applier's own hook. The worktree file the scenarios call `api.rs` is the
//! harness's `a.rs`.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod tm_common;
mod us_tmc_013_common;

use std::collections::BTreeSet;
use std::path::Path;

use gitraptor_api::actor::Actor;
use gitraptor_api::rpc::code;
use gitraptor_api::timemachine::{
    OverlapSubject, TmConfirmData, TmRejectReason, UndoParams, UndoResult,
};
use gitraptor_api::{capability, methods};
use gitraptor_core::channel::TestConfirmation;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::timemachine::apply::{
    Applier, ApplyHooks, ApplyPlan, PathIssue, PlanWorktree, RefScope,
};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationState, OperationTransition, Requester,
    RequesterOrigin, Scope, SnapshotFilter, SnapshotLevel, Target,
};
use gitraptor_core::timemachine::store::{CaptureRequest, SnapshotStore, WorktreeScope};
use gitraptor_testkit::diff;
use serde_json::{Value, json};
use us_tmc_013_common::*;

const LATER: &str = "fn a() { later_edit_of_another_actor(); }\n";
const B_RS: &str = "fn b() { somebody_elses(); }\n";

fn system_git() -> gitraptor_git::SystemGit {
    use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
    match resolve::resolve(
        &ResolveConfig::for_current_os(None),
        &gitraptor_git::Invoker::default(),
    ) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git >= 2.38: {diagnostics:?}"),
    }
}

fn running() -> Running {
    let (fx, wt) = repo_with_login();
    start(fx, wt, Some(TestConfirmation::Eligible), None)
}

fn undo_raw(c: &mut Client, wt: &Path, token: Option<&str>) -> Result<Value, ClientError> {
    c.call(methods::TM_UNDO, params(wt, token))
}

/// The undo of the work of an agent, confirmed by the developer (unattributed): the request, then
/// the same request with the challenge's token.
fn undo_through(c: &mut Client, wt: &Path) -> Result<Value, ClientError> {
    match undo_raw(c, wt, None) {
        Err(ClientError::Rpc(e)) if e.code == code::OPERATION_REJECTED => {
            let token = e
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<TmConfirmData>(d).ok())
                .and_then(|d| d.challenge)
                .map(|c| c.token);
            match token {
                Some(token) => undo_raw(c, wt, Some(&token)),
                None => Err(ClientError::Rpc(e)),
            }
        }
        other => other,
    }
}

fn snapshot_count(r: &Running) -> usize {
    r.oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap()
        .len()
}

fn assert_intact(r: &Running, before: &gitraptor_testkit::fingerprint::Snapshot) {
    let changes = diff(before, &r.fx.fingerprint());
    assert!(changes.is_empty(), "the repo changed: {changes:#?}");
}

/// An observation of the worktree as it is now: the capture the continuous observation would
/// take. Its mark is the greatest of the oplog plus one, so it is after every point there is.
fn capture_now(r: &Running, cause: Option<&str>) -> String {
    let (repo_id, key, mark) = {
        let log = r.oplog.lock().unwrap();
        let all = log.snapshots(&SnapshotFilter::default()).unwrap();
        let key = all
            .iter()
            .find_map(|s| s.record.worktrees.first().cloned())
            .unwrap_or_else(|| "wt-wt-feat-login".into());
        let mark = all
            .iter()
            .filter_map(|s| s.record.engine_mark)
            .max()
            .unwrap_or(0)
            .max(if cause.is_some() { i64::MAX / 2 } else { 0 })
            + 1;
        (log.repo_id().to_owned(), key, mark)
    };
    let (store, _) = SnapshotStore::open_or_create(&r.tp.dirs(), &repo_id).unwrap();
    let req = CaptureRequest {
        level: SnapshotLevel::Observation,
        echo_settled: None,
        common_dir: gitraptor_core::observe::locate(&r.fx.repo).unwrap(),
        worktrees: vec![WorktreeScope {
            key,
            path: r.worktree.clone(),
            hint: None,
        }],
        engine_mark: Some(mark),
        cause_operation: cause.map(str::to_owned),
        cause_event_seq: None,
        include_credentials: false,
        still_valid: None,
        give_way: None,
    };
    store.capture(&r.oplog, &req).unwrap().snapshot_id
}

/// An agent's operation like `agent_work`, but declaring `refs` in its scope and running `effect`
/// (what the operation did besides the reset) before its anchor is taken.
fn agent_op(r: &Running, refs: &[&str], effect: impl FnOnce()) -> AgentWork {
    let dirty = "fn a() { trabajo_del_agente_de_las_10(); }\n".to_owned();
    std::fs::write(r.worktree.join("a.rs"), &dirty).unwrap();
    let real = r.reset_hard();
    effect();
    let id = {
        let mut log = r.oplog.lock().unwrap();
        let id = log
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Protected,
                    subtype: Some("reset-hard".into()),
                    scope: Scope {
                        worktrees: vec![r.worktree.to_string_lossy().into_owned()],
                        refs: refs.iter().map(|x| (*x).to_owned()).collect(),
                    },
                    requester: Requester::Agent {
                        name: "claude-code".into(),
                        origin: RequesterOrigin::Detected,
                        session_id: AGENT_SESSION.into(),
                    },
                    channel: Channel::Mcp,
                    confirmed: false,
                    target: Target::None,
                    warnings: Vec::new(),
                    engine_mark: i64::MAX / 2,
                },
                1,
            )
            .unwrap();
        for step in [
            OperationTransition::PriorSnapshot {
                snapshot_id: &real.prior_snapshot_id,
            },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
            OperationTransition::Finished,
        ] {
            log.advance_operation(&id, step, 2).unwrap();
        }
        id
    };
    capture_now(r, Some(&id));
    AgentWork {
        operation_id: id,
        prior_snapshot_id: real.prior_snapshot_id,
        dirty,
    }
}

fn commit_on_head(r: &Running, message: &str) -> String {
    r.fx.git(&["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", message])
        .trim()
        .to_owned()
}

fn tip(r: &Running, branch: &str) -> String {
    r.fx.git(&["rev-parse", &format!("refs/heads/{branch}")])
        .trim()
        .to_owned()
}

fn move_branch(r: &Running, branch: &str, to: &str) {
    r.fx.git(&["update-ref", &format!("refs/heads/{branch}"), to]);
}

fn overlap_of(data: &TmConfirmData) -> &gitraptor_api::timemachine::TmOverlap {
    data.overlap
        .as_ref()
        .expect("the detail of the overlap (the connection has the capability)")
}

fn last_undo_request(r: &Running) -> Recorded {
    r.requests(OperationKind::Undo).pop().expect("a request")
}

/// Escenario 1: an operation of claude-1 on `api.rs`, then an unattributed change of `api.rs`.
#[test]
fn an_overlap_stops_the_undo_and_lists_both_changes() {
    let r = running();
    let work = r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    capture_now(&r, None);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(undo_through(&mut c, &r.worktree));

    assert_eq!(data.reason, TmRejectReason::Overlap);
    let overlap = overlap_of(&data);
    assert_eq!(
        overlap.undone.id, work.operation_id,
        "the change asked to undo"
    );
    assert!(
        matches!(overlap.undone.actor, Actor::Agent { .. }),
        "claude-1: {:?}",
        overlap.undone.actor
    );
    assert!(overlap.undone.at_utc_ms.is_some(), "and its moment");
    assert_eq!(overlap.total, 1);
    assert_eq!(overlap.conflicts.len(), 1, "{:?}", overlap.conflicts);
    let conflict = &overlap.conflicts[0];
    match &conflict.subject {
        OverlapSubject::Path { path, .. } => assert_eq!(path.raw(), "a.rs"),
        other => panic!("a path conflict expected, got {other:?}"),
    }
    assert_eq!(conflict.actor, Actor::Unattributed);
    assert!(conflict.seen_utc_ms > 0, "the moment it was seen");
    assert_eq!(r.a_rs(), LATER);
    assert_intact(&r, &before);
    let last = last_undo_request(&r);
    assert_eq!(last.view.state, OperationState::Rejected);
    assert_eq!(last.reason.as_deref(), Some("overlap"));
}

/// Escenario 2: the later change is in another file; the undo proceeds and leaves it.
#[test]
fn a_later_change_in_another_file_is_kept() {
    let r = running();
    let work = r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("b.rs"), B_RS).unwrap();
    capture_now(&r, None);
    let mut c = r.client();

    let value = undo_through(&mut c, &r.worktree).expect("the undo proceeds");

    let done: UndoResult = serde_json::from_value(value).unwrap();
    assert_eq!(done.undone_operation_id, work.operation_id);
    assert_eq!(r.a_rs(), work.dirty, "the agent's operation is taken back");
    assert_eq!(
        std::fs::read_to_string(r.worktree.join("b.rs")).unwrap(),
        B_RS,
        "the other actor's change is intact"
    );
    assert!(
        done.kept.iter().any(|k| k.path.raw() == "b.rs"),
        "b.rs is reported as kept: {:?}",
        done.kept
    );
}

/// Escenario 3: nothing changes after a stop, not even by closing the client.
#[test]
fn a_stopped_undo_leaves_the_repo_exactly_as_before() {
    let r = running();
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    capture_now(&r, None);
    let before = r.fx.fingerprint();
    let points = snapshot_count(&r);
    let mut c = r.client();

    let data = confirm_data(undo_through(&mut c, &r.worktree));
    assert_eq!(data.reason, TmRejectReason::Overlap);
    assert_intact(&r, &before);
    drop(c);

    assert_intact(&r, &before);
    assert!(
        snapshot_count(&r) <= points + 1,
        "no new point but, at most, the undo's own prior of phase 2"
    );
    let last = last_undo_request(&r);
    assert_eq!(last.view.state, OperationState::Rejected);
}

/// Escenario 3: no way to go on despite the overlap.
#[test]
fn a_stopped_undo_offers_no_way_to_continue() {
    let r = running();
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    capture_now(&r, None);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let (error, data) = rejected(undo_through(&mut c, &r.worktree));

    assert_eq!(error, code::OPERATION_REJECTED);
    let data = data.expect("data");
    assert_eq!(data["reason"], "overlap");
    assert!(
        data.get("challenge").is_none(),
        "no challenge to continue: {data}"
    );
    let text = data.to_string().to_lowercase();
    for word in ["continue", "force", "override"] {
        assert!(!text.contains(word), "the response offers {word:?}: {text}");
    }
    // The contract has no parameter to force it.
    for extra in ["force", "continue", "overlap", "ignore_overlap"] {
        let mut p = params(&r.worktree, None);
        p[extra] = json!(true);
        assert!(
            serde_json::from_value::<UndoParams>(p.clone()).is_err(),
            "UndoParams accepts {extra}"
        );
        // The daemon limits writes per requester (burst of 5, checked before the params) and
        // the undo above used two: only two of the probes go through the connection.
        if ["force", "overlap"].contains(&extra) {
            let (error, _) = rejected(c.call(methods::TM_UNDO, p));
            assert_eq!(error, code::INVALID_PARAMS, "{extra}");
        }
    }
    assert_intact(&r, &before);
    assert_eq!(r.a_rs(), LATER);
}

/// Escenario 4: a change before the operation does not count.
#[test]
fn an_earlier_change_does_not_count() {
    let r = running();
    std::fs::write(r.worktree.join("a.rs"), "fn a() { earlier_change(); }\n").unwrap();
    capture_now(&r, None);
    let work = r.agent_work(AGENT_SESSION);
    let mut c = r.client();

    let value = undo_through(&mut c, &r.worktree).expect("an earlier change does not stop it");

    let done: UndoResult = serde_json::from_value(value).unwrap();
    assert_eq!(done.undone_operation_id, work.operation_id);
    assert_eq!(r.a_rs(), work.dirty, "a.rs is as before the operation");
    assert!(done.kept.is_empty(), "{:?}", done.kept);
}

/// Fase 2 (D1): a write nobody captured right before the undo is seen by the tail.
#[test]
fn an_uncaptured_change_is_caught_by_the_tail() {
    let r = running();
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(undo_through(&mut c, &r.worktree));

    assert_eq!(data.reason, TmRejectReason::Overlap);
    assert_eq!(r.a_rs(), LATER);
    assert_intact(&r, &before);
    let last = last_undo_request(&r);
    assert_eq!(last.view.state, OperationState::Rejected);
    assert_eq!(last.reason.as_deref(), Some("overlap"));
    let log = r.oplog.lock().unwrap();
    let states: Vec<String> = log
        .journal(&last.view.record.operation_id)
        .unwrap()
        .into_iter()
        .filter_map(|j| j.state)
        .collect();
    let ready = states.iter().position(|s| s == "ready");
    let rejected_at = states.iter().position(|s| s == "rejected");
    assert!(
        ready.is_some() && rejected_at > ready,
        "the tail runs after the prior: ready, then rejected: {states:?}"
    );
}

/// The applier's last-instant protection with `keep`: a write during the application is an
/// `overlap` of `not_restored`, not a rejection, and the kept paths are still left alone.
#[test]
fn a_write_during_the_application_is_the_applier_overlap() {
    use gitraptor_core::timemachine::oplog::Oplog;
    let env = tm_common::Env::busy();
    let write = gitraptor_git::tm_write::WriteContext::new(
        system_git(),
        gitraptor_git::Invoker::default(),
        &env.tm_root().join(tm_common::REPO_ID),
    )
    .unwrap();
    let target = env.capture(SnapshotLevel::Observation, None).snapshot_id;
    env.f.write("a.txt", "someone else, after the target\n");
    env.f.write("b.txt", "kept, changed by another actor\n");
    let op = env
        .oplog
        .lock()
        .unwrap()
        .record_operation(
            &NewOperation {
                kind: OperationKind::Restore,
                subtype: None,
                scope: Scope {
                    worktrees: vec![env.f.repo.display().to_string()],
                    refs: vec![],
                },
                requester: Requester::Unattributed,
                channel: Channel::Cli,
                confirmed: true,
                target: Target::Snapshot(target.clone()),
                warnings: vec![],
                engine_mark: 1,
            },
            10,
        )
        .unwrap();
    let mut req = env.request(SnapshotLevel::GuaranteedPrior, None);
    req.cause_operation = Some(op.clone());
    let prior = env.store.capture(&env.oplog, &req).unwrap().snapshot_id;
    {
        let mut log: std::sync::MutexGuard<'_, Oplog> = env.oplog.lock().unwrap();
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
    }
    let plan = ApplyPlan {
        target_snapshot: target,
        prior_snapshot: prior,
        worktrees: vec![PlanWorktree {
            key: "main".into(),
            root: env.f.repo.clone(),
            recreate_id: None,
            keep: BTreeSet::from(["b.txt".to_owned()]),
        }],
        refs: RefScope::All,
    };
    let hooks = ApplyHooks {
        before_exchange: Some(Box::new(|root: &Path, rel: &[u8]| {
            if rel == b"a.txt" {
                std::fs::write(root.join("a.txt"), b"written during the application\n").unwrap();
            }
        })),
        ..ApplyHooks::default()
    };

    let report = Applier::new(
        &env.store,
        &write,
        &env.oplog,
        env.f.repo.clone(),
        gitraptor_core::observe::locate(&env.f.repo).unwrap(),
        env.f.profile.clone(),
    )
    .with_clock(|| 20)
    .with_hooks(hooks)
    .apply(&op, &plan)
    .expect("a write during the application is not a rejection");

    assert!(
        report
            .paths
            .iter()
            .any(|p| p.path == "a.txt" && matches!(p.issue, PathIssue::Overlap { .. })),
        "the written path is reported as overlap: {report:?}"
    );
    assert_eq!(
        std::fs::read(env.f.repo.join("a.txt")).unwrap(),
        b"written during the application\n"
    );
    assert_eq!(
        std::fs::read(env.f.repo.join("b.txt")).unwrap(),
        b"kept, changed by another actor\n",
        "a kept path is left alone"
    );
    assert!(
        report.kept.iter().any(|(_, p)| p == "b.txt"),
        "{:?}",
        report.kept
    );
}

/// A branch the undone operation moved and another actor moved again is an overlap.
#[test]
fn a_branch_moved_by_another_actor_is_an_overlap() {
    let r = running();
    let by_agent = commit_on_head(&r, "moved by the agent's operation");
    let by_other = commit_on_head(&r, "moved by another actor");
    let work = agent_op(&r, &["refs/heads/feat-login"], || {
        move_branch(&r, "feat-login", &by_agent);
    });
    assert!(!work.operation_id.is_empty());
    move_branch(&r, "feat-login", &by_other);
    capture_now(&r, None);
    let before = r.fx.fingerprint();
    let mut c = r.client();

    let data = confirm_data(undo_through(&mut c, &r.worktree));

    assert_eq!(data.reason, TmRejectReason::Overlap);
    let overlap = overlap_of(&data);
    assert!(
        overlap.conflicts.iter().any(|c| matches!(
            &c.subject,
            OverlapSubject::Ref { name }
                if name.raw() == "feat-login" || name.raw() == "refs/heads/feat-login"
        )),
        "the branch is a conflict: {:?}",
        overlap.conflicts
    );
    assert_eq!(
        tip(&r, "feat-login"),
        by_other,
        "the branch stays where it is"
    );
    assert_intact(&r, &before);
}

/// Guard 5.1: a branch the undo keeps, checked out in a worktree outside its scope, does not make
/// it `ref-in-use`.
#[test]
fn a_kept_branch_checked_out_elsewhere_is_not_ref_in_use() {
    let r = running();
    r.fx.git(&["branch", "feat-pagos"]);
    let _pagos = r.fx.add_worktree("feat-pagos", "feat-pagos");
    let by_other = commit_on_head(&r, "moved by another actor");
    // The undone operation names the branch in its scope but does not move it.
    let work = agent_op(&r, &["refs/heads/feat-pagos"], || {});
    move_branch(&r, "feat-pagos", &by_other);
    capture_now(&r, None);
    let mut c = r.client();

    let value = undo_through(&mut c, &r.worktree)
        .expect("a branch the undo does not move cannot block it (ref-in-use)");

    let done: UndoResult = serde_json::from_value(value).unwrap();
    assert_eq!(done.undone_operation_id, work.operation_id);
    assert_eq!(r.a_rs(), work.dirty);
    assert_eq!(
        tip(&r, "feat-pagos"),
        by_other,
        "the branch another actor moved is left where they put it"
    );
}

/// D4: an unattributed owner's undo ignores unattributed edits (one class), so it proceeds and
/// overwrites nothing it should keep.
#[test]
fn an_unattributed_undo_ignores_unattributed_edits() {
    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    let dirty = "fn a() { unattributed_work(); }\n";
    std::fs::write(r.worktree.join("a.rs"), dirty).unwrap();
    r.reset_hard();
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    capture_now(&r, None);
    let mut c = r.client();

    let value = undo_raw(&mut c, &r.worktree, None)
        .expect("the owner is unattributed: no confirmation, no overlap");

    let done: UndoResult = serde_json::from_value(value).unwrap();
    assert_eq!(r.a_rs(), dirty, "taken back to before the reset");
    assert!(done.kept.is_empty(), "{:?}", done.kept);
}

/// D8: the detail travels only to a connection with `CAP_TM_OVERLAP`.
#[test]
fn overlap_detail_needs_the_capability() {
    // With the capability (the default): detail and `kept`.
    let r = running();
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    capture_now(&r, None);
    let mut c = r.client();
    let (_, data) = rejected(undo_through(&mut c, &r.worktree));
    let data = data.expect("data");
    assert_eq!(data["reason"], "overlap");
    assert!(data.get("overlap").is_some(), "with the capability: {data}");

    let r = running();
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("b.rs"), B_RS).unwrap();
    capture_now(&r, None);
    let mut c = r.client();
    let done = undo_through(&mut c, &r.worktree).unwrap();
    assert!(done.get("kept").is_some(), "with the capability: {done}");

    // Without it: only the code, and no `kept`.
    let served: Vec<&'static str> = capability::all()
        .map(|c| c.name)
        .filter(|n| *n != methods::CAP_TM_OVERLAP.name)
        .collect();
    let (fx, wt) = repo_with_login();
    let r = start(
        fx,
        wt,
        Some(TestConfirmation::Eligible),
        Some(served.clone()),
    );
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("a.rs"), LATER).unwrap();
    capture_now(&r, None);
    let mut c = r.client();
    let (error, data) = rejected(undo_through(&mut c, &r.worktree));
    assert_eq!(error, code::OPERATION_REJECTED);
    let data = data.expect("data");
    assert_eq!(data["reason"], "overlap", "the code reaches any connection");
    assert!(
        data.get("overlap").is_none(),
        "without the capability only the code: {data}"
    );

    let (fx, wt) = repo_with_login();
    let r = start(fx, wt, Some(TestConfirmation::Eligible), Some(served));
    r.agent_work(AGENT_SESSION);
    std::fs::write(r.worktree.join("b.rs"), B_RS).unwrap();
    capture_now(&r, None);
    let mut c = r.client();
    let done = undo_through(&mut c, &r.worktree).unwrap();
    assert!(
        done.get("kept").is_none(),
        "without the capability the result has no kept: {done}"
    );
    assert_eq!(
        std::fs::read_to_string(r.worktree.join("b.rs")).unwrap(),
        B_RS,
        "the undo keeps the path all the same"
    );
}

/// The list is capped at `MAX_OVERLAP_CONFLICTS` and `total` is the real count.
#[test]
fn conflicts_are_capped_with_the_real_total() {
    let fx = gitraptor_testkit::Fixture::with_commit(&git());
    fx.write("a.rs", "fn a() {}\n");
    for i in 0..60 {
        fx.write(&format!("cap/f{i:02}.rs"), "fn f() {}\n");
    }
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "files"]);
    fx.git(&["branch", "feat-login"]);
    let wt = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let r = start(fx, wt, Some(TestConfirmation::Eligible), None);
    // claude-1's operation touches the 60 files: they are dirty and its reset throws that away.
    for i in 0..60 {
        std::fs::write(
            r.worktree.join(format!("cap/f{i:02}.rs")),
            "fn f() { agents_work(); }\n",
        )
        .unwrap();
    }
    agent_op(&r, &[], || {});
    for i in 0..60 {
        std::fs::write(
            r.worktree.join(format!("cap/f{i:02}.rs")),
            "fn f() { another_actor(); }\n",
        )
        .unwrap();
    }
    capture_now(&r, None);
    let mut c = r.client();

    let data = confirm_data(undo_through(&mut c, &r.worktree));

    assert_eq!(data.reason, TmRejectReason::Overlap);
    let overlap = overlap_of(&data);
    assert_eq!(
        overlap.conflicts.len(),
        gitraptor_api::timemachine::MAX_OVERLAP_CONFLICTS,
        "the list is capped"
    );
    assert_eq!(overlap.total, 60, "and the total is the real one");
}
