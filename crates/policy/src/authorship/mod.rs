//! Commit authorship (US-GRD-018, DS-US-GRD-018 § 3): the versioned table of the agents'
//! trailer identities and the `Co-Authored-By` parser. Pure, no I/O: the hook client runs it on
//! the message and sends the daemon only what it derived (D7), never the text, names or emails.

pub mod agents;
pub mod trailers;

use gitraptor_api::AgentKind;
use gitraptor_api::guard::AuthorshipFacts;

pub use agents::{AGENT_TRAILERS, AgentTrailer, TABLE_VERSION, recognise};
pub use trailers::{Cleanup, MAX_COAUTHORS, MAX_MESSAGE_BYTES, MessageOptions, coauthors};

/// The facts of one commit message: one entry per `Co-Authored-By`, with the agent it names
/// when the table recognises it.
pub fn facts(message: &[u8], options: &MessageOptions) -> AuthorshipFacts {
    if message.len() > MAX_MESSAGE_BYTES {
        return unreadable();
    }
    let text = String::from_utf8_lossy(message);
    AuthorshipFacts {
        coauthors: coauthors(&text, options)
            .iter()
            .map(|c| recognise(&c.name, &c.email))
            .collect::<Vec<Option<AgentKind>>>(),
        trailer_table: TABLE_VERSION,
        unreadable: false,
    }
}

/// The facts of a message that could not be read (missing, too large, not a regular file).
pub fn unreadable() -> AuthorshipFacts {
    AuthorshipFacts {
        coauthors: Vec::new(),
        trailer_table: TABLE_VERSION,
        unreadable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_carry_kinds_only() {
        let msg = b"feat: x\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\nCo-Authored-By: Ana <ana@x.com>\n";
        let f = facts(msg, &MessageOptions::default());
        assert_eq!(f.coauthors, vec![Some(AgentKind::ClaudeCode), None]);
        assert_eq!(f.trailer_table, TABLE_VERSION);
        assert!(!f.unreadable);
        // Nothing of the text travels.
        let wire = serde_json::to_string(&f).unwrap();
        assert!(
            !wire.contains("anthropic") && !wire.contains("Ana"),
            "{wire}"
        );
    }

    #[test]
    fn a_message_too_large_is_unreadable() {
        let big = vec![b'a'; MAX_MESSAGE_BYTES + 1];
        let f = facts(&big, &MessageOptions::default());
        assert!(f.unreadable && f.coauthors.is_empty());
    }
}
