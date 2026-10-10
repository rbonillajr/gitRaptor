//! Time Machine: snapshots and universal undo (F-001-03). This module holds
//! the parts that live in the engine (TQ-2 → a); the write layer on the
//! user's repo lives in `crates/git` (ADR-TMC-002).

pub mod apply;
pub mod chaos;
pub mod confirm;
pub mod continuous;
pub mod engine;
pub mod kept;
pub mod manual;
pub mod notices;
pub mod oplog;
pub mod protected;
/// The repo write lock lives in a neutral module shared with the executor
/// (ADR-CKP-002 § 5); re-exported for the applier.
pub use crate::repo_lock;
pub mod restore;
pub mod store;
pub mod sweep;
pub mod timeline;
pub mod undo;
