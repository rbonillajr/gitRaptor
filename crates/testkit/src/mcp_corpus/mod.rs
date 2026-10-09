//! Declarative MCP attack corpus: the case model and its loader, the judge that applies the same
//! checks to every case, the secret scanner, the output-schema checker and the KPI report.
//!
//! Pure logic only (`std` and `serde_json`): the runner that drives the real binaries lives next to
//! the tests that use it, and passes the size limits in as [`Limits`].
//!
//! - [`case`]: the closed JSON format of a case, its locations and placeholders.
//! - [`judge`]: the observation of one session and the verdict on it.
//! - [`scan`]: hidden characters, planted canaries, known token shapes, fixed stderr codes.
//! - [`schema`]: the JSON Schema subset the compact output schemas use, failing closed.
//! - [`report`]: per-case outcomes, the KPI and the gate.

pub mod case;
pub mod judge;
pub mod report;
pub mod scan;
pub mod schema;

pub use case::{Case, CaseError, Expect, Platform, Tier, expand, load_dir, parse};
pub use judge::{Answer, Failure, Limits, Observation, Secrets, Stream, Verdict, judge};
pub use report::{Outcome, Report, Row};
