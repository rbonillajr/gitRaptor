//! Internal library of `raptor` (ADR-CKP-003 § 12): the Cockpit TUI and its
//! channel client, so tests and the bench drive the `App` without a
//! screen. Not published.
//!
//! Boundary (Validation V5): `tui`, `model`, `client`, `present` and
//! `queue` never import the engine, the Git layer or the policy layer, with
//! no exception: the channel client library lives in `crates/api`, and the
//! binary injects how a daemon is started (INF-CKP-001 Entrega 2b).

pub mod client;
pub mod model;
pub mod present;
pub mod queue;
pub mod tui;
