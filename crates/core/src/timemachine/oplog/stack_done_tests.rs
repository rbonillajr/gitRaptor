//! TMC010-STACK-DONE (T002 of DS-US-TMC-010): `UndoStack::done`.

use super::*;

fn op(id: &str) -> OpRef {
    OpRef::Oplog(id.into())
}

fn dos() -> Vec<StackItem> {
    vec![
        StackItem::Do(op("a")),
        StackItem::Do(op("b")),
        StackItem::Do(op("c")),
    ]
}

#[test]
fn done_lists_what_is_not_undone_oldest_first() {
    let mut items = dos();
    items.push(StackItem::Undo {
        undo: "u1".into(),
        targets: vec![op("c")],
        selective: false,
    });
    let stack = UndoStack::build(items);
    assert_eq!(
        stack.done(),
        &[op("a"), op("b")],
        "TMC010-STACK-DONE: done() is the not-undone operations, oldest first"
    );
    assert_eq!(stack.last_operation(), Some(&op("b")));

    let whole = UndoStack::build(dos());
    assert_eq!(whole.done(), &[op("a"), op("b"), op("c")]);
    assert_eq!(whole.last_operation(), Some(&op("c")));
}

#[test]
fn done_gets_the_undone_operations_back_after_a_redo() {
    let mut items = dos();
    items.push(StackItem::Undo {
        undo: "u1".into(),
        targets: vec![op("c")],
        selective: false,
    });
    items.push(StackItem::Redo {
        redo: "r1".into(),
        undo: "u1".into(),
    });
    let stack = UndoStack::build(items);
    assert_eq!(
        stack.done(),
        &[op("a"), op("b"), op("c")],
        "TMC010-STACK-DONE: a redo of the undo brings c back"
    );
    assert_eq!(stack.last_operation(), Some(&op("c")));
}
