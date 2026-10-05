//! Named cut points of the hook-layer transactions (INF-GRD-001; NFR-12; ADR-GRD-001 § 4 and
//! Validación 3): kill the code under test before and after each step, run the recovery, and
//! require the repo to be **complete** or **identical** to what it was.
//!
//! # Protocol (what the code under test implements, only in test builds)
//!
//! The product reads two variables behind the cargo feature `test-cuts`, never in a release build
//! (a CI check in US-GRD-001 greps the release binary for [`ENV_CUT`]):
//!
//! - [`ENV_TRACE`] = a file path: append one line `<step>:<when>` for every point passed, where
//!   `when` is `before`, `during` or `after`.
//! - [`ENV_CUT`] = `<step>:<when>`: when that point is reached, die at once with exit code
//!   [`EXIT_CODE`], without cleanup (no unwinding, no `Drop`). `during` is a point inside a step
//!   that writes through Git: the process dies leaving the on-disk state of a Git killed mid-write
//!   (for `git config`, an orphan `config.lock`).
//!
//! [`trip`] implements the protocol for test-only code (the harness's own reference installer).
//!
//! # Sweep
//!
//! [`Sweep::run`] first runs the transaction without cuts, with the trace: every declared point
//! must be reached, and nothing else (a step the spec does not name is an error). Then, for each
//! point, on a fresh fixture: run the transaction killed at that point, run the recovery (another
//! process: the daemon start), and classify the result against the two allowed end states.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::process::ExitStatus;

use crate::exceptions::Exceptions;
use crate::fingerprint::{Change, Snapshot, diff};
use crate::fixture::Fixture;

/// `<step>:<when>` at which the code under test must die.
pub const ENV_CUT: &str = "GITRAPTOR_TEST_CUT";
/// File where the code under test appends every point it passes.
pub const ENV_TRACE: &str = "GITRAPTOR_TEST_CUT_TRACE";
/// Exit code of a process killed at its cut point.
pub const EXIT_CODE: i32 = 86;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum When {
    Before,
    /// Inside a step that writes through Git (Validación 3: "durante la escritura de la clave").
    During,
    After,
}

impl When {
    fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::During => "during",
            Self::After => "after",
        }
    }
}

/// One named cut point.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CutPoint {
    pub step: String,
    pub when: When,
}

impl CutPoint {
    pub fn new(step: impl Into<String>, when: When) -> Self {
        Self {
            step: step.into(),
            when,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (step, when) = s.trim().rsplit_once(':')?;
        let when = match when {
            "before" => When::Before,
            "during" => When::During,
            "after" => When::After,
            _ => return None,
        };
        (!step.is_empty()).then(|| Self::new(step, when))
    }
}

impl fmt::Display for CutPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.step, self.when.as_str())
    }
}

/// `before` and `after` of every step, plus `during` of the steps listed in `during`, in
/// transaction order.
pub fn points(steps: &[&str], during: &[&str]) -> Vec<CutPoint> {
    let mut out = Vec::new();
    for step in steps {
        out.push(CutPoint::new(*step, When::Before));
        if during.contains(step) {
            out.push(CutPoint::new(*step, When::During));
        }
        out.push(CutPoint::new(*step, When::After));
    }
    out
}

/// The protocol for test-only code under test: trace the point and die if it is the cut.
pub fn trip(step: &str, when: When) {
    let point = CutPoint::new(step, when);
    trace(&point);
    if std::env::var(ENV_CUT)
        .ok()
        .and_then(|s| CutPoint::parse(&s))
        == Some(point)
    {
        std::process::exit(EXIT_CODE);
    }
}

/// [`trip`] for a [`When::During`] point: when it is the cut, `on_cut` leaves the on-disk state
/// of the interrupted write (e.g. an orphan `config.lock`) before the process dies.
pub fn trip_during(step: &str, on_cut: impl FnOnce()) {
    let point = CutPoint::new(step, When::During);
    if std::env::var(ENV_CUT)
        .ok()
        .and_then(|s| CutPoint::parse(&s))
        == Some(point)
    {
        trace(&CutPoint::new(step, When::During));
        on_cut();
        std::process::exit(EXIT_CODE);
    }
    trip(step, When::During);
}

fn trace(point: &CutPoint) {
    if let Some(trace) = std::env::var_os(ENV_TRACE) {
        use std::io::Write;
        let mut file = std::fs::File::options()
            .create(true)
            .append(true)
            .open(trace)
            .expect("open cut trace");
        writeln!(file, "{point}").expect("write cut trace");
    }
}

/// How a run of the code under test ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Died at its cut point.
    Cut,
    /// Ran to the end successfully.
    Finished,
    /// Any other exit (a refusal, a panic, a signal).
    Failed(Option<i32>),
}

impl From<ExitStatus> for Exit {
    fn from(s: ExitStatus) -> Self {
        match s.code() {
            Some(EXIT_CODE) => Self::Cut,
            Some(0) => Self::Finished,
            code => Self::Failed(code),
        }
    }
}

/// Which snapshot an end state is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Baseline {
    /// Before [`Sweep::prepare`] (the pristine repo).
    Pristine,
    /// After [`Sweep::prepare`], right before the transaction.
    Start,
}

/// Semantic check of an end state; `Err` explains what is off.
pub type Probe<'a> = Box<dyn Fn(&Fixture) -> Result<(), String> + 'a>;

/// An allowed end state: the differences with its baseline that it admits, and a probe of the
/// semantic state (e.g. the effective `core.hooksPath` of every worktree).
pub struct EndState<'a> {
    pub name: &'static str,
    pub baseline: Baseline,
    pub exceptions: Exceptions,
    pub probe: Probe<'a>,
}

impl<'a> EndState<'a> {
    pub fn new(
        name: &'static str,
        baseline: Baseline,
        exceptions: Exceptions,
        probe: impl Fn(&Fixture) -> Result<(), String> + 'a,
    ) -> Self {
        Self {
            name,
            baseline,
            exceptions,
            probe: Box::new(probe),
        }
    }
}

type Run<'a> = Box<dyn Fn(&Fixture, Option<&CutPoint>, Option<&Path>) -> Exit + 'a>;

/// A transaction swept with a cut at every named point.
pub struct Sweep<'a> {
    pub scenario: String,
    pub build: Box<dyn Fn() -> Fixture + 'a>,
    /// Runs on the fixture before the [`Baseline::Start`] snapshot (e.g. a complete install
    /// before sweeping the uninstall).
    pub prepare: Box<dyn Fn(&Fixture) + 'a>,
    /// Launch the transaction (normally a child process) with [`ENV_CUT`] and [`ENV_TRACE`] set
    /// from its arguments.
    pub run: Run<'a>,
    /// Launch the recovery (daemon start), also in its own process.
    pub recover: Box<dyn Fn(&Fixture) + 'a>,
    pub points: Vec<CutPoint>,
    /// The end state if nothing was done ("idéntico al anterior").
    pub unchanged: EndState<'a>,
    /// The end state if the transaction is complete.
    pub done: EndState<'a>,
}

/// Result at one cut point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// After the recovery the repo is in this end state.
    Reached(&'static str),
    /// The process did not die at the point (it finished, failed, or never reached it).
    NotCut(Exit),
    /// Neither end state: the differences against each, and the probe failures.
    Broken {
        unchanged: Vec<Change>,
        done: Vec<Change>,
        probes: Vec<String>,
    },
}

/// Result of a sweep. Its `Display` names the scenario, the cut point, the path and the kind of
/// change of every failure (INF-GRD-001, Verificación manual).
#[derive(Debug, Clone)]
pub struct SweepReport {
    pub scenario: String,
    /// Problems of the uncut run: points declared but not reached, or reached but not declared.
    pub coverage: Vec<String>,
    pub results: Vec<(CutPoint, Outcome)>,
}

impl SweepReport {
    pub fn is_ok(&self) -> bool {
        self.coverage.is_empty()
            && !self.results.is_empty()
            && self
                .results
                .iter()
                .all(|(_, o)| matches!(o, Outcome::Reached(_)))
    }

    #[track_caller]
    pub fn assert_ok(&self) {
        assert!(self.is_ok(), "{self}");
    }

    pub fn outcome(&self, point: &str) -> Option<&Outcome> {
        let p = CutPoint::parse(point)?;
        self.results.iter().find(|(c, _)| *c == p).map(|(_, o)| o)
    }
}

impl fmt::Display for SweepReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "cut sweep '{}': {} point(s)",
            self.scenario,
            self.results.len()
        )?;
        if self.results.is_empty() {
            writeln!(f, "  no cut point declared")?;
        }
        for c in &self.coverage {
            writeln!(f, "  coverage: {c}")?;
        }
        for (point, outcome) in &self.results {
            match outcome {
                Outcome::Reached(state) => writeln!(f, "  {point}: ok ({state})")?,
                Outcome::NotCut(exit) => writeln!(
                    f,
                    "  {point}: FAILED, the process did not die there ({exit:?})"
                )?,
                Outcome::Broken {
                    unchanged,
                    done,
                    probes,
                } => {
                    writeln!(
                        f,
                        "  {point}: FAILED, neither identical nor complete after recovery"
                    )?;
                    for c in unchanged {
                        writeln!(f, "    vs identical: {c}")?;
                    }
                    for c in done {
                        writeln!(f, "    vs complete: {c}")?;
                    }
                    for p in probes {
                        writeln!(f, "    probe: {p}")?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl<'a> Sweep<'a> {
    pub fn run(&self) -> SweepReport {
        let coverage = self.coverage();
        let mut results = Vec::new();
        for point in &self.points {
            results.push((point.clone(), self.run_at(point)));
        }
        SweepReport {
            scenario: self.scenario.clone(),
            coverage,
            results,
        }
    }

    /// Uncut run with the trace: declared points == reached points.
    fn coverage(&self) -> Vec<String> {
        let f = (self.build)();
        (self.prepare)(&f);
        let trace = f.root.join("cut-trace.txt");
        let exit = (self.run)(&f, None, Some(&trace));
        let mut out = Vec::new();
        if exit != Exit::Finished {
            out.push(format!("the uncut run did not finish: {exit:?}"));
        }
        let reached: BTreeSet<CutPoint> = std::fs::read_to_string(&trace)
            .unwrap_or_default()
            .lines()
            .filter_map(CutPoint::parse)
            .collect();
        let declared: BTreeSet<CutPoint> = self.points.iter().cloned().collect();
        for p in declared.difference(&reached) {
            out.push(format!("{p} declared but never reached"));
        }
        for p in reached.difference(&declared) {
            out.push(format!("{p} reached but not declared"));
        }
        out
    }

    fn run_at(&self, point: &CutPoint) -> Outcome {
        let f = (self.build)();
        let keep = |e: &Exceptions| e.keep_content();
        let mut kept = keep(&self.unchanged.exceptions);
        kept.extend(keep(&self.done.exceptions));
        let snap = |f: &Fixture| Snapshot::take(&scopes_without_trace(f), &kept);
        let pristine = snap(&f);
        (self.prepare)(&f);
        let start = snap(&f);
        let exit = (self.run)(&f, Some(point), None);
        if exit != Exit::Cut {
            return Outcome::NotCut(exit);
        }
        (self.recover)(&f);
        let end = snap(&f);
        let check = |state: &EndState<'_>| {
            let base = match state.baseline {
                Baseline::Pristine => &pristine,
                Baseline::Start => &start,
            };
            let changes = state.exceptions.filter(&diff(base, &end), base, &end);
            let probe = (state.probe)(&f);
            (changes, probe)
        };
        let (unchanged, probe_u) = check(&self.unchanged);
        if unchanged.is_empty() && probe_u.is_ok() {
            return Outcome::Reached(self.unchanged.name);
        }
        let (done, probe_d) = check(&self.done);
        if done.is_empty() && probe_d.is_ok() {
            return Outcome::Reached(self.done.name);
        }
        let probes = [probe_u, probe_d]
            .into_iter()
            .zip([self.unchanged.name, self.done.name])
            .filter_map(|(p, name)| p.err().map(|e| format!("{name}: {e}")))
            .collect();
        Outcome::Broken {
            unchanged,
            done,
            probes,
        }
    }
}

/// Every scope of the fixture; the trace file lives in the root, outside every scope.
fn scopes_without_trace(f: &Fixture) -> Vec<crate::fingerprint::Scope> {
    f.scopes()
        .into_iter()
        .filter(|s| s.root.file_name().is_none_or(|n| n != "cut-trace.txt"))
        .collect()
}
