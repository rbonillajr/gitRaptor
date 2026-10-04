//! Time Machine: snapshots and universal undo (F-001-03). This module holds
//! the parts that live in the engine (TQ-2 → a); the write layer on the
//! user's repo lives in `crates/git` (ADR-TMC-002).

pub mod oplog;
