//! Write layer of the Time Machine (ADR-TMC-002 § 1), separate from the read layer.
//!
//! Only `crates/core::timemachine` may use it (checked by `crates/core/tests/static_check.rs`).
//! For now it holds the store writer; the applier of undo, redo and restore (TS-TMC-003) will
//! join it as its own submodule.

pub mod store;
