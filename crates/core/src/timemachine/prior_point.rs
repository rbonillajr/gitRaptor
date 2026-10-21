//! The one rule that picks the point "before a raw Git event", for the timeline and for the
//! undo's target (`docs/dev-briefs/tm-prior-point-binding.md`).
//!
//! A prior (`HookPrior`, `GuaranteedPrior`) is taken just before a command runs, but the engine
//! persists the command's first event later. A point recorded after the prior and whose mark is
//! still before that first event may already hold the command's effect under a stale mark: it
//! is *shadowed* by the prior and is never the point before an event. Events carry no process
//! identity, so the window of a prior runs from its mark to the first raw event of its own
//! worktree key after it.
//!
//! When in doubt the rule picks an older point: restoring too far back is recoverable (the undo
//! takes its own guaranteed prior first), a mid-operation or post-event photo loses the state.
//!
//! TRADEOFF: stale-but-recoverable prior for the first event after a no-op hooked command;
//! upgrade with per-event process identity.
//!
//! TRADEOFF: a foreign git event on the same worktree can close the prior's window before its
//! own command's first event; upgrade with per-event process identity.

use super::oplog::{SnapshotLevel, SnapshotView};

/// What the rule needs to know about a point of one worktree key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PointFacts {
    /// Record order in the oplog.
    pub seq: i64,
    /// The engine mark the point's state reaches.
    pub mark: i64,
    /// A `HookPrior` or a `GuaranteedPrior`.
    pub prior: bool,
    /// The capture saw an operation in progress (`None` rows count as `false`).
    pub mid_operation: bool,
}

impl PointFacts {
    /// The facts of a snapshot, `None` when it has no engine mark (it is never a point before
    /// an event).
    pub(crate) fn of(s: &SnapshotView) -> Option<Self> {
        Some(Self {
            seq: s.record.seq,
            mark: s.record.engine_mark?,
            prior: matches!(
                s.record.level,
                SnapshotLevel::GuaranteedPrior | SnapshotLevel::HookPrior
            ),
            mid_operation: s
                .complete
                .as_ref()
                .and_then(|c| c.mid_operation)
                .unwrap_or(false),
        })
    }
}

/// The indexes of `points` eligible as the point before the event `seq`, best first: the
/// greatest `(mark, seq)` first. `event_seqs` are the raw event seqs of the same key, ascending.
///
/// Eligible: `mark < seq`, `seq >= floor`, not a mid-operation observation (a prior never is
/// excluded for it) and not shadowed by any prior of `points`, available or not. Availability
/// is left to the caller.
pub(crate) fn before_event(
    points: &[PointFacts],
    event_seqs: &[i64],
    seq: i64,
    floor: i64,
) -> Vec<usize> {
    // Each prior with the first event of the key after its mark (`None` is "no event yet").
    let windows: Vec<(i64, Option<i64>)> = points
        .iter()
        .filter(|p| p.prior)
        .map(|p| {
            let at = event_seqs.partition_point(|e| *e <= p.mark);
            (p.seq, event_seqs.get(at).copied())
        })
        .collect();
    let shadowed = |q: &PointFacts| {
        windows.iter().any(|(p_seq, first)| {
            // A prior of an older generation has marks of another numbering: it does not
            // shadow the current one.
            let other_generation = *p_seq < floor && q.seq >= floor;
            q.seq > *p_seq && first.is_none_or(|f| q.mark < f) && !other_generation
        })
    };
    let mut eligible: Vec<usize> = points
        .iter()
        .enumerate()
        .filter(|(_, q)| {
            q.mark < seq && q.seq >= floor && (q.prior || !q.mid_operation) && !shadowed(q)
        })
        .map(|(i, _)| i)
        .collect();
    eligible.sort_by_key(|i| std::cmp::Reverse((points[*i].mark, points[*i].seq)));
    eligible
}

/// Whether a capture is a mid-operation photo: an operation was in progress when it started or
/// when it reached its validity point (D3).
pub(crate) fn mid_operation(at_start: bool, at_end: bool) -> bool {
    at_start || at_end
}

#[cfg(test)]
#[path = "prior_point_tests.rs"]
mod tests;
