//! Statistics and gates of the freshness and scale bench (INF-GRP-002, ADR-GRP-011 § 4).
//!
//! Pure functions, so the gates are tested without a daemon: a delay injected into one stage of
//! synthetic samples must fail the gate when it pushes the engine total over 300 ms, and only
//! warn, naming the stage, when the total stays within budget.
//!
//! Stage names are the canonical marks of ADR-GRP-011 § 2 (`t0`, `t_recv`, `t_flush`,
//! `t_computed`, `t_persisted`, `t_published`, `t_client_recv`, `t_render`).

use std::fmt;

/// Percentiles of one series, in milliseconds (nearest rank).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl Summary {
    /// `None` when there are no samples.
    pub fn of(samples: &[f64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut s = samples.to_vec();
        s.sort_by(f64::total_cmp);
        let rank = |p: f64| {
            let k = ((p / 100.0) * s.len() as f64).ceil() as usize;
            s[k.clamp(1, s.len()) - 1]
        };
        Some(Self {
            n: s.len(),
            p50: rank(50.0),
            p95: rank(95.0),
            p99: rank(99.0),
            max: s[s.len() - 1],
        })
    }

    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "n": self.n, "p50": round(self.p50), "p95": round(self.p95),
            "p99": round(self.p99), "max": round(self.max),
        })
    }
}

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Engine part of NFR-04 (ADR-GRP-011 § 2): above it, on the p95, the CI fails.
pub const ENGINE_BUDGET_MS: f64 = 300.0;

/// Where the bench runs: the provisional ceilings are per platform (Decisión del orquestador
/// 2026-10-05, validada por Arquitecto y PO), each the maximum measured there × 1.25, so a
/// faster machine does not inherit the slack of a slower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// A developer's Mac (the reference machine of ADR-GRP-011 § 4).
    MacLocal,
    /// The hosted `macos-latest` runner.
    MacCi,
    /// The hosted `ubuntu-latest` runner, or a Linux machine.
    Linux,
    Other,
}

impl Platform {
    pub fn detect(ci: bool) -> Self {
        match (cfg!(target_os = "macos"), cfg!(target_os = "linux"), ci) {
            (true, _, false) => Platform::MacLocal,
            (true, _, true) => Platform::MacCi,
            (_, true, _) => Platform::Linux,
            _ => Platform::Other,
        }
    }

    /// Provisional ceiling of the engine p95 in a burst scenario, which macOS does not meet yet
    /// (TD-GRP-001): the maximum measured there × 1.25, per scenario. On the runner it applies to
    /// the total already widened by the excess timer slack. `None`: the 300 ms budget is the gate
    /// (Linux meets it).
    pub fn burst_ceiling_ms(self, scenario: &str) -> Option<f64> {
        match (self, scenario) {
            (Platform::MacLocal, BURST_1K) => Some(420.0),
            (Platform::MacLocal, BURST_10K) => Some(620.0),
            (Platform::MacCi, BURST_1K) => Some(615.0),
            (Platform::MacCi, BURST_10K) => Some(585.0),
            _ => None,
        }
    }
}

/// Scale scenario of ADR-GRP-011 § 4: a burst of 1,000 files.
pub const BURST_1K: &str = "burst-1k";
/// Stress scenario: a burst of 10,000 files.
pub const BURST_10K: &str = "burst-10k";

/// Above this excess timer slack the machine cannot measure a 300 ms budget: its latency gates
/// become warnings and the run says so (Arquitecto, 2026-10-05; the way out is a dedicated
/// runner, ADR-GRP-011 § 4).
pub const MAX_SLACK_EXCESS_MS: f64 = 100.0;

/// What the debounce window may exceed its effective 75 ms by before the bench warns: the
/// jitter left once the timer slack is discounted (ADR-GRP-010 § 3). Decision of INF-GRP-002.
pub const DEBOUNCE_TOLERANCE_MS: f64 = 5.0;

/// A stage of the engine, with its marks and its p95 budget (ADR-GRP-011 § 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// `t0` → `t_recv`. Only isolated when `t0` is a plain write ("modify a file").
    Detection,
    /// `t_recv` → `t_flush`, the effective window.
    Debounce,
    /// `t_flush` → `t_computed`. Reported; budgeted together with persistence.
    Compute,
    /// `t_computed` → `t_persisted`. Reported; budgeted together with the compute.
    Persist,
    /// `t_flush` → `t_persisted`: the 150 ms row of the table.
    Recompute,
    /// `t_persisted` → `t_client_recv`.
    Publish,
    /// `t0` → `t_client_recv`.
    Total,
}

impl Stage {
    pub const ALL: [Stage; 7] = [
        Stage::Detection,
        Stage::Debounce,
        Stage::Compute,
        Stage::Persist,
        Stage::Recompute,
        Stage::Publish,
        Stage::Total,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Detection => "detection",
            Stage::Debounce => "debounce",
            Stage::Compute => "compute",
            Stage::Persist => "persist",
            Stage::Recompute => "recompute",
            Stage::Publish => "publish",
            Stage::Total => "total",
        }
    }

    /// Marks it goes from and to, with the canonical names.
    pub fn marks(self) -> (&'static str, &'static str) {
        match self {
            Stage::Detection => ("t0", "t_recv"),
            Stage::Debounce => ("t_recv", "t_flush"),
            Stage::Compute => ("t_flush", "t_computed"),
            Stage::Persist => ("t_computed", "t_persisted"),
            Stage::Recompute => ("t_flush", "t_persisted"),
            Stage::Publish => ("t_persisted", "t_client_recv"),
            Stage::Total => ("t0", "t_client_recv"),
        }
    }

    /// p95 budget, or `None` for the stages that are only reported.
    pub fn budget_ms(self) -> Option<f64> {
        match self {
            Stage::Detection => Some(50.0),
            Stage::Debounce => Some(75.0 + DEBOUNCE_TOLERANCE_MS),
            Stage::Compute | Stage::Persist => None,
            Stage::Recompute => Some(150.0),
            Stage::Publish => Some(25.0),
            Stage::Total => Some(ENGINE_BUDGET_MS),
        }
    }
}

/// The marks of one sample, in nanoseconds of the common monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sample {
    pub t0: u64,
    pub t_recv: u64,
    pub t_flush: u64,
    pub t_computed: u64,
    pub t_persisted: u64,
    pub t_published: u64,
    pub t_client_recv: u64,
}

impl Sample {
    /// Milliseconds of `stage`, or `None` when it cannot be isolated: detection when the batch
    /// opened before `t0` (a Git command writes before it ends, ADR-GRP-011 § 4). The total is
    /// zero when the event arrived before `t0`.
    pub fn stage_ms(&self, stage: Stage) -> Option<f64> {
        let ms = |a: u64, b: u64| b.checked_sub(a).map(|d| d as f64 / 1e6);
        match stage {
            Stage::Detection => ms(self.t0, self.t_recv),
            Stage::Debounce => ms(self.t_recv, self.t_flush),
            Stage::Compute => ms(self.t_flush, self.t_computed),
            Stage::Persist => ms(self.t_computed, self.t_persisted),
            Stage::Recompute => ms(self.t_flush, self.t_persisted),
            Stage::Publish => ms(self.t_persisted, self.t_client_recv),
            // Published while the Git command was still running: visible at `t0`.
            Stage::Total => Some(self.t_client_recv.saturating_sub(self.t0) as f64 / 1e6),
        }
    }
}

/// The samples of one scenario, warm-up already dropped.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: String,
    /// Detection is only isolated in "modify a file" (ADR-GRP-011 § 4, Enmienda 2026-10-04).
    pub isolates_detection: bool,
    /// Provisional non-regression ceiling of the engine total, for a scenario known not to meet
    /// the 300 ms yet: above it the CI fails; between the budget and it, a warning. `None`: the
    /// budget itself is the gate.
    pub ceiling_ms: Option<f64>,
    /// How much later than the engine expects this machine's timer wakes up (p95 measured by the
    /// bench minus the slack the engine discounts, never negative). Added to the budgets of the
    /// debounce and the total: the slack of a virtualized runner is not the engine's
    /// (ADR-GRP-011 § 4, Enmienda 2026-10-05).
    pub slack_excess_ms: f64,
    pub samples: Vec<Sample>,
}

impl Scenario {
    /// Budget of `stage` on this machine: the canonical one, plus the excess timer slack for
    /// the debounce and the total.
    pub fn budget_ms(&self, stage: Stage) -> Option<f64> {
        let base = stage.budget_ms()?;
        Some(match stage {
            Stage::Debounce | Stage::Total => base + self.slack_excess_ms.max(0.0),
            _ => base,
        })
    }

    pub fn summary(&self, stage: Stage) -> Option<Summary> {
        if stage == Stage::Detection && !self.isolates_detection {
            return None;
        }
        let v: Vec<f64> = self
            .samples
            .iter()
            .filter_map(|s| s.stage_ms(stage))
            .collect();
        Summary::of(&v)
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut stages = serde_json::Map::new();
        for stage in Stage::ALL {
            if let Some(s) = self.summary(stage) {
                let (from, to) = stage.marks();
                let mut v = s.to_json();
                v["from"] = from.into();
                v["to"] = to.into();
                v["budget_p95_ms"] = self.budget_ms(stage).into();
                stages.insert(stage.name().into(), v);
            }
        }
        serde_json::json!({
            "scenario": self.name,
            "stages": stages,
            "slack_excess_ms": self.slack_excess_ms,
            "ceiling_ms": self.ceiling_ms,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The CI fails.
    Fail,
    /// Reported, the CI goes on.
    Warn,
}

/// A gate that did not pass, naming what went over.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub level: Level,
    pub scenario: String,
    /// Stage or footprint metric.
    pub what: String,
    pub measured: f64,
    pub limit: f64,
    pub unit: &'static str,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            Level::Fail => "FAIL",
            Level::Warn => "WARN",
        };
        write!(
            f,
            "{level} {}: {} = {:.1} {} > {:.1} {}",
            self.scenario, self.what, self.measured, self.unit, self.limit, self.unit
        )
    }
}

/// Latency gates of ADR-GRP-011 § 4: the engine total over budget fails; a stage over its
/// budget with the total within only warns, with the stage named.
pub fn evaluate_latency(scenario: &Scenario) -> Vec<Finding> {
    let mut out = Vec::new();
    for stage in Stage::ALL {
        let (Some(budget), Some(s)) = (scenario.budget_ms(stage), scenario.summary(stage)) else {
            continue;
        };
        let (from, to) = stage.marks();
        let finding = |level, limit, note: &str| Finding {
            level,
            scenario: scenario.name.clone(),
            what: format!("p95 {} ({from} → {to}){note}", stage.name()),
            measured: s.p95,
            limit,
            unit: "ms",
        };
        match (stage, scenario.ceiling_ms) {
            (Stage::Total, Some(ceiling)) if s.p95 > ceiling => {
                out.push(finding(Level::Fail, ceiling, ", provisional ceiling"));
            }
            (Stage::Total, Some(_)) if s.p95 > budget => {
                out.push(finding(
                    Level::Warn,
                    budget,
                    ", known gap under its ceiling",
                ));
            }
            (Stage::Total, None) if s.p95 > budget => out.push(finding(Level::Fail, budget, "")),
            (Stage::Total, _) => {}
            _ if s.p95 > budget => out.push(finding(Level::Warn, budget, "")),
            _ => {}
        }
    }
    out
}

/// Footprint of the isolated daemon (NFR HUELLA).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Footprint {
    /// Mean CPU over the idle window, in % of one core.
    pub idle_cpu_pct: f64,
    pub idle_rss_mib: f64,
    /// Open descriptors at rest.
    pub fds: f64,
    /// Peak RSS during the 10K-file bursts.
    pub burst_rss_mib: f64,
    /// Mean CPU during the 10K-file bursts, in % of one core. Reported only.
    pub burst_cpu_pct: f64,
    /// Seconds until the RSS was back under the idle limit after the bursts; `None` when it was
    /// not within 60 s.
    pub burst_back_s: Option<f64>,
}

/// Limits of the footprint gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FootprintLimits {
    /// NFR HUELLA: failures.
    pub idle_cpu_pct: f64,
    pub idle_rss_mib: f64,
    pub fds: f64,
    /// Product target of the burst peak (⚠️ ASSUMPTION of the PO, 2026-10-05): a warning until
    /// TD-GRP-001 meets it. No ceiling yet: the peak varies too much between runs to bound it
    /// before TD-GRP-001 explains it (Arquitecto, 2026-10-05).
    pub burst_rss_target_mib: f64,
    /// Product target of the retention (⚠️ ASSUMPTION of the PO): back under the idle RSS limit
    /// within this many seconds after a burst; a warning.
    pub burst_back_target_s: f64,
}

/// Footprint gates (NFR HUELLA; Decisión del orquestador 2026-10-05, validada por Arquitecto y
/// PO). The same on every system: a per-OS slack is a decision to record in
/// `non-functional.md` when a runner needs it.
pub const FOOTPRINT_LIMITS: FootprintLimits = FootprintLimits {
    idle_cpu_pct: 1.0,
    idle_rss_mib: 150.0,
    fds: 256.0,
    burst_rss_target_mib: 250.0,
    burst_back_target_s: 60.0,
};

pub fn evaluate_footprint(f: &Footprint, limits: &FootprintLimits) -> Vec<Finding> {
    let finding = |level, what: &str, measured: f64, limit: f64, unit| Finding {
        level,
        scenario: "footprint".into(),
        what: what.into(),
        measured,
        limit,
        unit,
    };
    // A metric that could not be read is a failure, not a pass.
    let over = |measured: f64, limit: f64| measured > limit || measured.is_nan();
    let mut out = Vec::new();
    for (what, measured, limit, unit) in [
        ("idle CPU", f.idle_cpu_pct, limits.idle_cpu_pct, "%"),
        ("idle RSS", f.idle_rss_mib, limits.idle_rss_mib, "MiB"),
        ("open descriptors", f.fds, limits.fds, ""),
    ] {
        if over(measured, limit) {
            out.push(finding(Level::Fail, what, measured, limit, unit));
        }
    }
    if f.burst_rss_mib > limits.burst_rss_target_mib {
        out.push(finding(
            Level::Warn,
            "burst peak RSS, known gap (TD-GRP-001)",
            f.burst_rss_mib,
            limits.burst_rss_target_mib,
            "MiB",
        ));
    }
    let back = f.burst_back_s.unwrap_or(f64::INFINITY);
    if back > limits.burst_back_target_s {
        out.push(finding(
            Level::Warn,
            "RSS back under the idle limit after the bursts",
            back,
            limits.burst_back_target_s,
            "s",
        ));
    }
    out
}

/// Two runs on the same machine agree when their p95 totals differ by at most 25 ms or 20%,
/// whichever is larger (INF-GRP-002, reproducibility).
pub fn reproducible(p95_a: f64, p95_b: f64) -> bool {
    (p95_a - p95_b).abs() <= (0.2 * p95_a.max(p95_b)).max(25.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    /// A plausible sample: 20 ms detection, 76 ms window, 30 + 10 ms recompute, 2 ms publish.
    fn sample(i: u64, extra_compute_ms: u64) -> Sample {
        let t0 = 1_000 * MS + i * 1_000 * MS;
        let t_recv = t0 + 20 * MS;
        let t_flush = t_recv + 76 * MS;
        let t_computed = t_flush + (30 + extra_compute_ms) * MS;
        let t_persisted = t_computed + 10 * MS;
        let t_published = t_persisted + MS;
        Sample {
            t0,
            t_recv,
            t_flush,
            t_computed,
            t_persisted,
            t_published,
            t_client_recv: t_published + MS,
        }
    }

    fn scenario(extra_compute_ms: u64) -> Scenario {
        Scenario {
            name: "modify".into(),
            isolates_detection: true,
            ceiling_ms: None,
            slack_excess_ms: 0.0,
            samples: (0..200).map(|i| sample(i, extra_compute_ms)).collect(),
        }
    }

    #[test]
    fn percentiles_by_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = Summary::of(&v).unwrap();
        assert_eq!(
            (s.n, s.p50, s.p95, s.p99, s.max),
            (100, 50.0, 95.0, 99.0, 100.0)
        );
        assert_eq!(Summary::of(&[]), None);
        assert_eq!(Summary::of(&[7.0]).unwrap().p95, 7.0);
    }

    #[test]
    fn within_budget_nothing_is_reported() {
        assert_eq!(evaluate_latency(&scenario(0)), vec![]);
    }

    /// Sensitivity: a delay in the recompute that only pushes its stage over 150 ms warns and
    /// names the stage, without failing.
    #[test]
    fn a_stage_over_budget_with_the_total_within_only_warns() {
        let findings = evaluate_latency(&scenario(120));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].level, Level::Warn);
        assert!(findings[0].what.contains("recompute"), "{}", findings[0]);
        assert!(findings[0].what.contains("t_flush → t_persisted"));
    }

    /// Sensitivity: a delay that pushes the engine p95 over 300 ms fails, and the stage that
    /// caused it is named too.
    #[test]
    fn a_delay_that_breaks_the_engine_total_fails() {
        let findings = evaluate_latency(&scenario(200));
        assert!(
            findings
                .iter()
                .any(|f| f.level == Level::Fail && f.what.contains("total")),
            "{findings:?}"
        );
        assert!(findings.iter().any(|f| f.what.contains("recompute")));
    }

    /// A scenario with a provisional ceiling warns between the budget and the ceiling, and
    /// fails above the ceiling.
    #[test]
    fn a_provisional_ceiling_warns_below_it_and_fails_above_it() {
        let mut s = scenario(200);
        s.ceiling_ms = Some(500.0);
        let findings = evaluate_latency(&s);
        assert!(
            findings.iter().all(|f| f.level == Level::Warn),
            "{findings:?}"
        );
        assert!(findings.iter().any(|f| f.what.contains("known gap")));
        s.ceiling_ms = Some(320.0);
        let findings = evaluate_latency(&s);
        assert!(
            findings
                .iter()
                .any(|f| f.level == Level::Fail && f.what.contains("provisional ceiling")),
            "{findings:?}"
        );
    }

    /// A runner whose timer wakes up late gets that excess added to the total and the debounce,
    /// and to nothing else.
    #[test]
    fn the_excess_timer_slack_widens_only_debounce_and_total() {
        let mut s = scenario(200);
        assert!(evaluate_latency(&s).iter().any(|f| f.level == Level::Fail));
        s.slack_excess_ms = 140.0;
        assert_eq!(s.budget_ms(Stage::Total), Some(440.0));
        assert_eq!(s.budget_ms(Stage::Recompute), Some(150.0));
        let findings = evaluate_latency(&s);
        assert!(
            findings.iter().all(|f| f.level == Level::Warn),
            "{findings:?}"
        );
        assert!(findings.iter().any(|f| f.what.contains("recompute")));
    }

    #[test]
    fn ceilings_are_per_platform_and_scenario() {
        assert_eq!(Platform::Linux.burst_ceiling_ms(BURST_1K), None);
        assert_eq!(Platform::MacLocal.burst_ceiling_ms("modify"), None);
        assert!(
            Platform::MacLocal.burst_ceiling_ms(BURST_1K)
                < Platform::MacLocal.burst_ceiling_ms(BURST_10K)
        );
    }

    /// Only 5% of slow samples is still within the p95; 6% is not.
    #[test]
    fn the_gate_is_on_the_p95() {
        let mut s = scenario(0);
        for x in s.samples.iter_mut().take(10) {
            *x = sample(0, 400);
        }
        assert_eq!(evaluate_latency(&s), vec![]);
        s.samples[10] = sample(0, 400);
        s.samples[11] = sample(0, 400);
        assert!(evaluate_latency(&s).iter().any(|f| f.level == Level::Fail));
    }

    /// Git scenarios: the batch opens before `t0`, so detection is not isolated.
    #[test]
    fn detection_is_only_isolated_when_asked() {
        let mut s = scenario(0);
        s.isolates_detection = false;
        for x in &mut s.samples {
            x.t_recv = x.t0 - 5 * MS;
        }
        assert!(s.summary(Stage::Detection).is_none());
        assert!(s.summary(Stage::Total).is_some());
        let json = s.to_json();
        assert!(json["stages"].get("detection").is_none());
        assert_eq!(json["stages"]["total"]["from"], "t0");
        assert_eq!(json["stages"]["publish"]["to"], "t_client_recv");
    }

    #[test]
    fn every_footprint_limit_fails() {
        let ok = Footprint {
            idle_cpu_pct: 0.1,
            idle_rss_mib: 40.0,
            fds: 40.0,
            burst_rss_mib: 90.0,
            burst_cpu_pct: 300.0,
            burst_back_s: Some(2.0),
        };
        assert_eq!(evaluate_footprint(&ok, &FOOTPRINT_LIMITS), vec![]);
        let bad = Footprint {
            idle_cpu_pct: 1.5,
            idle_rss_mib: 151.0,
            fds: 300.0,
            burst_rss_mib: 900.0,
            burst_cpu_pct: 300.0,
            burst_back_s: None,
        };
        let findings = evaluate_footprint(&bad, &FOOTPRINT_LIMITS);
        let fails = findings.iter().filter(|f| f.level == Level::Fail).count();
        assert_eq!(fails, 3, "{findings:?}");
        // The burst peak has no ceiling yet: it only warns.
        assert!(
            findings
                .iter()
                .any(|f| f.level == Level::Warn && f.what.contains("burst peak"))
        );
        // Retention only warns.
        assert!(
            findings
                .iter()
                .any(|f| f.level == Level::Warn && f.what.contains("back under"))
        );
        let unknown = Footprint {
            idle_rss_mib: f64::NAN,
            ..ok
        };
        assert_eq!(evaluate_footprint(&unknown, &FOOTPRINT_LIMITS).len(), 1);
    }

    /// Between the product target and the provisional ceiling, the burst peak only warns.
    #[test]
    fn the_burst_peak_only_warns() {
        let f = Footprint {
            idle_cpu_pct: 0.1,
            idle_rss_mib: 40.0,
            fds: 40.0,
            burst_rss_mib: 300.0,
            burst_cpu_pct: 100.0,
            burst_back_s: Some(1.0),
        };
        let findings = evaluate_footprint(&f, &FOOTPRINT_LIMITS);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].level, Level::Warn);
    }

    #[test]
    fn reproducibility_tolerance() {
        assert!(reproducible(100.0, 120.0));
        assert!(reproducible(200.0, 160.0));
        assert!(!reproducible(100.0, 130.0));
        assert!(!reproducible(250.0, 190.0));
    }

    /// The stage names of the report are the canonical marks of ADR-GRP-011 § 2.
    #[test]
    fn marks_use_the_canonical_names() {
        let canonical = [
            "t0",
            "t_recv",
            "t_flush",
            "t_computed",
            "t_persisted",
            "t_published",
            "t_client_recv",
            "t_render",
        ];
        for stage in Stage::ALL {
            let (from, to) = stage.marks();
            assert!(
                canonical.contains(&from) && canonical.contains(&to),
                "{stage:?}"
            );
        }
    }
}
