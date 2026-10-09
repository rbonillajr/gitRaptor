//! "Intact repo" harness (INF-GRP-001): the core that every engine story is gated on
//! (BR-CONS-001, NFR-01, ADR-GRP-009 § Validación), extended for the Guardrails hook layer
//! (INF-GRD-001: prior hooks, install footprint, cut points and interceptability matrix).
//!
//! **Test support only.** Any crate may use it as a `[dev-dependencies]` entry and never as a
//! regular dependency (checked by `tests/dev_only.rs`). It does not depend on any GitRaptor crate,
//! so a crate's own tests never link two copies of that crate: the code under test is plugged in
//! as closures and probes.
//!
//! - [`fingerprint`]: snapshot and diff of the repo, its worktrees and what lies outside it.
//! - [`exceptions`]: the only differences a scenario may show (engine profile, autostart PQ-1,
//!   Guardrails install).
//! - [`control`]: control run, imputing to the engine only what it causes.
//! - [`fixture`]: the temporary machine (home, repo, other repo, profile).
//! - [`guard`]: the harness only touches roots it created; never the GitRaptor repo.
//! - [`canary`] (unix): SEC-09 canary repo.
//! - [`exec_audit`]: trap audit (portable gate) and kernel tracers (strace, eslogger).
//! - [`hooks`]: repos with prior hooks and simulated hook managers (INF-GRD-001).
//! - [`cut`]: named cut points of the hook-layer transactions and their sweep (NFR-12).
//! - [`interceptability`]: the matrix executor and the hook-process counter (ADR-GRD-002).
//! - [`repogen`]: deterministic reference repos of SPIKE-TMC-001 (profile `M`, D-TMC-21) and of
//!   INF-GRP-002 (profile `H`, 100K commits).
//! - [`freshness`]: statistics and gates of the freshness and footprint bench (INF-GRP-002).
//! - [`mcp_corpus`]: declarative MCP attack corpus: case model, judge, scanners and KPI report.
//! - [`sibling`]: binaries of another workspace package, next to the binary under test.

#[cfg(unix)]
pub mod canary;
pub mod control;
pub mod cut;
pub mod exceptions;
pub mod exec_audit;
pub mod fingerprint;
pub mod fixture;
pub mod freshness;
pub mod guard;
pub mod hooks;
pub mod interceptability;
pub mod mcp_corpus;
pub mod repogen;
pub mod sibling;

pub use control::{Mode, Report, Scenario, Step, check};
pub use exceptions::{Exception, Exceptions};
pub use fingerprint::{Change, ChangeKind, Field, Scope, Snapshot, diff};
pub use fixture::Fixture;
pub use sibling::sibling_bin;
