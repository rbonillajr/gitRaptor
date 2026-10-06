//! The daemon's life (ADR-GRP-005 § 4).

use super::{Group, method};

/// Orderly stop of the daemon (reserved, SEC-13).
pub const DAEMON_STOP: &str = "daemon.stop";
/// Stop requested by a newer installed binary to replace an older daemon.
pub const DAEMON_REPLACE: &str = "daemon.replace";

pub(super) const GROUP: Group = Group {
    methods: &[
        method(DAEMON_STOP, true, false),
        // Not reserved when the caller is the installed binary (`raptor` or
        // `raptor-mcp`); from anything else the daemon treats it as
        // `daemon.stop` (SEC-13). Its shape is frozen across protocol versions,
        // like `hello`'s, because it is how versions meet.
        method(DAEMON_REPLACE, false, true),
    ],
    ..Group::new("daemon")
};
