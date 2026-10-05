//! JSON-RPC and event contract shared by the engine, the CLI and the MCP server
//! (TS-GRP-004, ADR-GRP-005 § 5).
//!
//! The contract has three parts: a versioned handshake ([`messages::Hello`]),
//! queries and commands ([`methods`]) and an event stream by subscription
//! ([`event`]). Messages are JSON-RPC 2.0, one per line, bounded in size and
//! depth ([`framing`]); every type rejects unknown fields.
//!
//! Text that comes from a repo or an agent travels as [`Untrusted`] and is
//! sanitized before it reaches a terminal (SEC-12). The exposed actor has no
//! "human" variant ([`Actor`], ADR-GRP-013 § 6).

pub mod actor;
pub mod catalog;
pub mod clock;
pub mod event;
pub mod framing;
pub mod guard;
pub mod messages;
pub mod methods;
pub mod resources;
pub mod rpc;
pub mod scope;
pub mod timemachine;
pub mod untrusted;

pub use actor::{Actor, AgentKind, AgentOrigin};
pub use event::{Event, Timings};
pub use untrusted::{Untrusted, UntrustedName};

/// Version of the engine API contract (semantic, for humans).
pub const API_VERSION: &str = "7.0.0";

/// Wire protocol version negotiated in the handshake. A daemon serves every
/// client from [`MIN_COMPATIBLE_PROTOCOL`] up to its own version, each in
/// the shapes of the version it asked for; a client newer than the daemon
/// replaces it (ADR-GRP-005 § 4, SEC-13).
/// Version 3 (US-GRP-012): ahead/behind of each worktree (3.1: `git.event`
/// and `events.history`, US-GRP-002). Version 4
/// (TS-CKP-002): the two-phase catalog flow; `operation.run` executes a
/// prepared plan. Version 5 (US-GRP-007): agent sessions, `session.state`
/// and `sessions.list`. Version 6 (TS-GRP-004, ADR-CKP-003 § 4 N1 to N7):
/// scopes with their own contiguous sequence, `repo.locate`, the requester
/// in the handshake, bounded untrusted names and typed codes (6.1:
/// `engine.resources`, US-GRP-017, additive). Version 7 (US-GRD-001): the
/// `guard.*` methods and the `not-observed` repo rejection; a connection of
/// version 5 or 6 sees neither.
pub const PROTOCOL_VERSION: u32 = 7;

/// Oldest client protocol a daemon still serves: a long-lived `raptor-mcp`
/// survives an upgrade of the daemon (DS-TS-GRP-004 E-D1).
pub const MIN_COMPATIBLE_PROTOCOL: u32 = 5;

/// File name of the channel socket inside the profile's runtime folder.
pub const SOCKET_FILE: &str = "raptor.sock";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_is_set() {
        assert!(!API_VERSION.is_empty());
        const { assert!(PROTOCOL_VERSION >= 1) };
        const { assert!(MIN_COMPATIBLE_PROTOCOL <= PROTOCOL_VERSION) };
    }
}
