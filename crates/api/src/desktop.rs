//! The desktop app's connection class (TS-GRP-009, ADR-GRP-017 § 3).
//!
//! The daemon deduces the class from the peer's executable; `hello` does not
//! change shape (ADR-GRP-016). A connection of the class reads why its
//! profile was fixed with `connection.desktop`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Why the daemon fixed this profile for a desktop-class connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopReason {
    /// Row 3: clean ancestry and the app's path identity: full.
    PathIdentity,
    /// Row 4: clean ancestry, no identity: full only with controls 1 to 3.
    NoIdentity,
    /// Row 1: an agent in the ancestry: mcp.
    AgentInAncestry,
    /// An interpreter, a multiplexer or a broken chain: mcp.
    AmbiguousAncestry,
    /// Marked by an executor operation or below the daemon: mcp.
    DaemonDescendant,
    /// The requester could not be resolved: mcp.
    RequesterUnverified,
    /// The executable could not be read: mcp.
    IdentityUnverifiable,
    /// Declared `mcp` or a `raptor-mcp` executable: mcp.
    DeclaredMcp,
}

impl DesktopReason {
    /// The wire word, also used by the daemon's log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PathIdentity => "path-identity",
            Self::NoIdentity => "no-identity",
            Self::AgentInAncestry => "agent-in-ancestry",
            Self::AmbiguousAncestry => "ambiguous-ancestry",
            Self::DaemonDescendant => "daemon-descendant",
            Self::RequesterUnverified => "requester-unverified",
            Self::IdentityUnverifiable => "identity-unverifiable",
            Self::DeclaredMcp => "declared-mcp",
        }
    }
}

/// What the daemon could prove about the peer's executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopIdentity {
    Path,
    None,
    Unverifiable,
}

impl DesktopIdentity {
    /// The wire word, also used by the daemon's log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::None => "none",
            Self::Unverifiable => "unverifiable",
        }
    }
}

/// `connection.desktop` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopResult {
    pub reason: DesktopReason,
    pub identity: DesktopIdentity,
}
