//! The Guardrails configuration directory is out of an agent's reach (BR-AUTH-004).
//!
//! stub: replaced by the implementation slice (the movement is not inspected yet).

use gitraptor_api::AgentKind;

use super::Evaluation;
use super::glob::Budget;
use super::policies::Touched;

/// The Guardrails configuration directory, anchored at the root.
pub const CONFIG_PATTERN: &str = "/.gitraptor/";

/// An agent's movement whose new commits touch `/.gitraptor/` is denied with
/// `policy.config-protected` (level `minimum`, params `path` = first such path, `pattern`);
/// `touched.unverifiable` with an agent denies with `Cause::Unverifiable`, no params. The
/// person (`actor = None`) is never governed.
pub fn protect_config(
    _out: &mut Evaluation,
    _touched: &Touched,
    _actor: Option<AgentKind>,
    _budget: &mut Budget,
) {
    // stub: replaced by the implementation slice
}
