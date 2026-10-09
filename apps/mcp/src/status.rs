//! The `status` tool's argument: the cursor of a list the last answer did not include. The
//! cursor is checked here, before the engine is asked, so nothing malformed reaches it.
//!
//! Signature only: the behaviour is built against the tests of `server::status_tests`.
// The tool does not call it yet.
#![allow(dead_code)]

use rmcp::model::JsonObject;

/// The `cursor` of a call, or the name of the field that makes it malformed: an unknown
/// argument, or a `cursor` that is not a string of exactly 16 characters of `[0-9a-f]`
/// (ADR-MCP-001 § 4.2). `Ok(None)` for a call without one.
pub(crate) fn cursor_argument(arguments: Option<&JsonObject>) -> Result<Option<String>, &str> {
    let _ = arguments;
    todo!("US-MCP-004: validate the cursor argument")
}
