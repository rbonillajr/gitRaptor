//! T008 (D5, D6, H-01, ⛔8.1): who the `git` of a hook belongs to when the daemon runs it for the
//! executor. `ProcSource` doubles, no real processes, no `Command` (the boundary of
//! `guard_boundary.rs`). Child module of `guardrails::link`, attached there with
//! `#[cfg(test)] #[path = "link_tests.rs"] mod tests;`.

use super::*;
use crate::channel::AgentMatcher;
use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::{ProcError, ProcInfo, ProcSource};
use crate::channel::requester::Who;
use gitraptor_api::catalog::Layer;
use gitraptor_api::guard::{
    Decision, Effect, ExceptionState, Level, Operation, Reason, RefUpdate, RefValue, Rule,
};
use gitraptor_policy::guard::plan::{Expected, MatchFacts};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const UID: u32 = 501;
const DAEMON: (u32, u64) = (70, 700);
const FEAT: &str = "refs/heads/feat";

#[derive(Default)]
struct Tree {
    procs: HashMap<u32, ProcInfo>,
}

impl Tree {
    fn add(&mut self, pid: u32, ppid: u32, exe: &str, start: u64) {
        self.procs.insert(
            pid,
            ProcInfo {
                pid,
                ppid,
                uid: UID,
                start_us: start,
                exe: Some(PathBuf::from(exe)),
                controlling_terminal: false,
                session: pid,
                pgid: pid,
                desktop_session: None,
            },
        );
    }
}

impl ProcSource for Tree {
    fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
        self.procs.get(&pid).cloned().ok_or(ProcError::Gone)
    }
    fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
        None
    }
}

/// daemon(70) → git(100, started by the executor) → sh(110, the user's hook) → client(120).
fn tree() -> Tree {
    let mut t = Tree::default();
    t.add(70, 1, "/usr/local/bin/raptor", 700);
    t.add(100, 70, "/usr/bin/git", 1_100);
    t.add(110, 100, "/bin/sh", 1_200);
    t.add(120, 110, "/usr/local/bin/raptor-hook", 1_300);
    t
}

fn peer(pid: u32, start: u64) -> AcceptedPeer {
    AcceptedPeer {
        pid,
        start_us: start,
        accepted_us: 1_000_000,
    }
}

fn who() -> Who {
    Who::unattributed()
}

fn oid(c: char) -> String {
    c.to_string().repeat(40)
}

fn transitions() -> Arc<[Expected]> {
    Arc::from(vec![Expected::Exact {
        refname: FEAT.into(),
        old: RefValue::Zero,
        new: RefValue::Oid(oid('a')),
    }])
}

/// The executor opened an operation and marked `git`(100) as its child.
fn marks() -> Arc<ExecutorMarks> {
    let marks = Arc::new(ExecutorMarks::default());
    // The guard lives as long as the marks: leaked on purpose, the test ends with the process.
    std::mem::forget(marks.open("op-1", &who(), 1_000, None));
    marks.add_child("op-1", 100, 1_100, 100, transitions());
    marks
}

fn classify_in(t: &Tree, marks: &ExecutorMarks, daemon: (u32, u64), pid: u32, start: u64) -> Link {
    let matcher = AgentMatcher::default();
    let checks = Checks {
        uid: UID,
        procs: t,
        matcher: &matcher,
        daemon: Some(daemon),
        marks: Some(marks),
        terminal_proof: true,
        orphans_marked: true,
    };
    classify(peer(pid, start), &checks, marks)
}

fn decision(rule: Rule) -> Decision {
    Decision {
        decision_id: "plan-decision".into(),
        effect: Effect::Allow,
        applied_effect: Effect::Allow,
        reasons: vec![Reason {
            rule,
            level: Level::Minimum,
            cause: None,
            params: vec![],
        }],
        exception: ExceptionState::None,
        config_status: vec![],
        config_ref: vec![],
        notices: vec![],
        prior_snapshot: None,
    }
}

fn child_with(binding: Option<Arc<PlanBinding>>, registered: Arc<[Expected]>) -> ChildLink {
    ChildLink {
        operation_id: "op-1".into(),
        who: who(),
        binding,
        transitions: registered,
    }
}

// ----- classify (D5) -------------------------------------------------------------------------

#[test]
fn the_git_the_executor_started_is_the_direct_child() {
    let t = tree();
    let link = classify_in(&t, &marks(), DAEMON, 120, 1_300);
    match link {
        Link::Child(child) => {
            assert_eq!(child.operation_id, "op-1");
            assert_eq!(&*child.transitions, &*transitions());
        }
        other => panic!("expected the direct child, got {other:?}"),
    }
}

/// H-01: a hook that does `exec git` leaves a `git` that is not the registered child.
#[test]
fn an_exec_git_from_a_hook_is_a_grandchild() {
    let mut t = tree();
    // The hook's shell became `git` (exec): the nearest `git` of the client is that one.
    t.add(110, 100, "/usr/bin/git", 1_200);
    let link = classify_in(&t, &marks(), DAEMON, 120, 1_300);
    assert!(
        matches!(link, Link::Grandchild { who: ref w } if *w == who()),
        "{link:?}"
    );
}

#[test]
fn a_git_under_a_registered_child_that_is_not_it_is_a_grandchild() {
    let mut t = tree();
    // The hook (sh 110) runs another `git` (130), whose own hook is the client (140).
    t.add(130, 110, "/usr/bin/git", 1_250);
    t.add(140, 130, "/usr/local/bin/raptor-hook", 1_400);
    let link = classify_in(&t, &marks(), DAEMON, 140, 1_400);
    assert!(
        matches!(link, Link::Grandchild { who: ref w } if *w == who()),
        "{link:?}"
    );
}

/// ⛔8.1: the parent of the child must be the daemon with its start time; a pid of the daemon
/// that was reused by another process is not a parent.
#[test]
fn a_reused_pid_of_the_daemon_is_not_a_parent() {
    let mut t = tree();
    // Pid 70 is now another process, started later than the daemon the checks name.
    t.add(70, 1, "/usr/bin/other", 5_000);
    let link = classify_in(&t, &marks(), DAEMON, 120, 1_300);
    assert!(!matches!(link, Link::Child(_)), "{link:?}");
}

#[test]
fn a_hook_with_no_marked_ancestor_is_unrelated() {
    let mut t = Tree::default();
    t.add(70, 1, "/usr/local/bin/raptor", 700);
    t.add(200, 1, "/usr/bin/git", 2_000);
    t.add(210, 200, "/usr/local/bin/raptor-hook", 2_100);
    let link = classify_in(&t, &marks(), DAEMON, 210, 2_100);
    assert!(matches!(link, Link::None), "{link:?}");
}

// ----- the decision of a direct child (D6) -------------------------------------------------

fn binding(registered: Arc<[Expected]>) -> Arc<PlanBinding> {
    Arc::new(PlanBinding {
        plan_id: "plan-1".into(),
        decision: decision(Rule::MinimumForcePush),
        layer: Layer::Cockpit,
        who: who(),
        registered,
        late: Mutex::new(None),
    })
}

fn tx(refname: &str, old: RefValue, new: RefValue) -> Operation {
    Operation::RefTransaction {
        updates: vec![RefUpdate {
            refname: refname.into(),
            old,
            new,
        }],
        orphan_head: None,
    }
}

/// The abort of the atomic rebase moves the branch back (`RestoreTo`): it matches the child's own
/// transitions and reuses the plan's decision without calling `decide`.
#[test]
fn the_abort_child_reuses_the_plan_decision() {
    let registered: Arc<[Expected]> = Arc::from(vec![Expected::RestoreTo {
        refname: FEAT.into(),
        value: oid('b'),
    }]);
    let plan = binding(Arc::clone(&registered));
    let child = child_with(Some(Arc::clone(&plan)), registered);
    let mut calls = 0;
    let (served, inherited) = decide_for_child(
        &child,
        &tx(FEAT, RefValue::Oid(oid('c')), RefValue::Oid(oid('b'))),
        None,
        &MatchFacts::default(),
        &mut |_op| {
            calls += 1;
            decision(Rule::InputRejected)
        },
    );
    assert!(inherited);
    assert_eq!(calls, 0, "decide must not be called");
    assert_eq!(served, plan.decision);
    assert!(plan.late.lock().unwrap().is_none());
}

/// D6: a transition that is not the registered one is evaluated again and accumulated in `late`.
#[test]
fn another_transition_is_evaluated_again_and_kept_in_late() {
    let registered: Arc<[Expected]> = Arc::from(vec![Expected::RestoreTo {
        refname: FEAT.into(),
        value: oid('b'),
    }]);
    let plan = binding(Arc::clone(&registered));
    let child = child_with(Some(Arc::clone(&plan)), registered);
    let mut calls = 0;
    let (served, inherited) = decide_for_child(
        &child,
        &tx(FEAT, RefValue::Oid(oid('c')), RefValue::Oid(oid('d'))),
        None,
        &MatchFacts::default(),
        &mut |_op| {
            calls += 1;
            decision(Rule::InputRejected)
        },
    );
    assert!(!inherited);
    assert_eq!(calls, 1);
    assert_eq!(served.reasons[0].rule, Rule::InputRejected);
    let late = plan.late.lock().unwrap();
    let kept = late.as_ref().expect("the new evaluation is kept in `late`");
    assert_eq!(kept.reasons[0].rule, Rule::InputRejected);
}
