//! The versioned table of the agents' trailer identities (DS-US-GRD-018 § 3, D8). One row per
//! agent; the adapter of each agent owns its row (ADR-GRP-012 § 1). A new row changes
//! [`TABLE_VERSION`], which travels in the decision's `configRef`.

use gitraptor_api::AgentKind;
use unicode_normalization::UnicodeNormalization;

/// Version of [`AGENT_TRAILERS`].
pub const TABLE_VERSION: u32 = 1;

/// How one agent signs its commits as a co-author.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTrailer {
    pub agent: AgentKind,
    /// Exact match, ASCII without case.
    pub emails: &'static [&'static str],
    /// Prefix of the name after NFC and case folding.
    pub name_prefixes: &'static [&'static str],
    /// The example trailer of the deny message.
    pub example: &'static str,
}

/// Today only Claude Code: any tool built on Anthropic's SDK that signs with this email counts
/// as Claude Code (D8). Codex and Cursor add their rows with their adapters (D2 of the BRD).
pub const AGENT_TRAILERS: &[AgentTrailer] = &[AgentTrailer {
    agent: AgentKind::ClaudeCode,
    emails: &["noreply@anthropic.com"],
    name_prefixes: &["claude"],
    example: "Co-Authored-By: Claude <noreply@anthropic.com>",
}];

/// The agent a co-author names: the email matches **and** the name starts with the prefix.
pub fn recognise(name: &str, email: &str) -> Option<AgentKind> {
    let email = email.trim().to_ascii_lowercase();
    let name: String = name.trim().nfc().collect::<String>().to_lowercase();
    AGENT_TRAILERS
        .iter()
        .find(|row| {
            row.emails.iter().any(|e| *e == email)
                && row.name_prefixes.iter().any(|p| name.starts_with(p))
        })
        .map(|row| row.agent)
}

/// The row of `agent`, for the example of the deny message.
pub fn row(agent: AgentKind) -> Option<&'static AgentTrailer> {
    AGENT_TRAILERS.iter().find(|r| r.agent == agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_is_recognised_by_email_and_name() {
        for name in ["Claude", "Claude Opus 4.7", "claude sonnet 5.5", " CLAUDE "] {
            assert_eq!(
                recognise(name, "noreply@anthropic.com"),
                Some(AgentKind::ClaudeCode),
                "{name}"
            );
        }
        assert_eq!(
            recognise("Claude", "NoReply@Anthropic.com"),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn lookalikes_are_not_recognised() {
        assert_eq!(recognise("Claudia", "claudia@x.com"), None);
        assert_eq!(recognise("Ana", "noreply@anthropic.com"), None);
        assert_eq!(recognise("Claude", "noreply@anthropic.com.evil"), None);
        assert_eq!(recognise("Claude", ""), None);
    }

    #[test]
    fn every_row_recognises_its_own_example() {
        for row in AGENT_TRAILERS {
            let value = row.example.split_once(": ").unwrap().1;
            let (name, email) = value.split_once(" <").unwrap();
            assert_eq!(
                recognise(name, email.trim_end_matches('>')),
                Some(row.agent)
            );
        }
    }
}
