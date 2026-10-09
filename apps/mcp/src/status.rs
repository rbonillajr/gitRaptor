//! The `status` tool's argument: the cursor of a list the last answer did not include. The
//! cursor is checked here, before the engine is asked, so nothing malformed reaches it.

use gitraptor_api::methods::valid_cursor;
use rmcp::model::JsonObject;

/// The `cursor` of a call, or the name of the field that makes it malformed: an unknown
/// argument, or a `cursor` that is not a string of exactly 16 characters of `[0-9a-f]`
/// (ADR-MCP-001 § 4.2). `Ok(None)` for a call without one.
pub(crate) fn cursor_argument(arguments: Option<&JsonObject>) -> Result<Option<String>, &str> {
    let Some(arguments) = arguments else {
        return Ok(None);
    };
    // NFR-02: no argument but the cursor, so none can name another repo.
    if let Some(field) = arguments.keys().find(|k| k.as_str() != "cursor") {
        return Err(field);
    }
    match arguments.get("cursor") {
        None => Ok(None),
        Some(value) => match value.as_str() {
            Some(cursor) if valid_cursor(cursor) => Ok(Some(cursor.to_owned())),
            _ => Err("cursor"),
        },
    }
}
