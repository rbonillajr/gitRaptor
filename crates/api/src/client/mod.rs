//! The channel client library (TS-GRP-004, ADR-GRP-005 § 3 and § 5; INF-CKP-001 Entrega 2b,
//! ADR-CKP-003 Enmienda 2026-10-08), shared by `raptor`, its TUI and `raptor-mcp`.
//!
//! Connects to the daemon's channel and greets it. Before sending anything it checks the
//! socket's folder and the server's user (L-06, [`transport::connect`]). If no daemon runs,
//! [`ensure_daemon_with`] asks a [`Launch`] to start one and waits for the handshake; an
//! older daemon is replaced by asking it to stop, which it accepts only from the installed
//! binary (SEC-13). How a daemon is started (the login autostart, a clean environment, the
//! instance lock) is the engine's knowledge: `crates/core` implements [`Launch`], and this
//! module never launches a process.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::PROTOCOL_VERSION;
use crate::messages::{ClientKind, HelloResult, IncompatibleData, StopResult};
use crate::rpc::{ErrorObject, Notification, ServerMessage};

#[cfg(any(unix, windows))]
use {
    crate::clock::monotonic_ns,
    crate::framing::{FrameError, MAX_MESSAGE_BYTES, read_frame},
    crate::messages::{Hello, NoParams, ReplaceParams},
    crate::methods,
    crate::rpc::{Id, Request, code},
    std::io::{BufRead, BufReader, Write},
    std::time::Instant,
};

pub mod peer;
pub mod transport;

/// How long a call waits for its answer.
#[cfg(any(unix, windows))]
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the channel cannot run on this platform: there is no transport with
/// access control for it (neither a Unix socket nor a Windows named pipe).
/// Never a channel without access control (fail-closed).
pub const TRANSPORT_UNSUPPORTED: &str =
    "the local channel is not supported on this platform (no transport with access control)";

/// Why talking to the daemon failed.
#[derive(Debug)]
pub enum ClientError {
    /// No daemon is listening (no socket, or a stale one).
    NotRunning,
    /// The daemon speaks an older protocol and could not be replaced.
    Incompatible(IncompatibleData),
    /// The daemon speaks a newer protocol: this client must be updated.
    ClientTooOld(IncompatibleData),
    /// The daemon answered with an error.
    Rpc(ErrorObject),
    /// The daemon did not complete the handshake in time.
    StartTimeout,
    Io(io::Error),
    /// The daemon sent something that is not the contract.
    Protocol(&'static str),
    Unsupported(&'static str),
    /// This platform has no channel transport yet (Windows): there is no
    /// channel at all rather than one without access control.
    TransportUnsupported,
    /// The socket's folder is not private, or the server runs as another
    /// user (SEC-01, L-06): nothing was sent.
    ChannelRejected,
    /// Starting a daemon failed (spawning it, or the service manager): the engine is
    /// unavailable, which is not a rejected channel.
    Launch(io::Error),
    /// The channel server failed the caller's check of who it is (the
    /// Guardrails hook client, ADR-GRD-003 § 4).
    NotAuthentic,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => write!(f, "the GitRaptor daemon is not running"),
            Self::Incompatible(d) => write!(
                f,
                "the running daemon speaks protocol {} and could not be replaced",
                d.daemon_protocol
            ),
            Self::ClientTooOld(d) => write!(
                f,
                "the running daemon ({}) is newer than this client; update GitRaptor",
                d.binary_version
            ),
            Self::Rpc(err) => write!(f, "{err}"),
            Self::StartTimeout => write!(f, "the GitRaptor daemon did not start in time"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Protocol(what) => write!(f, "unexpected message from the daemon: {what}"),
            Self::Unsupported(what) => write!(f, "not supported: {what}"),
            Self::TransportUnsupported => f.write_str(TRANSPORT_UNSUPPORTED),
            Self::Launch(err) => write!(f, "the GitRaptor daemon could not be started: {err}"),
            Self::ChannelRejected => write!(
                f,
                "the GitRaptor channel was rejected: its folder or its server is not this user's"
            ),
            Self::NotAuthentic => f.write_str("the channel server could not be verified"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => Self::NotRunning,
            io::ErrorKind::PermissionDenied => Self::ChannelRejected,
            _ => Self::Io(err),
        }
    }
}

impl From<ErrorObject> for ClientError {
    fn from(err: ErrorObject) -> Self {
        Self::Rpc(err)
    }
}

/// Where and how to connect: the inputs of [`ensure_daemon_with`]. The caller resolves the
/// runtime folder (the profile is the engine's knowledge).
#[derive(Debug, Clone)]
pub struct Connect {
    /// The profile's runtime folder: the socket's, or the key of the pipe's name.
    pub runtime: PathBuf,
    pub kind: ClientKind,
    pub protocol: u32,
    /// From launch to completed handshake.
    pub start_timeout: Duration,
    /// The capabilities this binary understands (ADR-GRP-016 § 1): a daemon
    /// of the same protocol that lacks one is replaced. Every one of the
    /// contract; tests play a newer binary with more.
    pub capabilities: Vec<&'static str>,
}

impl Connect {
    pub fn new(runtime: PathBuf, kind: ClientKind) -> Self {
        Self {
            runtime,
            kind,
            protocol: PROTOCOL_VERSION,
            start_timeout: Duration::from_secs(5),
            capabilities: crate::capability::all().map(|c| c.name).collect(),
        }
    }
}

/// Starts a daemon and waits for an old one to go: the engine's side of the on-demand start
/// (ADR-GRP-005 § 3, SEC-10). `crates/core` implements it; this module only calls it.
pub trait Launch: Send {
    /// Starts a daemon in the background. [`ClientError::NotRunning`] when this client
    /// never starts one.
    fn launch(&mut self) -> Result<(), ClientError>;
    /// Waits up to `timeout` until a daemon that agreed to stop released its instance lock.
    fn wait_released(&mut self, timeout: Duration) -> bool;
}

/// A client that never starts a daemon (tests, and whoever must not).
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverLaunch;

impl Launch for NeverLaunch {
    fn launch(&mut self) -> Result<(), ClientError> {
        Err(ClientError::NotRunning)
    }

    fn wait_released(&mut self, _timeout: Duration) -> bool {
        false
    }
}

/// A greeted connection to the daemon. On Windows it cannot be built.
#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
pub struct Client {
    #[cfg(any(unix, windows))]
    stream: transport::Stream,
    #[cfg(any(unix, windows))]
    reader: BufReader<transport::Stream>,
    next_id: u64,
    /// Notifications read while waiting for an answer, with the
    /// [`monotonic_ns`] reading taken when their frame was read.
    notifications: VecDeque<(u64, Notification)>,
    hello: HelloResult,
}

/// A message from the daemon with the moment it was read
/// (`t_client_recv`, ADR-CKP-003 § 6), before it is decoded.
#[derive(Debug)]
pub enum Incoming {
    /// A notification read while a call waited for its answer, already decoded.
    Buffered {
        recv_ns: u64,
        notification: Notification,
    },
    /// A complete frame, not decoded yet: decoding counts in the client's budget.
    Frame { recv_ns: u64, bytes: Vec<u8> },
}

/// Outcome of a handshake on an open connection.
#[cfg(any(unix, windows))]
enum Greeting {
    Ready(HelloResult),
    Incompatible(IncompatibleData),
}

impl Client {
    /// Connects and greets. Does not start a daemon.
    #[cfg(any(unix, windows))]
    pub fn connect(runtime: &Path, kind: ClientKind, protocol: u32) -> Result<Self, ClientError> {
        let mut client = Self::open_at(runtime)?;
        match client.greet(kind, protocol)? {
            Greeting::Ready(hello) => {
                client.hello = hello;
                client.accept_capabilities(kind)?;
                Ok(client)
            }
            Greeting::Incompatible(data) if data.daemon_protocol < protocol => {
                Err(ClientError::Incompatible(data))
            }
            Greeting::Incompatible(data) => Err(ClientError::ClientTooOld(data)),
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub fn connect(
        _runtime: &Path,
        _kind: ClientKind,
        _protocol: u32,
    ) -> Result<Self, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    /// Connects to the channel in a runtime folder fixed beforehand (the constant of a
    /// Guardrails dispatcher, ADR-GRD-003 § 4). Before sending anything, `verify` gets the
    /// server's pid as the kernel reports it; if it refuses, nothing is sent
    /// ([`ClientError::NotAuthentic`]). Does not start a daemon.
    #[cfg(any(unix, windows))]
    pub fn connect_runtime(
        runtime: &Path,
        kind: ClientKind,
        protocol: u32,
        verify: impl FnOnce(u32) -> bool,
    ) -> Result<Self, ClientError> {
        let mut client = Self::open_at(runtime)?;
        let pid = peer::peer_cred(&client.stream)?.pid;
        if !verify(pid) {
            return Err(ClientError::NotAuthentic);
        }
        match client.greet(kind, protocol)? {
            Greeting::Ready(hello) => {
                client.hello = hello;
                client.accept_capabilities(kind)?;
                Ok(client)
            }
            Greeting::Incompatible(data) if data.daemon_protocol < protocol => {
                Err(ClientError::Incompatible(data))
            }
            Greeting::Incompatible(data) => Err(ClientError::ClientTooOld(data)),
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub fn connect_runtime(
        _runtime: &Path,
        _kind: ClientKind,
        _protocol: u32,
        _verify: impl FnOnce(u32) -> bool,
    ) -> Result<Self, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    #[cfg(any(unix, windows))]
    fn open_at(runtime: &Path) -> Result<Self, ClientError> {
        let stream = transport::connect(runtime)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            next_id: 1,
            notifications: VecDeque::new(),
            hello: HelloResult {
                protocol: 0,
                binary_version: String::new(),
                instance_id: String::new(),
                daemon_pid: 0,
                profile: crate::messages::ConnectionProfile::Full,
                max_message_bytes: 0,
                methods: Vec::new(),
                requester: None,
                capabilities: None,
            },
        })
    }

    #[cfg(any(unix, windows))]
    fn greet(&mut self, kind: ClientKind, protocol: u32) -> Result<Greeting, ClientError> {
        let hello = Hello {
            protocol,
            client: kind,
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        match self.call::<_, HelloResult>(methods::HELLO, hello) {
            Ok(result) => Ok(Greeting::Ready(result)),
            Err(ClientError::Rpc(err)) if err.code == code::INCOMPATIBLE_PROTOCOL => {
                let data = err
                    .data
                    .and_then(|d| serde_json::from_value(d).ok())
                    .ok_or(ClientError::Protocol("incompatible without data"))?;
                Ok(Greeting::Incompatible(data))
            }
            Err(err) => Err(err),
        }
    }

    /// What the daemon said in the handshake.
    pub fn hello(&self) -> &HelloResult {
        &self.hello
    }

    /// Capabilities this binary understands that the daemon does not serve
    /// (ADR-GRP-016 § 1): the daemon is older than this binary. Empty for a
    /// daemon of protocol 5 to 8, which the protocol number replaces.
    pub fn missing_capabilities(&self, known: &[&'static str]) -> Vec<&'static str> {
        let Some(served) = &self.hello.capabilities else {
            return Vec::new();
        };
        known
            .iter()
            .copied()
            .filter(|k| !served.iter().any(|s| s == k))
            .collect()
    }

    /// Asks the daemon for the capabilities added after protocol 9 that this
    /// binary understands and the daemon serves; the legacy ones come with
    /// the protocol. Nothing to ask, nothing sent.
    #[cfg(any(unix, windows))]
    fn accept_capabilities(&mut self, kind: ClientKind) -> Result<(), ClientError> {
        let Some(served) = &self.hello.capabilities else {
            return Ok(());
        };
        let wanted: Vec<String> = crate::capability::all()
            .filter(|c| c.legacy.is_none() && served.iter().any(|s| s == c.name))
            // `raptor-mcp` never asks for names and emails (US-GRD-019).
            .filter(|c| !(kind == ClientKind::Mcp && c.name == methods::CAP_EVENTS_AUTHORSHIP.name))
            .map(|c| c.name.to_owned())
            .collect();
        if !wanted.is_empty() {
            let _: crate::capability::AcceptResult = self.call(
                methods::CONNECTION_ACCEPT,
                crate::capability::AcceptParams {
                    capabilities: wanted,
                },
            )?;
        }
        Ok(())
    }

    /// Sends a request and waits for its answer. Notifications that arrive
    /// meanwhile are kept for [`Client::next_notification`].
    #[cfg(any(unix, windows))]
    pub fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: P,
    ) -> Result<R, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_line(&Request::new(id, method, params))?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ClientError::Io(io::ErrorKind::TimedOut.into()));
            }
            let Some(bytes) = self.read_frame(Some(remaining))? else {
                return Err(ClientError::Io(io::ErrorKind::UnexpectedEof.into()));
            };
            let recv_ns = monotonic_ns();
            match decode(&bytes)? {
                ServerMessage::Response(resp) if resp.id == Some(Id::Num(id)) => {
                    return Ok(resp.into_result()?);
                }
                // An error the daemon could not tie to a request (a frame
                // too large, a connection limit) answers this one.
                ServerMessage::Response(resp) if resp.id.is_none() => {
                    return Err(resp
                        .error
                        .map(ClientError::Rpc)
                        .unwrap_or(ClientError::Protocol("response without id")));
                }
                ServerMessage::Response(_) => {}
                ServerMessage::Notification(n) => self.notifications.push_back((recv_ns, n)),
            }
        }
    }

    /// Writes one raw line (tests use it to send malformed messages).
    #[cfg(any(unix, windows))]
    pub fn send_raw(&mut self, line: &[u8]) -> Result<(), ClientError> {
        self.stream.write_all(line)?;
        self.stream.write_all(b"\n")?;
        Ok(())
    }

    #[cfg(any(unix, windows))]
    fn send_line(&mut self, message: &impl Serialize) -> Result<(), ClientError> {
        let line = serde_json::to_vec(message).map_err(|_| ClientError::Protocol("encode"))?;
        self.send_raw(&line)
    }

    /// The next message from the daemon; `None` at end of stream.
    #[cfg(any(unix, windows))]
    pub fn read_message(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Option<ServerMessage>, ClientError> {
        match self.read_frame(timeout)? {
            Some(bytes) => decode(&bytes).map(Some),
            None => Ok(None),
        }
    }

    /// The next complete frame, not decoded; `None` at end of stream.
    #[cfg(any(unix, windows))]
    fn read_frame(&mut self, timeout: Option<Duration>) -> Result<Option<Vec<u8>>, ClientError> {
        self.reader.get_ref().set_read_timeout(timeout)?;
        let mut buf = Vec::new();
        match read_frame(&mut self.reader, &mut buf, MAX_MESSAGE_BYTES) {
            Ok(true) => Ok(Some(buf)),
            Ok(false) => Ok(None),
            Err(FrameError::TooLarge) => Err(ClientError::Protocol("message too large")),
            Err(FrameError::Io(err)) => Err(ClientError::Io(err)),
        }
    }

    /// The next message, stamped with [`monotonic_ns`] as soon as its frame
    /// is read, waiting up to `timeout`. `None` on timeout; the end of the
    /// stream is an `UnexpectedEof` error.
    #[cfg(any(unix, windows))]
    pub fn next_incoming(&mut self, timeout: Duration) -> Result<Option<Incoming>, ClientError> {
        if let Some((recv_ns, notification)) = self.notifications.pop_front() {
            return Ok(Some(Incoming::Buffered {
                recv_ns,
                notification,
            }));
        }
        match self.read_frame(Some(timeout)) {
            Ok(Some(bytes)) => Ok(Some(Incoming::Frame {
                recv_ns: monotonic_ns(),
                bytes,
            })),
            Ok(None) => Err(ClientError::Io(io::ErrorKind::UnexpectedEof.into())),
            Err(ClientError::Io(err))
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// The next notification, waiting up to `timeout`. `None` on timeout or
    /// end of stream.
    #[cfg(any(unix, windows))]
    pub fn next_notification(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Notification>, ClientError> {
        if let Some((_, n)) = self.notifications.pop_front() {
            return Ok(Some(n));
        }
        match self.read_message(Some(timeout)) {
            Ok(Some(ServerMessage::Notification(n))) => Ok(Some(n)),
            Ok(Some(ServerMessage::Response(_))) | Ok(None) => Ok(None),
            Err(ClientError::Io(err))
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// Asks the daemon to stop. A reserved command: the daemon decides.
    #[cfg(any(unix, windows))]
    pub fn stop_daemon(&mut self) -> Result<StopResult, ClientError> {
        self.call(methods::DAEMON_STOP, NoParams {})
    }

    /// Reads lines until the daemon closes the connection (after a stop).
    #[cfg(any(unix, windows))]
    pub fn wait_closed(&mut self, timeout: Duration) -> bool {
        let _ = self.reader.get_ref().set_read_timeout(Some(timeout));
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
    }
}

/// A frame as a message of the contract.
#[cfg(any(unix, windows))]
fn decode(bytes: &[u8]) -> Result<ServerMessage, ClientError> {
    serde_json::from_slice(bytes).map_err(|_| ClientError::Protocol("not a contract message"))
}

/// Connects to the daemon, starting it through `launch` if needed (ADR-GRP-005 § 3) and
/// replacing an older one (SEC-13). `on_launch` is called right before a daemon is started,
/// so a client can say "starting the engine" apart from "connecting" (ADR-CKP-003 § 4).
#[cfg(any(unix, windows))]
pub fn ensure_daemon_with(
    connect: &Connect,
    launch: &mut dyn Launch,
    on_launch: &mut dyn FnMut(),
) -> Result<Client, ClientError> {
    match Client::connect(&connect.runtime, connect.kind, connect.protocol) {
        Ok(client)
            if client
                .missing_capabilities(&connect.capabilities)
                .is_empty() =>
        {
            return Ok(client);
        }
        // A daemon of this protocol but older than this binary: replaced if
        // it accepts (only from the installed binary); otherwise the client
        // goes on with what it was granted, as before capabilities.
        Ok(client) => {
            if let Some(client) = replace_same_protocol(connect, launch, client)? {
                return Ok(client);
            }
        }
        Err(ClientError::NotRunning) => {}
        Err(ClientError::Incompatible(data)) => replace(connect, launch, &data)?,
        Err(err) => return Err(err),
    }
    on_launch();
    launch.launch()?;
    let deadline = Instant::now() + connect.start_timeout;
    loop {
        match Client::connect(&connect.runtime, connect.kind, connect.protocol) {
            Ok(client) => return Ok(client),
            Err(ClientError::NotRunning) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(ClientError::NotRunning) => return Err(ClientError::StartTimeout),
            Err(err) => return Err(err),
        }
    }
}

/// Windows: no transport yet. Every operation of a [`Client`] (which
/// cannot be constructed there) fails with [`ClientError::TransportUnsupported`].
#[cfg(not(any(unix, windows)))]
impl Client {
    pub fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        _method: &str,
        _params: P,
    ) -> Result<R, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    pub fn send_raw(&mut self, _line: &[u8]) -> Result<(), ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    pub fn read_message(
        &mut self,
        _timeout: Option<Duration>,
    ) -> Result<Option<ServerMessage>, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    pub fn next_notification(
        &mut self,
        _timeout: Duration,
    ) -> Result<Option<Notification>, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    pub fn next_incoming(&mut self, _timeout: Duration) -> Result<Option<Incoming>, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    pub fn stop_daemon(&mut self) -> Result<StopResult, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    pub fn wait_closed(&mut self, _timeout: Duration) -> bool {
        false
    }
}

#[cfg(not(any(unix, windows)))]
pub fn ensure_daemon_with(
    connect: &Connect,
    _launch: &mut dyn Launch,
    _on_launch: &mut dyn FnMut(),
) -> Result<Client, ClientError> {
    Client::connect(&connect.runtime, connect.kind, connect.protocol)
}

/// Asks a daemon of this protocol that lacks capabilities of this binary
/// to step down for it (ADR-GRP-016 § 1). `Some(client)`: it refused, go on
/// with that connection; `None`: it stopped and released its lock.
#[cfg(any(unix, windows))]
fn replace_same_protocol(
    connect: &Connect,
    launch: &mut dyn Launch,
    mut old: Client,
) -> Result<Option<Client>, ClientError> {
    let stop: Result<StopResult, ClientError> = old.call(
        methods::DAEMON_REPLACE,
        ReplaceParams {
            protocol: connect.protocol,
        },
    );
    match stop {
        Ok(stop) if stop.stopping => {}
        Ok(_) | Err(ClientError::Rpc(_)) => return Ok(Some(old)),
        Err(err) => return Err(err),
    }
    old.wait_closed(Duration::from_secs(10));
    if launch.wait_released(Duration::from_secs(10)) {
        Ok(None)
    } else {
        Err(ClientError::StartTimeout)
    }
}

/// Asks an older daemon to step down for this (installed) binary and waits
/// until it released the instance lock.
#[cfg(any(unix, windows))]
fn replace(
    connect: &Connect,
    launch: &mut dyn Launch,
    data: &IncompatibleData,
) -> Result<(), ClientError> {
    let mut old = Client::open_at(&connect.runtime)?;
    match old.greet(connect.kind, connect.protocol)? {
        Greeting::Incompatible(_) => {}
        Greeting::Ready(_) => return Ok(()),
    }
    let stop: StopResult = old
        .call(
            methods::DAEMON_REPLACE,
            ReplaceParams {
                protocol: connect.protocol,
            },
        )
        .map_err(|err| match err {
            ClientError::Rpc(_) => ClientError::Incompatible(data.clone()),
            other => other,
        })?;
    if !stop.stopping {
        return Err(ClientError::Incompatible(data.clone()));
    }
    old.wait_closed(Duration::from_secs(10));
    if launch.wait_released(Duration::from_secs(10)) {
        Ok(())
    } else {
        Err(ClientError::Incompatible(data.clone()))
    }
}
