//! "Last operation" and the undo/redo stack of one scope (ADR-TMC-003 § 4,
//! TQ-9 → a).
//!
//! An undo takes back the most recent operation of the scope that is not
//! undone yet; consecutive undos go back one more each time. A redo redoes
//! the last undo while nothing new happened in the scope: any new operation
//! clears the redo. Each worktree has its own stack, and operations that
//! only touch refs share the repo's refs stack.

use std::collections::{HashMap, HashSet};

use super::Oplog;
use super::model::{OpRef, OperationKind, OperationState, Target};
use crate::profile::Result;

/// The subtype the oplog records for an undo by agent: a selective undo.
pub const SELECTIVE_UNDO_SUBTYPE: &str = "agent";

/// Whose stack: one worktree, or the repo's refs for operations that touch
/// no worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackScope {
    Worktree(String),
    Refs,
}

/// A raw Git event of the engine that changes the repo state in this scope
/// (commit, checkout, reset, merge, rebase...). Referenced by sequence,
/// never copied. Events caused by an oplog operation are skipped, so they
/// do not count twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEvent {
    /// Engine sequence (ADR-GRP-013).
    pub seq: i64,
    /// Oplog operation whose execution produced the event, if any.
    pub caused_by: Option<String>,
}

/// One entry of the scope's history, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackItem {
    /// Something that changed the scope and can be undone.
    Do(OpRef),
    /// An undo of `targets`; `selective` when it took back a chosen set (an agent's work)
    /// instead of the top of the stack.
    Undo {
        undo: String,
        targets: Vec<OpRef>,
        selective: bool,
    },
    /// A redo of the undo `undo`.
    Redo { redo: String, undo: String },
}

/// The stack after replaying a scope's history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoStack {
    done: Vec<OpRef>,
    redo: Vec<(String, Vec<OpRef>)>,
    /// History number of each operation done, in the order the history did them (from 1).
    number: HashMap<OpRef, u64>,
    /// Active barriers: the selective undo and the highest history number done when it ran.
    barriers: Vec<(String, u64)>,
}

impl UndoStack {
    /// Replays `items`. A selective undo leaves a barrier at the highest history number still
    /// done when it ran (every operation done then is below it, also another actor's after its
    /// last target); its redo lifts it. A number, not a position in `done`: later undos do not
    /// shift it. After a redo `done` is back in history order, so the next plain undo takes the
    /// most recent operation and never one under a redone target.
    pub fn build(items: impl IntoIterator<Item = StackItem>) -> Self {
        let mut stack = Self::default();
        let mut next = 0u64;
        for item in items {
            match item {
                StackItem::Do(op) => {
                    next += 1;
                    stack.number.entry(op.clone()).or_insert(next);
                    stack.done.push(op);
                    stack.redo.clear();
                }
                StackItem::Undo {
                    undo,
                    targets,
                    selective,
                } => {
                    stack.done.retain(|op| !targets.contains(op));
                    if selective {
                        let highest = stack
                            .done
                            .iter()
                            .filter_map(|op| stack.number.get(op))
                            .max()
                            .copied();
                        if let Some(highest) = highest {
                            stack.barriers.push((undo.clone(), highest));
                        }
                    }
                    stack.redo.push((undo, targets));
                }
                StackItem::Redo { undo, .. } => {
                    if let Some(pos) = stack.redo.iter().rposition(|(u, _)| *u == undo) {
                        let (_, targets) = stack.redo.remove(pos);
                        stack.done.extend(targets);
                        let number = &stack.number;
                        stack
                            .done
                            .sort_by_key(|op| number.get(op).copied().unwrap_or(u64::MAX));
                        stack.barriers.retain(|(u, _)| *u != undo);
                    }
                }
            }
        }
        stack
    }

    /// The most recent operation of the scope that is not undone: what the
    /// next undo takes back.
    pub fn last_operation(&self) -> Option<&OpRef> {
        self.done.last()
    }

    /// Whether `op` was done when a selective undo still in force ran: undoing it would bring
    /// back work that undo took away.
    pub fn is_below_barrier(&self, op: &OpRef) -> bool {
        let Some(highest) = self.barriers.iter().map(|(_, n)| *n).max() else {
            return false;
        };
        self.number.get(op).is_some_and(|n| *n <= highest)
    }

    /// The operations of the scope that are not undone, oldest first.
    pub fn done(&self) -> &[OpRef] {
        &self.done
    }

    /// The undo the next redo redoes, if any.
    pub fn next_redo(&self) -> Option<&str> {
        self.redo.last().map(|(undo, _)| undo.as_str())
    }
}

impl Oplog {
    /// The stack of `scope`, merging the oplog's operations with the raw
    /// Git events the engine supplies. Operations are placed among events by
    /// their engine mark, not by clock. Finished operations count; an
    /// interrupted one, of any kind, counts as something done, so the next
    /// undo returns to its prior snapshot (US-TMC-019).
    pub fn undo_stack(&self, scope: &StackScope, external: &[ExternalEvent]) -> Result<UndoStack> {
        self.undo_stack_in(scope, external, 0)
    }

    /// Like [`Oplog::undo_stack`], with the rows before oplog sequence
    /// `floor` from another generation of the engine store (or from before
    /// the common mark, US-TMC-004): their marks do not compare with the
    /// events', so they go before every event, in their own order.
    pub fn undo_stack_in(
        &self,
        scope: &StackScope,
        external: &[ExternalEvent],
        floor: i64,
    ) -> Result<UndoStack> {
        let mut keyed: Vec<((i64, u8, i64), StackItem)> = Vec::new();
        for event in external.iter().filter(|e| e.caused_by.is_none()) {
            keyed.push(((event.seq, 0, 0), StackItem::Do(OpRef::GitEvent(event.seq))));
        }
        let ops = self.operations(&Default::default())?;
        let own: HashSet<String> = ops
            .iter()
            .map(|op| op.record.operation_id.clone())
            .collect();
        for op in ops {
            let in_scope = match scope {
                StackScope::Worktree(w) => op.record.scope.worktrees.contains(w),
                StackScope::Refs => op.record.scope.worktrees.is_empty(),
            };
            if !in_scope || op.tampered {
                continue;
            }
            let id = op.record.operation_id.clone();
            let item = match (op.state, op.record.kind, &op.record.target) {
                (OperationState::Interrupted, _, _) => StackItem::Do(OpRef::Oplog(id)),
                (OperationState::Finished, OperationKind::Undo, Target::Undo(targets)) => {
                    StackItem::Undo {
                        undo: id,
                        targets: targets.clone(),
                        // An undo by agent took back a chosen set, not the top of the stack.
                        selective: op.record.subtype.as_deref() == Some(SELECTIVE_UNDO_SUBTYPE),
                    }
                }
                (OperationState::Finished, OperationKind::Redo, Target::Redo(undo))
                    if own.contains(undo) =>
                {
                    StackItem::Redo {
                        redo: id,
                        undo: undo.clone(),
                    }
                }
                (
                    OperationState::Finished,
                    OperationKind::Protected | OperationKind::Restore,
                    _,
                ) => StackItem::Do(OpRef::Oplog(id)),
                _ => continue,
            };
            let mark = if op.record.seq < floor {
                i64::MIN
            } else {
                op.record.engine_mark
            };
            keyed.push(((mark, 1, op.record.seq), item));
        }
        keyed.sort_by_key(|(key, _)| *key);
        Ok(UndoStack::build(keyed.into_iter().map(|(_, item)| item)))
    }
}

#[cfg(test)]
#[path = "stack_done_tests.rs"]
mod done_tests;

#[cfg(test)]
#[path = "stack_barrier_tests.rs"]
mod barrier_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str) -> OpRef {
        OpRef::Oplog(id.into())
    }

    #[test]
    fn undo_undo_redo_walks_back_and_forth() {
        let stack = UndoStack::build([
            StackItem::Do(op("a")),
            StackItem::Do(op("b")),
            StackItem::Undo {
                undo: "u1".into(),
                targets: vec![op("b")],
                selective: false,
            },
            StackItem::Undo {
                undo: "u2".into(),
                targets: vec![op("a")],
                selective: false,
            },
        ]);
        assert_eq!(stack.last_operation(), None);
        assert_eq!(stack.next_redo(), Some("u2"));
        let stack = UndoStack::build([
            StackItem::Do(op("a")),
            StackItem::Do(op("b")),
            StackItem::Undo {
                undo: "u1".into(),
                targets: vec![op("b")],
                selective: false,
            },
            StackItem::Undo {
                undo: "u2".into(),
                targets: vec![op("a")],
                selective: false,
            },
            StackItem::Redo {
                redo: "r1".into(),
                undo: "u2".into(),
            },
        ]);
        assert_eq!(stack.last_operation(), Some(&op("a")));
        assert_eq!(stack.next_redo(), Some("u1"));
    }

    #[test]
    fn a_new_operation_clears_the_redo() {
        let stack = UndoStack::build([
            StackItem::Do(op("a")),
            StackItem::Undo {
                undo: "u1".into(),
                targets: vec![op("a")],
                selective: false,
            },
            StackItem::Do(OpRef::GitEvent(7)),
        ]);
        assert_eq!(stack.next_redo(), None);
        assert_eq!(stack.last_operation(), Some(&OpRef::GitEvent(7)));
    }
}
