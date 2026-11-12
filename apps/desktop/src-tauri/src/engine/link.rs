//! The seam between the app and the engine (DS-US-DSK-003 § Firmas del stack): a [`Connector`]
//! opens a [`Link`] to the daemon, and every failure is a [`ConnectFailure`] the supervisor can
//! show. The app talks to the engine only through this channel, like the TUI (ADR-GRP-017).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::client::{Client, ClientError, Incoming};
use gitraptor_api::messages::{ClientKind, HelloResult};
use gitraptor_api::methods::{
    CAP_OBSERVATION_TIERS, CAP_PREDICTION_PAIRS, CAP_REQUESTER, CAP_SCOPE_ACTIVITY, QUERY_CANCEL,
};
use gitraptor_api::rpc::{self, Id, Notification, Response, ServerMessage};
use serde_json::{Value, json};

/// The capabilities the app needs from the engine: `raptor daemon ensure` is asked for each.
pub const REQUIRED_CAPABILITIES: &[&str] = &[
    CAP_SCOPE_ACTIVITY.name,
    CAP_OBSERVATION_TIERS.name,
    CAP_PREDICTION_PAIRS.name,
    CAP_REQUESTER.name,
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
    /// The daemon's own executable, when the link knows it.
    fn daemon_exe(&self) -> Option<&Path> {
        None
    }
    /// Sends a request without waiting for its answer and returns its id; the answer is taken
    /// with [`Link::reply`]. A link that cannot do it refuses with `METHOD_NOT_FOUND`.
    fn submit(&mut self, _method: &str, _params: Value) -> Result<u64, LinkError> {
        Err(LinkError::Refused {
            code: rpc::code::METHOD_NOT_FOUND,
        })
    }
    /// Forgets a pending id whose answer the caller decoded itself.
    fn settle(&mut self, _id: u64) {}
    /// The answer to a request of [`Link::submit`] (or the one cancelled with [`Link::cancel`]),
    /// once it was read; `None` while it is pending.
    fn reply(&mut self, _id: u64) -> Option<Result<Value, LinkError>> {
        None
    }
    /// Asks the daemon to cancel the request `id` without waiting for the answer. The answer of
    /// `id` is still kept for [`Link::reply`]: the daemon frees its slot only when it arrives.
    fn cancel(&mut self, _id: u64) {}
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

/// Most answers of [`Link::submit`] kept for [`Link::reply`] at once.
const MAX_KEPT_REPLIES: usize = 8;

/// A [`Link`] over a channel client.
pub struct ClientLink {
    client: Client,
    /// Ids of the requests whose answer the caller will ask for; any other answer is dropped, so
    /// a daemon cannot make the app keep what nobody asked for.
    wanted: HashSet<u64>,
    replies: BTreeMap<u64, Result<Value, LinkError>>,
}

impl ClientLink {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            wanted: HashSet::new(),
            replies: BTreeMap::new(),
        }
    }

    /// Settles the answer's id and keeps the answer only when it was asked for.
    fn take(&mut self, response: Response) {
        let Some(Id::Num(id)) = response.id else {
            return;
        };
        self.client.settle(id);
        if !self.wanted.remove(&id) {
            return;
        }
        let result = match (response.error, response.result) {
            (Some(error), _) => Err(LinkError::Refused { code: error.code }),
            (None, Some(value)) => Ok(value),
            (None, None) => Err(LinkError::Lost),
        };
        if self.replies.len() >= MAX_KEPT_REPLIES {
            self.replies.pop_first();
        }
        self.replies.insert(id, result);
    }
}

impl Link for ClientLink {
    fn hello(&self) -> &HelloResult {
        self.client.hello()
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.client
            .call::<Value, Value>(method, params)
            .map_err(link_error)
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<(u64, Notification)>, LinkError> {
        // An answer read here is kept for `reply` and the wait goes on, so `None` still means
        // that nothing arrived in `timeout` (the supervisor reads it as "idle").
        let deadline = Instant::now() + timeout;
        let mut first = true;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            // A zero read timeout is an error on Unix sockets: once the window is spent, stop
            // before reading (the first pass still drains what is already queued).
            if left.is_zero() && !first {
                return Ok(None);
            }
            first = false;
            match self.client.next_incoming(left).map_err(link_error)? {
                None => return Ok(None),
                Some(Incoming::Buffered {
                    recv_ns,
                    notification,
                }) => return Ok(Some((recv_ns, notification))),
                Some(Incoming::Reply { response, .. }) => self.take(response),
                Some(Incoming::Frame { recv_ns, bytes }) => {
                    match serde_json::from_slice::<ServerMessage>(&bytes) {
                        Ok(ServerMessage::Notification(notification)) => {
                            return Ok(Some((recv_ns, notification)));
                        }
                        Ok(ServerMessage::Response(response)) => self.take(response),
                        Err(_) => return Err(LinkError::Lost),
                    }
                }
            }
        }
    }

    fn submit(&mut self, method: &str, params: Value) -> Result<u64, LinkError> {
        let id = self.client.submit(method, params).map_err(link_error)?;
        self.wanted.insert(id);
        Ok(id)
    }

    fn settle(&mut self, id: u64) {
        self.client.settle(id);
    }

    fn reply(&mut self, id: u64) -> Option<Result<Value, LinkError>> {
        self.replies.remove(&id)
    }

    fn cancel(&mut self, id: u64) {
        // Its own answer is never asked for, so `take` settles and drops it.
        drop(self.client.submit(QUERY_CANCEL, json!({ "request": id })));
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
            .map(|client| Box::new(ClientLink::new(client)) as Box<dyn Link>)
            .map_err(map_client_error)
    }
}
