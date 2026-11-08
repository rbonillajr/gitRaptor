//! TMC011-BARRIER (T007 of DS-US-TMC-011, D6): the barrier a selective undo leaves in the stack.
//! Child module of `oplog::stack`, mounted there as `mod barrier_tests` (`use super::*`).
//!
//! History used below (the number of each `Do` is its position, 1-based): `a` (someone else),
//! `b` (the agent), `c` (someone else), `d` (the agent), `e` (someone else, after the agent's
//! last operation). The selective undo `u1` takes back `b` and `d`.

use super::*;

fn op(id: &str) -> OpRef {
    OpRef::Oplog(id.into())
}

fn history() -> Vec<StackItem> {
    ["a", "b", "c", "d", "e"]
        .into_iter()
        .map(|id| StackItem::Do(op(id)))
        .collect()
}

fn selective_undo(id: &str, targets: &[&str]) -> StackItem {
    StackItem::Undo {
        undo: id.into(),
        targets: targets.iter().map(|t| op(t)).collect(),
        selective: true,
    }
}

fn plain_undo(id: &str, targets: &[&str]) -> StackItem {
    StackItem::Undo {
        undo: id.into(),
        targets: targets.iter().map(|t| op(t)).collect(),
        selective: false,
    }
}

fn after_selective_undo() -> UndoStack {
    let mut items = history();
    items.push(selective_undo("u1", &["b", "d"]));
    UndoStack::build(items)
}

#[test]
fn a_selective_undo_sets_a_barrier() {
    let stack = after_selective_undo();
    assert_eq!(
        stack.done(),
        &[op("a"), op("c"), op("e")],
        "TMC011-BARRIER: the selective undo took only b and d"
    );
    for below in ["a", "c"] {
        assert!(
            stack.is_below_barrier(&op(below)),
            "TMC011-BARRIER: {below} was done when the selective undo ran: it is below the barrier"
        );
    }
    // A plain undo leaves no barrier.
    let mut items = history();
    items.push(plain_undo("u1", &["e"]));
    let plain = UndoStack::build(items);
    for id in ["a", "b", "c", "d"] {
        assert!(
            !plain.is_below_barrier(&op(id)),
            "TMC011-BARRIER: a plain undo sets no barrier ({id})"
        );
    }
}

#[test]
fn another_actors_operation_after_the_agents_last_is_below_the_barrier() {
    let stack = after_selective_undo();
    assert!(
        stack.is_below_barrier(&op("e")),
        "TMC011-BARRIER: e came after the agent's last target and was done when the undo ran: \
         undoing it would bring back work the selective undo took away (D6)"
    );
}

#[test]
fn an_operation_after_the_selective_undo_is_above_it() {
    let mut items = history();
    items.push(selective_undo("u1", &["b", "d"]));
    items.push(StackItem::Do(op("f")));
    let stack = UndoStack::build(items);
    assert!(
        !stack.is_below_barrier(&op("f")),
        "TMC011-BARRIER: an operation after the selective undo is above the barrier"
    );
    assert!(stack.is_below_barrier(&op("e")));
    // A later plain undo takes f and the barrier stays where it was: a number, not a position.
    let mut items = history();
    items.push(selective_undo("u1", &["b", "d"]));
    items.push(StackItem::Do(op("f")));
    items.push(plain_undo("u2", &["f"]));
    items.push(StackItem::Do(op("g")));
    let stack = UndoStack::build(items);
    assert!(
        stack.is_below_barrier(&op("e")) && stack.is_below_barrier(&op("a")),
        "TMC011-BARRIER: removing operations above does not shift the barrier"
    );
    assert!(
        !stack.is_below_barrier(&op("g")),
        "TMC011-BARRIER: g is above the barrier"
    );
}

#[test]
fn redoing_it_lifts_the_barrier() {
    let mut items = history();
    items.push(selective_undo("u1", &["b", "d"]));
    items.push(StackItem::Redo {
        redo: "r1".into(),
        undo: "u1".into(),
    });
    let stack = UndoStack::build(items);
    for id in ["a", "b", "c", "d", "e"] {
        assert!(
            !stack.is_below_barrier(&op(id)),
            "TMC011-BARRIER: after the redo of the selective undo nothing is below a barrier ({id})"
        );
    }
    // With two active, the highest counts; lifting the later one leaves the earlier.
    let mut items = history();
    items.push(selective_undo("u1", &["b"]));
    items.push(StackItem::Do(op("f")));
    items.push(selective_undo("u2", &["f"]));
    items.push(StackItem::Redo {
        redo: "r2".into(),
        undo: "u2".into(),
    });
    let stack = UndoStack::build(items);
    assert!(
        stack.is_below_barrier(&op("e")),
        "TMC011-BARRIER: the barrier of u1 (up to e) is still active"
    );
    assert!(
        !stack.is_below_barrier(&op("f")),
        "TMC011-BARRIER: f was not done when u1 ran: above its barrier"
    );
}

#[test]
fn redo_of_a_selective_undo_restores_history_order() {
    let mut items = history();
    items.push(selective_undo("u1", &["b", "d"]));
    items.push(StackItem::Redo {
        redo: "r1".into(),
        undo: "u1".into(),
    });
    let stack = UndoStack::build(items);
    assert_eq!(
        stack.done(),
        &[op("a"), op("b"), op("c"), op("d"), op("e")],
        "TMC011-BARRIER: after the redo, done is back in history order, not b and d on top"
    );
    assert_eq!(
        stack.last_operation(),
        Some(&op("e")),
        "TMC011-BARRIER: the next plain undo takes e, the most recent operation"
    );
    // A plain undo keeps its order: its targets were the top.
    let mut items = history();
    items.push(plain_undo("u1", &["d", "e"]));
    items.push(StackItem::Redo {
        redo: "r1".into(),
        undo: "u1".into(),
    });
    assert_eq!(
        UndoStack::build(items).done(),
        &[op("a"), op("b"), op("c"), op("d"), op("e")]
    );
}

#[test]
fn undo_below_the_barrier_is_rejected() {
    let stack = after_selective_undo();
    // The rule `raptor undo` applies to its set: any operation below the barrier rejects it.
    let plain_set = [op("e")];
    assert!(
        plain_set.iter().any(|o| stack.is_below_barrier(o)),
        "TMC011-BARRIER: an undo whose set holds an operation below the barrier is rejected"
    );
    assert_eq!(
        stack.last_operation(),
        Some(&op("e")),
        "the top of the stack is itself below the barrier: the plain undo has nowhere to go"
    );
}
