//! Presentation (ADR-CKP-003 § 8 and § 10): the ingest, the only place
//! where contract text becomes [`SafeText`], and the typed i18n catalog.

pub mod i18n;
pub mod ingest;
pub mod prediction_text;
mod sanitize;

pub use sanitize::{NAME_MAX_CHARS, SafeText, TEXT_MAX_CHARS};

#[cfg(test)]
mod prediction_text_tests;
