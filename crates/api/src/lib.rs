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
pub mod capability;
pub mod catalog;
pub mod clock;
pub mod discovery;
pub mod event;
pub mod framing;
pub mod guard;
pub mod mcp_view;
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

/// Version of the engine API contract (semantic, for humans). Frozen with
/// the protocol at 9 (ADR-GRP-016 § 1): what a connection has is told by
/// its methods and capabilities, not by this text.
pub const API_VERSION: &str = "9.0.0";

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
/// `engine.resources`, US-GRP-017, additive; 6.2: `registration.register`
/// and `registration.withdraw`, US-GRP-009, additive). Version 7
/// (US-GRD-001): the `guard.*` methods, the `-32016` code and the
/// `not-observed` repo rejection; a connection of version 5 or 6 sees none.
/// Version 8 (US-TMC-004): Git events of kind `reset`; a connection of
/// version 5 to 7 never receives them. Version 9 (ADR-GRP-016): the last
/// one for additive changes. The handshake announces the daemon's
/// capabilities and `connection.accept` takes the client's; a new method or
/// a new shape is declared by its module ([`methods`], [`capability`]) and
/// this number does not change again unless something is removed.
pub const PROTOCOL_VERSION: u32 = 9;

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
