//! Guardrails hook layer (US-GRD-001): install with the developer's permission, native
//! dispatchers, the hook client and the evaluation of the safe minimum.
//!
//! - [`install`]: the transaction of ADR-GRD-001 § 4, run by the daemon loop, the only writer.
//! - [`evaluate`]: the decision of ADR-GRD-003, served by the channel and by degraded mode.
//! - [`hook`]: `raptor hook`, the client a dispatcher starts.
//!
//! Only this module reaches the Guardrails write layer of `crates/git` (ADR-GRD-001 § 7).

pub mod actor;
pub mod authorship;
pub mod constants;
pub mod cut;
pub mod evaluate;
pub mod health;
#[cfg(test)]
mod health_tests;
pub mod hook;
pub mod install;
pub mod journal;
pub mod log;
pub mod pending;
pub mod prior;
pub mod protection;
pub mod registry;
pub mod second_line;
pub mod uninstall;

pub use registry::{GuardEntry, GuardRegistry};
