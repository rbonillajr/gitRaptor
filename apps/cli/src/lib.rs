//! Internal library of `raptor` (ADR-CKP-003 § 12): the Cockpit TUI and its
//! channel client, so tests and the bench drive the `App` without a
//! screen. Not published.
//!
//! Boundary (Validation V5): `tui`, `model`, `client`, `present` and
//! `queue` never import the engine, the Git layer or the policy layer.
//! `link` is the named exception: it plugs in the channel client library,
//! which still lives in `crates/core` until it moves to `crates/api`.

pub mod client;
pub mod link;
pub mod model;
pub mod present;
pub mod queue;
pub mod tui;
