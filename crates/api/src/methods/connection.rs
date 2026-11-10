//! The connection itself: handshake, liveness and capabilities.

use super::{Group, method};
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};

/// Handshake; must be the first message of every connection.
pub const HELLO: &str = "hello";
/// Liveness check.
pub const PING: &str = "ping";
/// The capabilities the client understands (protocol 9, ADR-GRP-016 § 1).
/// Before the first subscription; the daemon answers with what the
/// connection has.
pub const CONNECTION_ACCEPT: &str = "connection.accept";

/// Why the daemon fixed the profile of a desktop-class connection
/// (protocol 9, TS-GRP-009). Offered only to that class.
pub const CONNECTION_DESKTOP: &str = "connection.desktop";

/// The requester in the handshake (protocol 6, ADR-CKP-003 § 4 N5).
pub const CAP_REQUESTER: Capability = Capability::legacy("connection.requester", 6);

pub(super) const GROUP: Group = Group {
    methods: &[
        method(HELLO, false, true),
        method(PING, false, true),
        // Every profile: it only changes shapes, never what a connection may do.
        method(CONNECTION_ACCEPT, false, true).since(CAPABILITIES_PROTOCOL),
        // Open to `mcp` so a narrowed desktop connection can read why; the
        // daemon offers it only to that class.
        method(CONNECTION_DESKTOP, false, true).since(CAPABILITIES_PROTOCOL),
    ],
    capabilities: &[CAP_REQUESTER],
    ..Group::new("connection")
};
