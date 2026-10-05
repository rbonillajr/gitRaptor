//! The CPU window of RES-01: mean and peak over the last 10 minutes.
//!
//! Pure: it only sees `(monotonic ns, CPU ns)` samples, so it is tested
//! with synthetic ones and no real clock.

use std::collections::VecDeque;

/// One point: wall time on the monotonic clock and the process CPU time,
/// both in ns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuPoint {
    pub at_ns: u64,
    pub cpu_ns: u64,
}

/// Mean and peak of a window, in % of one core.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuStats {
    pub mean_pct: Option<f64>,
    pub peak_pct: Option<f64>,
    /// Seconds the mean covers.
    pub window_s: u64,
}

/// The samples of the last `window_ns`.
#[derive(Debug, Clone)]
pub struct CpuWindow {
    window_ns: u64,
    points: VecDeque<CpuPoint>,
}

fn pct(from: CpuPoint, to: CpuPoint) -> Option<f64> {
    let wall = to.at_ns.checked_sub(from.at_ns).filter(|w| *w > 0)?;
    let cpu = to.cpu_ns.saturating_sub(from.cpu_ns);
    Some(cpu as f64 * 100.0 / wall as f64)
}

impl CpuWindow {
    pub fn new(window_ns: u64) -> Self {
        Self {
            window_ns,
            points: VecDeque::new(),
        }
    }

    /// Adds a sample and drops those older than the window.
    pub fn push(&mut self, point: CpuPoint) {
        if self
            .points
            .back()
            .is_some_and(|last| point.at_ns < last.at_ns)
        {
            return;
        }
        self.points.push_back(point);
        let start = point.at_ns.saturating_sub(self.window_ns);
        while self.points.front().is_some_and(|p| p.at_ns < start) {
            self.points.pop_front();
        }
    }

    /// Mean and peak up to `now`, a live sample that is not kept: the
    /// interval in progress counts too.
    pub fn stats(&self, now: CpuPoint) -> CpuStats {
        // The mean starts at the oldest sample inside the window, so it never
        // covers more than the window.
        let start = now.at_ns.saturating_sub(self.window_ns);
        let mut points: Vec<CpuPoint> = self
            .points
            .iter()
            .copied()
            .filter(|p| p.at_ns >= start && p.at_ns <= now.at_ns)
            .collect();
        points.push(now);
        let first = points[0];
        let peak = points
            .windows(2)
            .filter_map(|w| pct(w[0], w[1]))
            .fold(None, |max: Option<f64>, p| {
                Some(max.map_or(p, |m| m.max(p)))
            });
        CpuStats {
            mean_pct: pct(first, now),
            peak_pct: peak,
            window_s: now.at_ns.saturating_sub(first.at_ns) / 1_000_000_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    fn at(s: u64, cpu_ms: u64) -> CpuPoint {
        CpuPoint {
            at_ns: s * S,
            cpu_ns: cpu_ms * 1_000_000,
        }
    }

    #[test]
    fn mean_is_cpu_time_over_wall_time() {
        let mut w = CpuWindow::new(600 * S);
        w.push(at(0, 0));
        w.push(at(10, 100)); // 1 % in the first 10 s
        w.push(at(20, 100)); // 0 % in the next 10 s
        let stats = w.stats(at(30, 400)); // 3 % in the interval in progress
        assert_eq!(stats.window_s, 30);
        assert!((stats.mean_pct.unwrap() - 400.0 / 30_000.0 * 100.0).abs() < 1e-9);
        assert!((stats.peak_pct.unwrap() - 3.0).abs() < 1e-9);
    }

    /// A daemon younger than one interval still has a mean.
    #[test]
    fn a_young_daemon_has_a_partial_window() {
        let mut w = CpuWindow::new(600 * S);
        w.push(at(0, 0));
        let stats = w.stats(at(2, 10));
        assert_eq!(stats.window_s, 2);
        assert!((stats.mean_pct.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(stats.peak_pct, stats.mean_pct);
    }

    #[test]
    fn nothing_to_compare_is_not_a_zero() {
        let w = CpuWindow::new(600 * S);
        let stats = w.stats(at(5, 10));
        assert_eq!(stats.mean_pct, None);
        assert_eq!(stats.peak_pct, None);
        assert_eq!(stats.window_s, 0);
    }

    /// Only the last 10 minutes count: a busy start falls out of the window.
    #[test]
    fn the_window_is_cut_at_ten_minutes() {
        let mut w = CpuWindow::new(600 * S);
        w.push(at(0, 0));
        w.push(at(10, 5_000)); // 50 % in the first interval
        let mut cpu = 5_000;
        for s in (20..=700).step_by(10) {
            cpu += 10; // 0.1 %
            w.push(at(s, cpu));
        }
        let stats = w.stats(at(710, cpu + 10));
        assert_eq!(stats.window_s, 600);
        assert!((stats.mean_pct.unwrap() - 0.1).abs() < 1e-9, "{stats:?}");
        assert!((stats.peak_pct.unwrap() - 0.1).abs() < 1e-9, "{stats:?}");
        assert!(w.points.len() <= 62);
    }

    #[test]
    fn a_sample_from_the_past_is_ignored() {
        let mut w = CpuWindow::new(600 * S);
        w.push(at(10, 100));
        w.push(at(5, 0));
        assert_eq!(w.points.len(), 1);
    }
}
