//! Local histogram per stage of the Cockpit budget (ADR-CKP-003 § 6): from
//! `t_client_recv` to `t_render`, split into decode, apply and paint, plus
//! the key feedback. In memory only: it never leaves the machine (NFR-03)
//! and the TUI does not write the profile.

use std::collections::VecDeque;
use std::fmt;

/// The Cockpit budget: p95 of `t_client_recv` → `t_render` (ADR-GRP-011 E2).
pub const COCKPIT_P95_NS: u64 = 100_000_000;
/// Key read → frame (ADR-GRP-004 § 3): above it, a warning.
pub const KEY_FEEDBACK_P95_NS: u64 = 100_000_000;

/// Samples kept per stage; older ones are dropped.
const MAX_SAMPLES: usize = 100_000;

/// Durations of one stage, in nanoseconds.
#[derive(Debug, Clone, Default)]
pub struct Samples(VecDeque<u64>);

impl Samples {
    pub fn record(&mut self, ns: u64) {
        if self.0.len() == MAX_SAMPLES {
            self.0.pop_front();
        }
        self.0.push_back(ns);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Nearest-rank percentile `p` (0–100).
    pub fn percentile(&self, p: u64) -> Option<u64> {
        if self.0.is_empty() {
            return None;
        }
        let mut sorted: Vec<u64> = self.0.iter().copied().collect();
        sorted.sort_unstable();
        let rank = (p * sorted.len() as u64).div_ceil(100).max(1) as usize;
        sorted.get(rank - 1).copied()
    }

    pub fn p95(&self) -> Option<u64> {
        self.percentile(95)
    }
}

/// The stages of the Cockpit budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Frame read → decoded (channel thread).
    Decode,
    /// Decoded → applied by `update`, waiting in the queue included.
    Apply,
    /// Applied → `draw` returned.
    Paint,
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Decode => "decode",
            Self::Apply => "apply",
            Self::Paint => "paint",
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub decode: Samples,
    pub apply: Samples,
    pub paint: Samples,
    /// `t_client_recv` → `t_render`.
    pub total: Samples,
    /// Key read → frame.
    pub key: Samples,
    pub frames: u64,
}

impl Metrics {
    /// One engine message, painted at `render_ns`.
    pub fn record(&mut self, recv_ns: u64, decoded_ns: u64, applied_ns: u64, render_ns: u64) {
        self.decode.record(decoded_ns.saturating_sub(recv_ns));
        self.apply.record(applied_ns.saturating_sub(decoded_ns));
        self.paint.record(render_ns.saturating_sub(applied_ns));
        self.total.record(render_ns.saturating_sub(recv_ns));
    }

    /// The stage with the highest p95: what to look at when the total
    /// goes over budget.
    pub fn slowest_stage(&self) -> Option<Stage> {
        [
            (Stage::Decode, self.decode.p95()),
            (Stage::Apply, self.apply.p95()),
            (Stage::Paint, self.paint.p95()),
        ]
        .into_iter()
        .filter_map(|(stage, p95)| p95.map(|v| (stage, v)))
        .max_by_key(|(_, v)| *v)
        .map(|(stage, _)| stage)
    }
}

impl fmt::Display for Metrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |s: &Samples| {
            s.p95()
                .map_or_else(|| "-".to_owned(), |v| format!("{:.2}", v as f64 / 1e6))
        };
        write!(
            f,
            "p95 ms: total {} (decode {}, apply {}, paint {}), key {}; {} messages, {} frames",
            ms(&self.total),
            ms(&self.decode),
            ms(&self.apply),
            ms(&self.paint),
            ms(&self.key),
            self.total.len(),
            self.frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentile() {
        let mut s = Samples::default();
        assert_eq!(s.p95(), None);
        for v in 1..=100 {
            s.record(v);
        }
        assert_eq!(s.p95(), Some(95));
        assert_eq!(s.percentile(100), Some(100));
        assert_eq!(s.percentile(0), Some(1));
    }

    #[test]
    fn the_report_names_the_slowest_stage() {
        let mut m = Metrics::default();
        m.record(0, 1_000, 2_000, 50_000);
        assert_eq!(m.slowest_stage(), Some(Stage::Paint));
        assert_eq!(m.total.p95(), Some(50_000));
    }
}
