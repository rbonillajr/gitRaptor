//! TMC003-STACK (T002 of DS-US-TMC-003): which operation emptied a pending redo.
//! Child module of `oplog::stack`, mounted there as `mod redo_tests` (`use super::*`).
//!
//! Needs: `UndoStack::redo_invalidated() -> Option<&OpRef>` (not written yet).

use super::*;

fn op(id: &str) -> OpRef {
    OpRef::Oplog(id.into())
}

fn done(id: &str) -> StackItem {
    StackItem::Do(op(id))
}

fn undo(id: &str, targets: &[&str]) -> StackItem {
    StackItem::Undo {
        undo: id.into(),
        targets: targets.iter().map(|t| op(t)).collect(),
        selective: false,
    }
}

fn redo(id: &str, of: &str) -> StackItem {
    StackItem::Redo {
        redo: id.into(),
        undo: of.into(),
    }
}

#[test]
fn a_new_operation_says_it_invalidated_the_redo() {
    let stack = UndoStack::build([done("a"), done("b"), undo("u1", &["b"]), done("c")]);
    assert_eq!(
        stack.redo_invalidated(),
        Some(&op("c")),
        "TMC003-STACK: the operation that emptied the pending redo is the one reported"
    );
    assert_eq!(
        stack.next_redo(),
        None,
        "TMC003-STACK: next_redo is unchanged: nothing to redo"
    );
    assert_eq!(stack.last_operation(), Some(&op("c")));
    // A raw Git event invalidates too.
    let stack = UndoStack::build([
        done("a"),
        undo("u1", &["a"]),
        StackItem::Do(OpRef::GitEvent(9)),
    ]);
    assert_eq!(stack.redo_invalidated(), Some(&OpRef::GitEvent(9)));
}

#[test]
fn an_undo_after_the_invalidation_reopens_the_redo() {
    let stack = UndoStack::build([
        done("a"),
        done("b"),
        undo("u1", &["b"]),
        done("c"),
        undo("u2", &["c"]),
    ]);
    assert_eq!(
        stack.redo_invalidated(),
        None,
        "TMC003-STACK: a later undo reopens the redo, so nothing is invalidated"
    );
    assert_eq!(stack.next_redo(), Some("u2"));
}

#[test]
fn a_stack_that_never_had_a_redo_has_nothing_invalidated() {
    let stack = UndoStack::build([done("a"), done("b"), done("c")]);
    assert_eq!(
        stack.redo_invalidated(),
        None,
        "TMC003-STACK: no undo ever happened: nothing invalidated"
    );
    assert_eq!(UndoStack::build([]).redo_invalidated(), None);
}

#[test]
fn a_redone_undo_is_not_an_invalidation() {
    let stack = UndoStack::build([done("a"), undo("u1", &["a"]), redo("r1", "u1")]);
    assert_eq!(
        stack.redo_invalidated(),
        None,
        "TMC003-STACK: a redo does not invalidate"
    );
    assert_eq!(stack.next_redo(), None);
    assert_eq!(stack.last_operation(), Some(&op("a")));
}

#[test]
fn an_interrupted_redo_that_emptied_nothing_invalidates_nothing() {
    // An interrupted redo counts as an operation done (US-TMC-019), but with the redo list
    // already empty there was nothing for it to empty.
    let stack = UndoStack::build([
        done("a"),
        undo("u1", &["a"]),
        redo("r1", "u1"),
        done("r2-interrupted"),
    ]);
    assert_eq!(
        stack.redo_invalidated(),
        None,
        "TMC003-STACK: an interrupted redo that emptied nothing reports no invalidation"
    );
    assert_eq!(
        stack.last_operation(),
        Some(&op("r2-interrupted")),
        "TMC003-STACK: the next undo takes back the interrupted redo"
    );
}
