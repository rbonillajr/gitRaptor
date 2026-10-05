//! The actor the engine exposes (ADR-GRP-013 § 6, Q34).
//!
//! Only two variants: an agent with its origin, or "unattributed". There is
//! no "human" variant, so the engine cannot emit one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::UntrustedName;

/// Who an event is attributed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "actor", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Actor {
    Agent {
        kind: AgentKind,
        /// Declared name of an "other agent" (text from an agent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<UntrustedName>,
        origin: AgentOrigin,
    },
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AgentOrigin {
    Detected,
    Registered,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_form() {
        let actor = Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        };
        assert_eq!(
            serde_json::to_string(&actor).unwrap(),
            r#"{"actor":"agent","kind":"claude-code","origin":"detected"}"#
        );
        assert_eq!(
            serde_json::to_string(&Actor::Unattributed).unwrap(),
            r#"{"actor":"unattributed"}"#
        );
    }

    /// Q34: the schema has no "human" variant and the decoder refuses one.
    #[test]
    fn schema_has_no_human_variant() {
        let schema = serde_json::to_string(&schemars::schema_for!(Actor)).unwrap();
        assert!(!schema.to_lowercase().contains("human"), "{schema}");
        assert!(schema.contains("unattributed"));
        assert!(serde_json::from_str::<Actor>(r#"{"actor":"human"}"#).is_err());
    }
}
