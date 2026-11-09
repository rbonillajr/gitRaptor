//! The copyable command of a conscious exception, as a pure function of a log entry.

use std::path::Path;

use gitraptor_api::guard::{GuardLogEntry, Rule};
use serde::Serialize;

/// Rules a conscious exception can skip.
pub const SKIPPABLE: &[Rule] = &[];

/// Why no command is offered for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotOffered {
    Commit,
    Rebase,
    Operation,
    Rule,
    Remote,
    WhereUnknown,
    RateLimited,
    UnsafeText,
    RaptorUnknown,
}

/// The command that announces an exception for the denial in `entry`.
///
/// # Errors
///
/// A [`NotOffered`] reason when the entry cannot be turned into a safe command.
pub fn exception(_raptor: Option<&Path>, _entry: &GuardLogEntry) -> Result<String, NotOffered> {
    Err(NotOffered::Operation)
}

/// The command that cancels the pending exception of the repo at `path`.
#[must_use]
pub fn cancel(_raptor: Option<&Path>, _path: &str) -> Option<String> {
    None
}
