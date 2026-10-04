//! The local client channel of the daemon (TS-GRP-004, ADR-GRP-005 § 5 and
//! § 6).
//!
//! A Unix socket only the user can open ([`transport`]); per connection, a
//! versioned JSON-RPC handshake, queries served from memory, an event
//! stream by subscription ([`bus`]) and reserved commands authorized only
//! by the daemon ([`authz`]) and audited. Limits and a rate limit keep one
//! client from starving the others (SEC-08).

pub mod authz;
pub mod bus;
#[cfg(unix)]
mod conn;
pub mod peer;
#[cfg(unix)]
mod server;
pub mod transport;
pub mod validate;

use std::path::PathBuf;
use std::time::Duration;

use gitraptor_api::PROTOCOL_VERSION;

pub use authz::{AgentMatcher, ExeClass};
pub use bus::{EngineShared, EventBus};
#[cfg(unix)]
pub use server::{BoundChannel, Server};
#[cfg(unix)]
pub(crate) use server::{ServeArgs, ServerCtx, file_id};

/// Why the channel cannot run on this platform yet. Windows needs a named
/// pipe with a DACL for the user's SID, first-instance creation, remote
/// clients refused and SQOS identification in the client (ADR-GRP-005 § 5).
/// Until then there is no channel at all: never one without access control
/// (fail-closed). Pendiente: etapa de validación multiplataforma.
pub const TRANSPORT_UNSUPPORTED: &str = "the local channel is not supported on Windows yet (named pipe with an access-control list pending)";

/// Limits of the channel (SEC-08, SEC-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLimits {
    pub max_connections: usize,
    /// Extra slots only for a client that passes the reserved checks (the
    /// developer), so a flood of agent connections cannot lock it out.
    pub terminal_slots: usize,
    /// Connections per client process `(pid, start)`.
    pub per_client: usize,
    pub subscriptions_per_connection: usize,
    /// Messages queued per connection before it counts as slow.
    pub outbox: usize,
    /// Requests per second per connection, and burst.
    pub rate_per_sec: u32,
    pub burst: u32,
    /// The whole handshake, from accept to a valid `hello`.
    pub handshake_timeout: Duration,
    /// A connection without subscriptions that sends nothing for this long
    /// is closed.
    pub idle_timeout: Duration,
    pub write_timeout: Duration,
    /// Events kept for `events.subscribe { from_seq }`.
    pub replay: usize,
}

impl Default for ChannelLimits {
    fn default() -> Self {
        Self {
            max_connections: 32,
            terminal_slots: 2,
            per_client: 8,
            subscriptions_per_connection: 4,
            outbox: 1024,
            rate_per_sec: 100,
            burst: 200,
            handshake_timeout: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(60),
            write_timeout: Duration::from_secs(2),
            replay: 1024,
        }
    }
}

/// Inputs of the channel. Tests inject another uid, agent names, the
/// launch path of the "installed" binary and the protocol version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelConfig {
    pub limits: ChannelLimits,
    pub agents: AgentMatcher,
    /// The only uid allowed to connect. `None`: this process's euid.
    pub expected_uid: Option<u32>,
    /// Path the daemon was launched from. `None`: `current_exe()`.
    pub launch_exe: Option<PathBuf>,
    pub protocol: u32,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            limits: ChannelLimits::default(),
            agents: AgentMatcher::default(),
            expected_uid: None,
            launch_exe: None,
            protocol: PROTOCOL_VERSION,
        }
    }
}
