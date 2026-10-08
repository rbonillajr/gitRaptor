//! What `raptor-mcp` sends to the model (US-MCP-005, ADR-MCP-001 § 5 and
//! § 6): the bounds of a tool result, the closed list of tool error codes
//! and the one pass that turns every untrusted text of a result into data
//! that cannot hide anything.
//!
//! Names are cut per class by their MCP view ([`UntrustedText::mcp_name`]);
//! [`for_mcp`] then rewrites every `{"untrusted": …}` object of the result,
//! whatever type produced it, so a field a later story adds is escaped even
//! if nobody remembers to.
//!
//! [`UntrustedText::mcp_name`]: crate::untrusted::UntrustedText::mcp_name

use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Actor;
use crate::methods::{McpStatus, McpStatusAction};
use crate::untrusted::{UntrustedName, sanitize};

/// Most bytes of each part of a tool result (structured and text), measured
/// after escaping (§ 6).
pub const MAX_MCP_PART_BYTES: usize = 24 * 1024;

/// Most characters of a name (branch, worktree, agent, ref) in a response.
pub const MAX_MCP_NAME_CHARS: usize = 100;

/// Most bytes of a path, or of any untrusted text, in a response.
pub const MAX_MCP_PATH_BYTES: usize = 1024;

/// Reads per minute of one `mcp` connection, and their burst (SEC-08).
pub const MCP_READS_PER_MINUTE: u32 = 120;
pub const MCP_READ_BURST: u32 = 30;

/// Seconds a caller over the read limit waits for its next call: the time
/// one call takes to come back.
pub const MCP_RETRY_AFTER_S: u64 = 60u64.div_ceil(MCP_READS_PER_MINUTE as u64);

/// Most time of a read, the start of the engine included (BR-MCP-TIME-001).
pub const MCP_READ_TIME_LIMIT: Duration = Duration::from_secs(10);

/// Why a tool call gives no data: stable codes, in English. Adding one is
/// compatible; removing or changing one is a major change of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpToolError {
    /// The repo is observed but the developer did not enable it.
    RepoNotEnabled,
    /// The session's folder is in no observed worktree.
    NotInObservedWorktree,
    /// The repo is enabled but GitRaptor cannot read it now.
    RepoUnavailable,
    /// GitRaptor is not running and could not be started.
    EngineUnavailable,
    /// The engine could not verify who is calling.
    IdentityUnverified,
    /// Too many calls on this connection.
    RateLimited,
    /// The engine did not answer in time.
    TimeLimit,
    /// The answer did not fit its budget; nothing of it is sent.
    ResultTooLarge,
    Internal,
}

impl McpToolError {
    pub const ALL: [Self; 9] = [
        Self::RepoNotEnabled,
        Self::NotInObservedWorktree,
        Self::RepoUnavailable,
        Self::EngineUnavailable,
        Self::IdentityUnverified,
        Self::RateLimited,
        Self::TimeLimit,
        Self::ResultTooLarge,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepoNotEnabled => "repo-not-enabled",
            Self::NotInObservedWorktree => "not-in-observed-worktree",
            Self::RepoUnavailable => "repo-unavailable",
            Self::EngineUnavailable => "engine-unavailable",
            Self::IdentityUnverified => "identity-unverified",
            Self::RateLimited => "rate-limited",
            Self::TimeLimit => "time-limit",
            Self::ResultTooLarge => "result-too-large",
            Self::Internal => "internal",
        }
    }
}

/// Bytes of compact JSON per estimated token (RES-MCP-01 to 03): a
/// conservative ratio for JSON, identifiers and Spanish text, without a
/// tokenizer. ⚠️ ASSUMPTION: not measured against Claude's tokenizer; hex
/// runs about 2 bytes per token.
pub const MCP_BYTES_PER_TOKEN: usize = 3;

/// Most estimated tokens of the base of the catalog: the `initialize`
/// result, `instructions` included (RES-MCP-01).
pub const MCP_BASE_TOKENS: usize = 250;

/// Most estimated tokens of one tool as the model reads it: name,
/// description and `inputSchema` (RES-MCP-01).
pub const MCP_TOOL_TOKENS: usize = 150;

/// Most estimated tokens of one tool's compact `outputSchema` (RES-MCP-01).
pub const MCP_OUTPUT_SCHEMA_TOKENS: usize = 400;

/// Most estimated tokens of the base and every tool together, output
/// schemas apart (RES-MCP-01).
pub const MCP_CATALOG_TOKENS: usize = 1500;

/// Most estimated tokens of each part of a `status` result (RES-MCP-02).
pub const MCP_STATUS_TOKENS: usize = 300;

/// Most estimated tokens of a refusal, in en and es (RES-MCP-03).
pub const MCP_REFUSAL_TOKENS: usize = 80;

/// The estimated tokens of `text`: its UTF-8 bytes over
/// [`MCP_BYTES_PER_TOKEN`], rounded up.
pub const fn estimated_tokens(text: &str) -> usize {
    text.len().div_ceil(MCP_BYTES_PER_TOKEN)
}

/// Whether `text`, named `what`, fits `budget` estimated tokens under
/// `rule`; when it does not, says by how much, in tokens and bytes.
pub fn check_token_budget(rule: &str, what: &str, text: &str, budget: usize) -> Result<(), String> {
    let tokens = estimated_tokens(text);
    if tokens <= budget {
        return Ok(());
    }
    let limit = budget * MCP_BYTES_PER_TOKEN;
    Err(format!(
        "{rule}: {what} is ~{tokens} tokens ({} B), {} tokens ({} B) over its budget of {budget} tokens ({limit} B)",
        text.len(),
        tokens - budget,
        text.len() - limit,
    ))
}

/// Every way the catalog exceeds RES-MCP-01, given the `initialize` result
/// and the tools of `tools/list` as they go on the wire: the base, each tool
/// as the model reads it (name, description, `inputSchema`), each
/// `outputSchema` and the total. Empty when it fits.
pub fn catalog_overruns(initialize: &Value, tools: &[Value]) -> Vec<String> {
    const RULE: &str = "RES-MCP-01";
    let base = initialize.to_string();
    let mut overruns = Vec::new();
    let mut total = base.clone();
    overruns
        .extend(check_token_budget(RULE, "the initialize result", &base, MCP_BASE_TOKENS).err());
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or("?");
        let mut read = tool.clone();
        let output = read
            .as_object_mut()
            .and_then(|t| t.remove("outputSchema"))
            .map(|o| o.to_string());
        let read = read.to_string();
        let what = format!("tool `{name}` (name, description, inputSchema)");
        overruns.extend(check_token_budget(RULE, &what, &read, MCP_TOOL_TOKENS).err());
        if let Some(output) = output {
            let what = format!("the outputSchema of `{name}`");
            overruns
                .extend(check_token_budget(RULE, &what, &output, MCP_OUTPUT_SCHEMA_TOKENS).err());
        }
        total.push_str(&read);
    }
    overruns
        .extend(check_token_budget(RULE, "the whole catalog", &total, MCP_CATALOG_TOKENS).err());
    overruns
}

/// The `status` tool's result (US-MCP-003, RES-MCP-02): the field allowlist
/// of `mcp.status` without what the model never needs. No `repo_id` (no
/// tool takes a repo: it is the session's) nor `repo_state` (a repo that
/// cannot be read is refused), and `main` only when true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpStatusView {
    pub worktree: UntrustedName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<UntrustedName>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub main: bool,
    pub requester: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<McpStatusAction>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl From<&McpStatus> for McpStatusView {
    /// Names cut at their MCP bound; escaping is [`for_mcp`].
    fn from(status: &McpStatus) -> Self {
        let status = status.for_mcp();
        Self {
            worktree: status.worktree,
            branch: status.branch,
            main: status.main,
            requester: status.requester,
            action: status.action,
        }
    }
}

/// The one alternative of an `anyOf`/`oneOf` that is not `{"type":"null"}`,
/// when there is only one.
fn single_alternative(alternatives: Option<&Value>) -> Option<Value> {
    let Some(Value::Array(items)) = alternatives else {
        return None;
    };
    let mut rest = items
        .iter()
        .filter(|s| **s != serde_json::json!({"type": "null"}));
    match (rest.next(), rest.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    }
}

/// JSON Schema formats a client may validate; the rest (`uint32`) only cost
/// bytes.
const STANDARD_FORMATS: [&str; 6] = ["date-time", "date", "time", "duration", "uri", "uuid"];

/// A generated JSON Schema as the catalog sends it (RES-MCP-04): without
/// `$schema`, `title`, `description` and non-standard `format` keywords,
/// which only cost tokens. Walks schema positions only: a property named
/// `title` stays, and `enum`, `const`, `required`, `maxLength` and
/// `additionalProperties` are untouched.
///
/// An `anyOf` of a schema and `null` becomes the schema, and a `oneOf` of
/// one schema, that schema: MCP results carry no nulls (an absent field is
/// left out, RES-MCP-04), so the schema only gets stricter.
pub fn compact_schema(schema: &mut Value) {
    if let Value::Object(map) = schema {
        map.remove("$schema");
    }
    strip_annotations(schema);
}

fn strip_annotations(schema: &mut Value) {
    let Value::Object(map) = schema else {
        return;
    };
    for key in ["anyOf", "oneOf"] {
        if let Some(only) = single_alternative(map.get(key)) {
            map.remove(key);
            if let Value::Object(only) = only {
                map.extend(only);
            }
        }
    }
    map.remove("title");
    map.remove("description");
    if map
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|f| !STANDARD_FORMATS.contains(&f))
    {
        map.remove("format");
    }
    for key in ["properties", "$defs"] {
        if let Some(Value::Object(named)) = map.get_mut(key) {
            named.values_mut().for_each(strip_annotations);
        }
    }
    for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(Value::Array(items)) = map.get_mut(key) {
            items.iter_mut().for_each(strip_annotations);
        }
    }
    for key in ["items", "additionalProperties", "not"] {
        if let Some(sub) = map.get_mut(key) {
            strip_annotations(sub);
        }
    }
}

/// Rewrites every `{"untrusted": …}` object of a tool result: escape
/// sequences out, any other control, bidi, zero-width or Tags character
/// replaced ([`sanitize`]), the userinfo, query and fragment of URLs out,
/// and the text cut at [`MAX_MCP_PATH_BYTES`] with `truncated`.
pub fn for_mcp(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(text)) = map.get_mut("untrusted") {
                let mut clean = strip_url_secrets(&sanitize(text));
                let cut = cut_at(&mut clean, MAX_MCP_PATH_BYTES);
                *text = clean;
                if cut {
                    map.insert("truncated".into(), true.into());
                }
            }
            map.iter_mut()
                .filter(|(key, _)| *key != "untrusted")
                .for_each(|(_, v)| for_mcp(v));
        }
        Value::Array(items) => items.iter_mut().for_each(for_mcp),
        _ => {}
    }
}

/// Removes the userinfo (`user:token@`), the query (`?access_token=…`) and
/// the fragment of every `scheme://` URL in `text`: tokens travel in all
/// three (SEC-05).
fn strip_url_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("://") {
        out.push_str(&rest[..start + 3]);
        let after = &rest[start + 3..];
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        let (url, tail) = after.split_at(end);
        // Up to the last `@`, even past a `/`: a password with an unencoded
        // `/` must not leave its tail behind (fails closed on an `@` in a
        // path).
        let head = &url[..url.find(['?', '#']).unwrap_or(url.len())];
        out.push_str(head.rsplit_once('@').map_or(head, |(_, host)| host));
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn cut_at(text: &mut String, max: usize) -> bool {
    if text.len() <= max {
        return false;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn escaped(mut value: Value) -> Value {
        for_mcp(&mut value);
        value
    }

    #[test]
    fn every_untrusted_text_is_escaped_wherever_it_is() {
        let result = escaped(json!({
            "worktree": {"untrusted": "shop-\u{1b}]52;c;cHduZWQ=\u{7}feat"},
            "requester": {"actor": "agent", "name": {"untrusted": "a\u{202e}b", "lossy": true}},
            "list": [{"path": {"untrusted": "x\u{9b}2Jy"}}],
            "plain": "not\u{1b}marked"
        }));
        assert_eq!(result["worktree"], json!({"untrusted": "shop-feat"}));
        assert_eq!(
            result["requester"]["name"],
            json!({"untrusted": "a\u{FFFD}b", "lossy": true})
        );
        assert_eq!(result["list"][0]["path"]["untrusted"], "xy");
        // Only marked text is data from the repo; the rest is the binary's.
        assert_eq!(result["plain"], "not\u{1b}marked");
    }

    #[test]
    fn zero_width_and_tags_characters_cannot_hide_text() {
        let text = "ok\u{200b}\u{2060}\u{feff}\u{e0049}\u{e0067}";
        let result = escaped(json!({"untrusted": text}));
        assert_eq!(
            result["untrusted"],
            "ok\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
    }

    #[test]
    fn urls_lose_their_userinfo_query_and_fragment() {
        let text = "see https://user:ghp_tok@example.com/o/r.git?private_token=glpat#frag \
                    and http://host/p?access_token=abc done";
        let result = escaped(json!({"untrusted": text}));
        assert_eq!(
            result["untrusted"],
            "see https://example.com/o/r.git and http://host/p done"
        );
        assert_eq!(strip_url_secrets("https://tok@h?x=1"), "https://h");
        assert_eq!(strip_url_secrets("feat/login"), "feat/login");
        assert_eq!(strip_url_secrets("https://u:p/ss@h/r"), "https://h/r");
    }

    #[test]
    fn untrusted_text_is_cut_at_the_path_bound() {
        let result = escaped(json!({"untrusted": "é".repeat(MAX_MCP_PATH_BYTES)}));
        let text = result["untrusted"].as_str().unwrap();
        assert!(text.len() <= MAX_MCP_PATH_BYTES);
        assert_eq!(result["truncated"], true);
        assert!(
            escaped(json!({"untrusted": "short"}))
                .get("truncated")
                .is_none()
        );
    }

    #[test]
    fn error_codes_are_stable_and_unique() {
        let mut codes: Vec<_> = McpToolError::ALL.iter().map(|c| c.as_str()).collect();
        for code in McpToolError::ALL {
            assert_eq!(serde_json::to_value(code).unwrap(), code.as_str());
        }
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), McpToolError::ALL.len());
        assert_eq!(MCP_RETRY_AFTER_S, 1);
    }

    #[test]
    fn a_text_over_its_token_budget_says_by_how_much() {
        assert_eq!(estimated_tokens(""), 0);
        assert_eq!(estimated_tokens("abcd"), 2);
        assert_eq!(estimated_tokens("é"), 1);
        assert!(check_token_budget("RES-MCP-03", "refusal", &"x".repeat(240), 80).is_ok());
        let inflated = "x".repeat(250);
        let message = check_token_budget("RES-MCP-03", "refusal", &inflated, 80).unwrap_err();
        assert_eq!(
            message,
            "RES-MCP-03: refusal is ~84 tokens (250 B), 4 tokens (10 B) over its budget of 80 tokens (240 B)"
        );
    }

    #[test]
    fn a_compact_schema_keeps_what_validates() {
        let mut schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "T",
            "description": "internal BR-X",
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "d", "maxLength": 100},
                "n": {"type": "integer", "format": "uint32"},
                "at": {"type": "string", "format": "date-time"},
                "kind": {"oneOf": [{"const": "a", "description": "d"}]},
                "maybe": {"anyOf": [{"$ref": "#/$defs/U"}, {"type": "null"}], "description": "d"},
                "either": {"anyOf": [{"type": "string"}, {"type": "integer"}]},
                "list": {"type": "array", "items": {"$ref": "#/$defs/U", "title": "x"}}
            },
            "$defs": {"U": {"description": "d", "enum": ["description"]}},
            "required": ["title"],
            "additionalProperties": false
        });
        compact_schema(&mut schema);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "maxLength": 100},
                    "n": {"type": "integer"},
                    "at": {"type": "string", "format": "date-time"},
                    "kind": {"const": "a"},
                    "maybe": {"$ref": "#/$defs/U"},
                    "either": {"anyOf": [{"type": "string"}, {"type": "integer"}]},
                    "list": {"type": "array", "items": {"$ref": "#/$defs/U"}}
                },
                "$defs": {"U": {"enum": ["description"]}},
                "required": ["title"],
                "additionalProperties": false
            })
        );
    }

    /// RES-MCP-01: each part of the catalog over its budget says so.
    #[test]
    fn an_inflated_catalog_fails_by_part() {
        let tool = json!({"name": "status", "description": "short",
                          "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"}});
        let init = json!({"instructions": "short"});
        assert!(catalog_overruns(&init, std::slice::from_ref(&tool)).is_empty());

        let mut fat = tool.clone();
        fat["description"] = json!("x".repeat(MCP_TOOL_TOKENS * MCP_BYTES_PER_TOKEN));
        fat["outputSchema"]["description"] = json!("x".repeat(1200));
        let fat_init = json!({"instructions": "x".repeat(MCP_BASE_TOKENS * MCP_BYTES_PER_TOKEN)});
        let overruns = catalog_overruns(&fat_init, &[fat]);
        assert_eq!(overruns.len(), 3, "{overruns:#?}");
        assert!(overruns[0].starts_with("RES-MCP-01: the initialize result is"));
        assert!(
            overruns[1].starts_with("RES-MCP-01: tool `status` (name, description, inputSchema)")
        );
        assert!(overruns[2].starts_with("RES-MCP-01: the outputSchema of `status`"));
        assert!(overruns.iter().all(|o| o.contains(" over its budget of ")));

        // Each tool within its own budget, too many of them together.
        let mut near = tool;
        near["description"] = json!("x".repeat(MCP_TOOL_TOKENS * MCP_BYTES_PER_TOKEN - 100));
        let many = vec![near; 12];
        let overruns = catalog_overruns(&init, &many);
        assert_eq!(overruns.len(), 1, "{overruns:#?}");
        assert!(overruns[0].starts_with("RES-MCP-01: the whole catalog"));
    }

    fn status(main: bool) -> McpStatus {
        McpStatus {
            repo_id: "f".repeat(64),
            repo_state: crate::messages::RepoStateView::Observed,
            worktree: UntrustedName::new("shop-feat-a"),
            branch: Some(UntrustedName::new("feat-a")),
            main,
            requester: Actor::Unattributed,
            action: Some(McpStatusAction::RegisterToWrite),
        }
    }

    /// SEC-12 and RES-MCP-02: the tool's view is the field allowlist, with
    /// `main` only when true.
    #[test]
    fn the_status_view_is_its_field_allowlist() {
        let view = serde_json::to_value(McpStatusView::from(&status(false))).unwrap();
        let mut keys: Vec<_> = view.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["action", "branch", "requester", "worktree"]);
        assert_eq!(
            serde_json::to_string(&McpStatusView::from(&status(false))).unwrap(),
            r#"{"worktree":{"untrusted":"shop-feat-a"},"branch":{"untrusted":"feat-a"},"requester":{"actor":"unattributed"},"action":"register-to-write"}"#
        );
        let main = serde_json::to_value(McpStatusView::from(&status(true))).unwrap();
        assert_eq!(main["main"], true);
        let long = McpStatus {
            worktree: UntrustedName::new("n".repeat(1024)),
            ..status(false)
        };
        let view = McpStatusView::from(&long);
        assert_eq!(view.worktree.raw().len(), MAX_MCP_NAME_CHARS);
        assert!(view.worktree.is_truncated());
    }
}
