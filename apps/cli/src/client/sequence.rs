//! Position of a scope in its stream (ADR-CKP-003 § 4, DEP-CKP-6): a
//! snapshot at `N`, then events from `N + 1`. An event at or below the last
//! applied one is a duplicate; one above `last + 1` is a gap, and nothing
//! more is applied until a new snapshot arrives.

/// Where a scope stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SeqTrack {
    /// No snapshot yet, or one was asked after a gap, a resync or a
    /// reconnection: every event is dropped.
    #[default]
    Waiting,
    /// Following the stream of the daemon run `run_id` after `seq`.
    At { run_id: String, seq: u64 },
}

/// What to do with an event of the scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Apply,
    Duplicate,
    Gap,
    /// Waiting for a snapshot: drop it.
    Drop,
}

impl SeqTrack {
    pub fn is_synced(&self) -> bool {
        matches!(self, Self::At { .. })
    }

    /// Starts again from a snapshot at `seq`.
    pub fn reset(&mut self, run_id: &str, seq: u64) {
        *self = Self::At {
            run_id: run_id.to_owned(),
            seq,
        };
    }

    /// Judges the event at `scope_seq` and, when it applies, advances.
    pub fn accept(&mut self, scope_seq: u64) -> Verdict {
        match self {
            Self::Waiting => Verdict::Drop,
            Self::At { seq, .. } if scope_seq <= *seq => Verdict::Duplicate,
            Self::At { seq, .. } if scope_seq == *seq + 1 => {
                *seq = scope_seq;
                Verdict::Apply
            }
            Self::At { .. } => {
                *self = Self::Waiting;
                Verdict::Gap
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_are_dropped_and_a_gap_stops_everything() {
        let mut track = SeqTrack::default();
        assert_eq!(track.accept(1), Verdict::Drop);
        track.reset("run", 4);
        assert_eq!(track.accept(4), Verdict::Duplicate);
        assert_eq!(track.accept(5), Verdict::Apply);
        assert_eq!(track.accept(5), Verdict::Duplicate);
        assert_eq!(track.accept(7), Verdict::Gap);
        // Nothing after a gap applies, not even the missing one.
        assert_eq!(track.accept(6), Verdict::Drop);
        assert_eq!(track.accept(8), Verdict::Drop);
        track.reset("run", 8);
        assert_eq!(track.accept(9), Verdict::Apply);
    }

    /// Property over arbitrary sequences: applied events are exactly the
    /// contiguous run after the snapshot, in order, each once.
    #[test]
    fn applied_events_are_contiguous_and_unique() {
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..2_000 {
            let mut track = SeqTrack::default();
            let start = state % 50;
            track.reset("run", start);
            let mut applied = Vec::new();
            for _ in 0..40 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let seq = start + state % 12;
                if track.accept(seq) == Verdict::Apply {
                    applied.push(seq);
                }
            }
            let expected: Vec<u64> = (start + 1..start + 1 + applied.len() as u64).collect();
            assert_eq!(applied, expected);
        }
    }
}
