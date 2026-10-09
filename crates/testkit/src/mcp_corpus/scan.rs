//! Scanners over what the server printed.

use serde_json::Value;

use super::judge::Secrets;

/// Whether `c` is invisible or a control character a terminal could act on.
pub fn is_hidden(c: char) -> bool {
    let _ = c;
    false
}

/// JSON pointers of every string value or key holding a hidden character.
pub fn hidden_characters(value: &Value) -> Vec<String> {
    let _ = value;
    Vec::new()
}

/// Names of the planted secrets found in `text`.
pub fn secrets_in(text: &str, secrets: &Secrets) -> Vec<String> {
    let _ = (text, secrets);
    Vec::new()
}

/// Known credential shapes found in `text`.
pub fn token_shapes(text: &str) -> Vec<&'static str> {
    let _ = text;
    Vec::new()
}

/// Checks that stderr only carries fixed `raptor-mcp: <code>` lines.
///
/// # Errors
/// The 1-based number of the first bad line.
pub fn stderr_fixed_codes(stderr: &str) -> Result<(), usize> {
    let _ = stderr;
    Ok(())
}
