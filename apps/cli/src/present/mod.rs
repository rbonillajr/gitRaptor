//! Presentation (ADR-CKP-003 § 8 and § 10): the ingest, the only place
//! where contract text becomes [`SafeText`], and the typed i18n catalog.

pub mod conflict_alert_text;
pub mod diff;
pub mod diff_text;
pub mod freshness_text;
pub mod graph;
pub mod graph_text;
pub mod i18n;
pub mod ingest;
pub mod prediction;
pub mod prediction_text;
mod sanitize;

pub use sanitize::{NAME_MAX_CHARS, SafeText, TEXT_MAX_CHARS};

#[cfg(test)]
mod graph_tests;
#[cfg(test)]
mod graph_text_tests;
#[cfg(test)]
mod prediction_text_tests;
