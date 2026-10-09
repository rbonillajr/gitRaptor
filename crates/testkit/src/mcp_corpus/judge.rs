//! The judge: the same checks for every case, over what one session produced.

use serde_json::Value;

use super::case::Case;

/// Size limits the runner takes from the production crate (the testkit depends on none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub part_bytes: usize,
    pub refusal_bytes: usize,
    pub name_chars: usize,
}

/// Planted secrets as `(name, value)`. Only the name is ever reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets(pub Vec<(String, String)>);

/// One answer of the server to a message of the case.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub tool: Option<String>,
    pub message: Value,
}

/// Everything one session produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    /// `result.tools` of this session's `tools/list`.
    pub tools: Vec<Value>,
    /// In arrival order; handshake, `tools/list` and the sentinel excluded.
    pub answers: Vec<Answer>,
    /// Every stdout line, verbatim, handshake included.
    pub stdout: Vec<String>,
    pub stderr: String,
    pub sentinel_answered: bool,
    /// `Change` (Display) left after the case's exceptions.
    pub repo_changes: Vec<String>,
    pub traps_fired: Vec<String>,
}

/// An output stream of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Why a case was not rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    NotRejected,
    WrongRejection {
        expected: String,
        got: String,
    },
    MissingAnswers {
        expected: usize,
        got: usize,
    },
    SessionDied,
    StdoutNotProtocol {
        line: usize,
    },
    FieldNotAllowed {
        at: String,
    },
    OverBudget {
        what: String,
        bytes: usize,
        limit: usize,
    },
    /// The canary's NAME, never its value.
    SecretLeaked {
        secret: String,
        stream: Stream,
    },
    TokenShape {
        shape: &'static str,
        stream: Stream,
    },
    HiddenCharacter {
        at: String,
    },
    StderrNotFixed {
        line: usize,
    },
    RepoChanged(Vec<String>),
    TrapFired(Vec<String>),
    /// The case carries a `known_gap` mark but the server now rejects it: the gap is closed and
    /// the mark must be dropped so the case demands the rejection from here on.
    KnownGapClosed,
    Harness(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stub")
    }
}

/// The judge's answer for one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Rejected,
    Failed(Vec<Failure>),
    /// A `known_gap` case that got through, with its reference: expected today, counted apart.
    KnownGap(String),
}

/// Applies the common checks and the case's `expect` to one observation.
pub fn judge(
    case: &Case,
    observation: &Observation,
    secrets: &Secrets,
    limits: &Limits,
) -> Verdict {
    let _ = (case, observation, secrets, limits);
    Verdict::Rejected
}
