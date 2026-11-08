//! The undo of one agent's work: the set of its operations in a period and what each one
//! changed, subject by subject.

use std::collections::BTreeMap;

use gitraptor_git::Oid;
use gitraptor_git::tm_write::files::Kind;
use gitraptor_git::tm_write::worktree::HeadValue;

use super::oplog::OpRef;

/// What an operation changes: a path of a worktree, a ref, or a worktree's `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subject {
    Path { worktree_key: String, path: String },
    Ref(String),
    Head { worktree_key: String },
}

/// The value of a subject; `None` is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Entry {
        file: Option<(Kind, Oid)>,
        index: Option<(Kind, Oid)>,
    },
    Ref(Option<Oid>),
    Head(HeadValue),
}

/// One operation's change: each subject's value before and after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpChange {
    pub op: OpRef,
    pub before: BTreeMap<Subject, Value>,
    pub after: BTreeMap<Subject, Value>,
}

#[cfg(test)]
#[path = "undo_agent_tests.rs"]
mod tests;
