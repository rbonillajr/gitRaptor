//! Control run (ADR-GRP-009, Validación 3): each scenario runs twice on two fixtures built by
//! the same code, with and without the engine, with the same user/agent actions. Only the
//! difference between both is imputed to the engine.
//!
//! Decisión del orquestador (2026-10-04), validada por el Arquitecto: the subtraction only
//! applies when the control run itself shows changes (user or agent activity). Without that
//! activity the control must be empty and the engine run must show zero differences. The
//! subtraction matches by scope, path and kind of change, so in concurrent scenarios an engine
//! effect on a path the user also touched is masked; there the guarantee comes from the `exec`
//! audit and the argv log, not from the fingerprint.

use std::fmt;

use crate::exceptions::Exceptions;
use crate::fingerprint::{Change, diff};
use crate::fixture::Fixture;

type Action<'a> = Box<dyn Fn(&Fixture) + Sync + 'a>;

/// One step of a scenario.
pub enum Step<'a> {
    /// The user or an agent; runs in both runs.
    User(Action<'a>),
    /// The engine (code under test); runs only in the engine run.
    Engine(Action<'a>),
    /// User and engine at the same time; the control run only runs `user`.
    Concurrently {
        user: Action<'a>,
        engine: Action<'a>,
    },
}

impl<'a> Step<'a> {
    pub fn user(f: impl Fn(&Fixture) + Sync + 'a) -> Self {
        Self::User(Box::new(f))
    }

    pub fn engine(f: impl Fn(&Fixture) + Sync + 'a) -> Self {
        Self::Engine(Box::new(f))
    }

    pub fn concurrently(
        user: impl Fn(&Fixture) + Sync + 'a,
        engine: impl Fn(&Fixture) + Sync + 'a,
    ) -> Self {
        Self::Concurrently {
            user: Box::new(user),
            engine: Box::new(engine),
        }
    }
}

/// How the imputable changes were computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Empty control: every difference of the engine run is imputed to the engine.
    Strict,
    /// User activity: differences also present in the control run are not imputed.
    Subtracted,
}

/// Result of a scenario: what the engine caused, with enough context to read a failure.
#[derive(Debug, Clone)]
pub struct Report {
    pub scenario: String,
    pub mode: Mode,
    pub control: Vec<Change>,
    pub imputable: Vec<Change>,
}

impl Report {
    pub fn is_intact(&self) -> bool {
        self.imputable.is_empty()
    }

    /// Panics with the report if the engine caused any difference.
    #[track_caller]
    pub fn assert_intact(&self) {
        assert!(self.is_intact(), "{self}");
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "scenario '{}' ({:?}): {} change(s) imputable to the engine",
            self.scenario,
            self.mode,
            self.imputable.len()
        )?;
        for c in &self.imputable {
            writeln!(f, "  - {c}")?;
        }
        if !self.control.is_empty() {
            writeln!(f, "  control run (not imputed): {}", self.control.len())?;
        }
        Ok(())
    }
}

/// A scenario: a fixture builder, steps and the exceptions it may show.
pub struct Scenario<'a> {
    pub name: String,
    pub build: Box<dyn Fn() -> Fixture + 'a>,
    pub steps: Vec<Step<'a>>,
    pub exceptions: Exceptions,
}

impl<'a> Scenario<'a> {
    pub fn new(name: impl Into<String>, build: impl Fn() -> Fixture + 'a) -> Self {
        Self {
            name: name.into(),
            build: Box::new(build),
            steps: Vec::new(),
            exceptions: Exceptions::none(),
        }
    }

    pub fn step(mut self, step: Step<'a>) -> Self {
        self.steps.push(step);
        self
    }

    pub fn exceptions(mut self, exceptions: Exceptions) -> Self {
        self.exceptions = exceptions;
        self
    }

    /// Run the engine run and the control run and compute what is imputable to the engine.
    pub fn run(&self) -> Report {
        let engine = self.run_once(true);
        let control = self.run_once(false);
        let (mode, imputable) = if control.is_empty() {
            (Mode::Strict, engine)
        } else {
            (Mode::Subtracted, subtract(&engine, &control))
        };
        Report {
            scenario: self.name.clone(),
            mode,
            control,
            imputable,
        }
    }

    fn run_once(&self, with_engine: bool) -> Vec<Change> {
        let f = (self.build)();
        let before = f.snapshot(&self.exceptions);
        for step in &self.steps {
            match step {
                Step::User(user) => user(&f),
                Step::Engine(engine) if with_engine => engine(&f),
                Step::Engine(_) => {}
                Step::Concurrently { user, engine } if with_engine => {
                    std::thread::scope(|s| {
                        s.spawn(|| engine(&f));
                        user(&f);
                    });
                }
                Step::Concurrently { user, .. } => user(&f),
            }
        }
        let after = f.snapshot(&self.exceptions);
        self.exceptions
            .filter(&diff(&before, &after), &before, &after)
    }
}

/// Changes of `engine` with no change of the same kind on the same path in `control`.
pub fn subtract(engine: &[Change], control: &[Change]) -> Vec<Change> {
    engine
        .iter()
        .filter(|e| {
            !control
                .iter()
                .any(|c| c.scope == e.scope && c.path == e.path && c.kind.same_kind(&e.kind))
        })
        .cloned()
        .collect()
}

/// One fixture, no control: snapshot, run `engine`, snapshot. For scenarios without user
/// activity, where the control would be empty anyway.
pub fn check(name: &str, f: &Fixture, exceptions: &Exceptions, engine: impl FnOnce()) -> Report {
    let before = f.snapshot(exceptions);
    engine();
    let after = f.snapshot(exceptions);
    Report {
        scenario: name.into(),
        mode: Mode::Strict,
        control: Vec::new(),
        imputable: exceptions.filter(&diff(&before, &after), &before, &after),
    }
}
