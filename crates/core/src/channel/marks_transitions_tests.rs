//! T006 ⛔6.1 (D7): the transitions of a child are registered with its identity in the same
//! critical section, so they are visible the moment the start barrier drops. Child module of
//! `channel::marks`, attached there with
//! `#[cfg(test)] #[path = "marks_transitions_tests.rs"] mod transitions_tests;`.

use super::*;
use gitraptor_policy::guard::plan::Expected;
use std::sync::mpsc::channel;

fn transitions() -> Arc<[Expected]> {
    Arc::from(vec![Expected::RestoreTo {
        refname: "refs/heads/feat".into(),
        value: "1".repeat(40),
    }])
}

/// A hook that waits for the barrier finds the child with its transitions as soon as it drops.
#[test]
fn transitions_are_visible_once_the_barrier_drops() {
    let marks = Arc::new(ExecutorMarks::default());
    let _guard = marks.open("op-1", &Who::unattributed(), 1_000, None);
    let pending = marks.begin_spawn();
    let (tx, rx) = channel();
    let reader = {
        let marks = Arc::clone(&marks);
        std::thread::spawn(move || {
            // What `guard.evaluate` does: wait for the registration, then look the child up.
            let registered = marks.wait_registered(Duration::from_secs(5));
            tx.send((registered, marks.child_link(50, 1_100))).unwrap();
        })
    };
    // The child is marked with its transitions, then the barrier drops.
    marks.add_child("op-1", 50, 1_100, 50, transitions());
    drop(pending);
    let (registered, link) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the reader answers once the barrier drops");
    reader.join().unwrap();
    assert!(registered);
    let link = link.expect("the child is visible with its identity");
    assert_eq!(link.operation_id, "op-1");
    assert_eq!(link.who, Who::unattributed());
    assert_eq!(&*link.transitions, &*transitions());
    assert!(link.binding.is_none());
}

#[test]
fn a_child_link_needs_the_exact_pid_and_start_and_dies_with_the_operation() {
    let marks = Arc::new(ExecutorMarks::default());
    let guard = marks.open("op-1", &Who::unattributed(), 1_000, None);
    marks.add_child("op-1", 50, 1_100, 50, transitions());
    assert!(marks.child_link(50, 1_100).is_some());
    // A reused pid has another start; an unknown child has no link.
    assert!(marks.child_link(50, 1_101).is_none());
    assert!(marks.child_link(51, 1_100).is_none());
    // `lookup` is unchanged: a grandchild of the same group is marked but is not a child.
    let info = ProcInfo {
        pid: 51,
        ppid: 50,
        uid: 501,
        start_us: 1_200,
        exe: None,
        controlling_terminal: false,
        desktop_session: None,
        session: 51,
        pgid: 50,
    };
    assert!(marks.lookup(&info).is_some());
    assert!(marks.child_link(51, 1_200).is_none());
    drop(guard);
    assert!(marks.child_link(50, 1_100).is_none());
}
