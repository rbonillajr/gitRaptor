//! Capabilities: what a connection understands beyond the methods it may
//! call (ADR-GRP-016 § 1).
//!
//! A new method needs no capability: a client finds it in the `methods` of
//! the handshake. A capability is a change of *shape* (a new event kind, a
//! new field, a new code from an existing method): the daemon serves it only
//! to a connection that said it understands it. Each contract module
//! declares its own capabilities in its file under [`crate::methods`]; none
//! of them bumps a shared counter.
//!
//! Before protocol 9 the shapes came with the protocol number: the
//! capabilities of those versions carry the protocol that brought them
//! ([`Capability::legacy`]) and a connection of that protocol, or of 9, has
//! them. A capability added after 9 has no legacy protocol and is granted
//! only through `connection.accept`.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The protocol that froze the counter: from it on, additive features are
/// capabilities and methods, never a new protocol number.
pub const CAPABILITIES_PROTOCOL: u32 = 9;

/// Longest list `connection.accept` takes.
pub const MAX_ACCEPTED: usize = 64;

/// Longest capability name.
pub const MAX_NAME_LEN: usize = 64;

/// One change of shape a connection may understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// `<module>.<feature>`, unique in the contract.
    pub name: &'static str,
    /// The protocol (5 to 8) that brought it before capabilities existed: a
    /// connection of that protocol or newer has it without asking. `None`:
    /// only granted through `connection.accept`.
    pub legacy: Option<u32>,
}

impl Capability {
    /// A capability added after protocol 9.
    pub const fn new(name: &'static str) -> Self {
        Self { name, legacy: None }
    }

    /// A capability that a protocol before 9 brought with its number.
    pub const fn legacy(name: &'static str, protocol: u32) -> Self {
        Self {
            name,
            legacy: Some(protocol),
        }
    }
}

/// Every capability of the contract, from every module.
pub fn all() -> impl Iterator<Item = &'static Capability> {
    crate::methods::GROUPS
        .iter()
        .flat_map(|g| g.capabilities.iter())
}

/// The capabilities a connection of `protocol` has before it accepts any:
/// those its protocol brought (all of them before 9 for a connection of 9).
pub fn implied(protocol: u32) -> BTreeSet<&'static str> {
    all()
        .filter(|c| c.legacy.is_some_and(|since| protocol >= since))
        .map(|c| c.name)
        .collect()
}

/// `connection.accept` parameters (protocol 9): the capabilities the client
/// understands. Unknown names are ignored, so a newer client can talk to an
/// older daemon.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptParams {
    pub capabilities: Vec<String>,
}

/// `connection.accept` result: every capability the connection has now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptResult {
    pub capabilities: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_names_are_unique_and_bounded() {
        let mut names: Vec<_> = all().map(|c| c.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "a capability is declared twice");
        assert!(names.iter().all(|n| n.len() <= MAX_NAME_LEN));
    }

    /// Only protocols 5 to 8 brought capabilities with their number.
    #[test]
    fn legacy_capabilities_belong_to_frozen_protocols() {
        for c in all() {
            if let Some(p) = c.legacy {
                assert!(
                    (crate::MIN_COMPATIBLE_PROTOCOL..CAPABILITIES_PROTOCOL).contains(&p),
                    "{}",
                    c.name
                );
            }
        }
        // A connection of 9 has every legacy capability.
        let legacy: BTreeSet<_> = all()
            .filter(|c| c.legacy.is_some())
            .map(|c| c.name)
            .collect();
        assert_eq!(implied(CAPABILITIES_PROTOCOL), legacy);
    }
}
