//! Is the hook layer of a repo still active? (US-GRD-004, ADR-GRD-005 § 1.) Read-only.

use std::path::Path;

use gitraptor_api::guard::{Diagnostic, HooksLayer};

use super::journal::Journal;

/// What a check found: the layer and the diagnostics that do not change the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub hooks: HooksLayer,
    pub diagnostics: Vec<Diagnostic>,
}

/// The value of `core.hooksPath` as read; `Err` when the configuration could not be read.
pub type Key = Result<Option<String>, ()>;

pub fn check_with(_common: &Path, _journal: Option<&Journal>, _key: Key) -> Health {
    unimplemented!("US-GRD-004")
}

pub fn fingerprint(_common: &Path, _journal: &Journal) -> u64 {
    unimplemented!("US-GRD-004")
}
