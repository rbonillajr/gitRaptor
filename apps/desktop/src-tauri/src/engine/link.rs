//! The seam between the app and the engine (DS-US-DSK-003 § Firmas del stack): a [`Connector`]
//! opens a [`Link`] to the daemon, and every failure is a [`ConnectFailure`] the supervisor can
//! show. The app talks to the engine only through this channel, like the TUI (ADR-GRP-017).

use std::path::PathBuf;
use std::time::Duration;

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::client::{Client, ClientError, Incoming};
use gitraptor_api::messages::{ClientKind, HelloResult};
use gitraptor_api::methods::{CAP_OBSERVATION_TIERS, CAP_PREDICTION_PAIRS, CAP_SCOPE_ACTIVITY};
use gitraptor_api::rpc::{Notification, ServerMessage};
use serde_json::Value;

/// The capabilities the app needs from the engine: `raptor daemon ensure` is asked for each.
pub const REQUIRED_CAPABILITIES: &[&str] = &[
    CAP_SCOPE_ACTIVITY.name,
    CAP_OBSERVATION_TIERS.name,
    CAP_PREDICTION_PAIRS.name,
];

/// A greeted connection to the daemon.
pub trait Link: Send {
    /// What the daemon said in the handshake.
    fn hello(&self) -> &HelloResult;
    /// Calls `method` and waits for its answer.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError>;
    /// The next notification with the monotonic reading of when it was read, waiting up to
    /// `timeout`; `None` when none arrived (an answer read meanwhile is dropped).
    fn next(&mut self, timeout: Duration) -> Result<Option<(u64, Notification)>, LinkError>;
}

/// Opens a [`Link`], starting the engine first when `start` is `true`.
pub trait Connector: Send + 'static {
    fn connect(&mut self, start: bool) -> Result<Box<dyn Link>, ConnectFailure>;
}

/// Why a call on an open [`Link`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// The daemon answered with an error.
    Refused { code: i64 },
    /// The connection is gone or no longer speaks the contract.
    Lost,
}

/// Why no [`Link`] could be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectFailure {
    /// No daemon runs (and none was to be started).
    NotRunning,
    /// The engine could not be started, or its `raptor` could not be run or understood.
    LaunchFailed,
    /// A daemon runs that the app cannot use.
    Incompatible(Incompatibility),
    /// The channel's folder or its server is not this user's, or the server is not an installed
    /// `raptor` (L-06, B-3).
    Rejected,
    /// No channel transport on this platform.
    Unsupported,
    /// Anything else on the way: the connection broke or did not speak the contract.
    Lost,
}

/// The daemon the app cannot use, as reported: raw, not sanitized (the supervisor sanitizes it
/// before showing it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incompatibility {
    pub which: IncompatibilityKind,
    pub daemon_version: Option<String>,
    pub daemon_path: Option<String>,
    /// The required capabilities the daemon does not serve.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncompatibilityKind {
    /// The daemon speaks an older protocol.
    DaemonOlder,
    /// The daemon speaks a newer protocol: the app must be updated.
    AppOlder,
    /// Same protocol, without a required capability.
    MissingCapabilities,
}

/// The [`ConnectFailure`] of a client error.
pub fn map_client_error(err: ClientError) -> ConnectFailure {
    let incompatible = |which, data: gitraptor_api::messages::IncompatibleData| {
        ConnectFailure::Incompatible(Incompatibility {
            which,
            daemon_version: Some(data.binary_version),
            daemon_path: None,
            missing: Vec::new(),
        })
    };
    match err {
        ClientError::NotRunning => ConnectFailure::NotRunning,
        ClientError::Incompatible(data) => incompatible(IncompatibilityKind::DaemonOlder, data),
        ClientError::ClientTooOld(data) => incompatible(IncompatibilityKind::AppOlder, data),
        ClientError::ChannelRejected | ClientError::NotAuthentic => ConnectFailure::Rejected,
        ClientError::TransportUnsupported => ConnectFailure::Unsupported,
        ClientError::Launch(_) | ClientError::StartTimeout => ConnectFailure::LaunchFailed,
        ClientError::Rpc(_)
        | ClientError::Io(_)
        | ClientError::Protocol(_)
        | ClientError::Unsupported(_) => ConnectFailure::Lost,
    }
}

/// A [`Link`] over a channel client.
pub struct ClientLink(pub Client);

impl Link for ClientLink {
    fn hello(&self) -> &HelloResult {
        self.0.hello()
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.0
            .call::<Value, Value>(method, params)
            .map_err(link_error)
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<(u64, Notification)>, LinkError> {
        match self.0.next_incoming(timeout).map_err(link_error)? {
            None => Ok(None),
            Some(Incoming::Buffered {
                recv_ns,
                notification,
            }) => Ok(Some((recv_ns, notification))),
            Some(Incoming::Frame { recv_ns, bytes }) => {
                match serde_json::from_slice::<ServerMessage>(&bytes) {
                    Ok(ServerMessage::Notification(notification)) => {
                        Ok(Some((recv_ns, notification)))
                    }
                    Ok(ServerMessage::Response(_)) => Ok(None),
                    Err(_) => Err(LinkError::Lost),
                }
            }
        }
    }
}

fn link_error(err: ClientError) -> LinkError {
    match err {
        ClientError::Rpc(err) => LinkError::Refused { code: err.code },
        _ => LinkError::Lost,
    }
}

/// A [`Connector`] to a runtime folder known beforehand. It never starts the engine: with
/// `start` it connects all the same.
///
/// It does **not** check B-3 (who serves the socket): it is for tests and fake daemons. The app
/// connects to the engine only through [`super::launcher::BundleConnector`], which does.
pub struct SocketConnector {
    pub runtime: PathBuf,
}

impl Connector for SocketConnector {
    fn connect(&mut self, _start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        Client::connect(&self.runtime, ClientKind::Cli, PROTOCOL_VERSION)
            .map(|client| Box::new(ClientLink(client)) as Box<dyn Link>)
            .map_err(map_client_error)
    }
}
