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

use serde::Serialize;
use serde_json::Value;

use crate::untrusted::sanitize;

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
}
