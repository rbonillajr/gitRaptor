//! Whether an undo would overwrite later work of another actor (ADR-TMC-005 § 5).
//!
//! Phase 1 ([`check`]) runs while the undo is planned, over the clean points after the base of
//! the undone change; phase 2 ([`check_tail`]) covers the tail from the last point to the undo's
//! prior, with the repo held. Both are pure over what they receive and fail closed: a point or
//! object that cannot be read in time is `unverified`, never "no overlap".

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use super::apply::PlanWorktree;
use super::oplog::{OpRef, OperationView, Requester, SnapshotView};
use super::store::SnapshotStore;
use super::undo::RawSide;

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;

/// What the overlap check reads.
#[derive(Debug, Clone, Copy)]
pub struct OverlapInput<'a> {
    /// The reference actors (one in the undo): their own later work never overlaps.
    pub reference: &'a [Requester],
    /// What is taken back (one in the undo).
    pub undone: &'a [OpRef],
    /// T: the snapshot the undo returns to.
    pub target: &'a str,
    /// Key and root of each worktree in scope.
    pub worktrees: &'a [PlanWorktree],
    /// Full names of the branches in scope.
    pub refs: &'a BTreeSet<String>,
    pub ops: &'a [OperationView],
    pub snaps: &'a [SnapshotView],
    /// Raw events of each registered worktree, by root.
    pub raw: &'a HashMap<PathBuf, RawSide>,
    pub deadline: Instant,
}

/// What the overlap check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlapReport {
    pub conflicts: Vec<Conflict>,
    /// Worktree key → paths another actor changed that the undone change did not touch.
    pub kept: BTreeMap<String, BTreeSet<String>>,
    pub kept_refs: BTreeSet<String>,
    /// Where the tail starts (phase 2).
    pub last_point: Option<String>,
    /// The check could not be done: the undo is refused as `overlap-unverified`.
    pub unverified: bool,
}

/// A path or branch the undone change touched and another actor touched afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub subject: Subject,
    pub actor: Requester,
    /// Engine time (ms since the epoch) of the point that first showed the later change.
    pub seen_ms: i64,
}

/// What a conflict is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subject {
    Path { worktree_key: String, path: String },
    Ref(String),
}

/// Phase 1: classifies every path and branch the undo would write against the clean points
/// after the base of the undone change.
pub fn check(store: &SnapshotStore, input: &OverlapInput<'_>) -> OverlapReport {
    let _ = (store, input);
    // Fail closed until the classification exists: never "no overlap".
    OverlapReport {
        unverified: true,
        ..OverlapReport::default()
    }
}

/// Phase 2: the same check for the tail (last point of phase 1 → `prior`), with the raw events
/// up to `intent_mark`, merged into `phase1`.
pub fn check_tail(
    store: &SnapshotStore,
    input: &OverlapInput<'_>,
    phase1: &OverlapReport,
    prior: &str,
    intent_mark: i64,
) -> OverlapReport {
    let _ = (store, input, prior, intent_mark);
    OverlapReport {
        unverified: true,
        ..phase1.clone()
    }
}
