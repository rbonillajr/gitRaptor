//! Client library shared by `raptor` and `raptor-mcp` (TS-GRP-004,
//! ADR-GRP-005 § 3 and § 5).
//!
//! Connects to the daemon's channel and greets it; if no daemon runs, starts
//! the installed binary with a clean environment (SEC-10) and waits for the
//! handshake. An older daemon is replaced by asking it to stop, which it
//! accepts only from the installed binary (SEC-13). Registering the login
//! autostart, and starting through the service manager when it is
//! registered, belong to US-GRP-004.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{ClientKind, HelloResult, IncompatibleData, StopResult};
use gitraptor_api::rpc::{ErrorObject, Notification, ServerMessage};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::profile::ProfileDirs;

#[cfg(unix)]
use {
    crate::daemon::wait_until_released,
    gitraptor_api::clock::monotonic_ns,
    gitraptor_api::framing::{FrameError, MAX_MESSAGE_BYTES, read_frame},
    gitraptor_api::messages::{Hello, NoParams, ReplaceParams},
    gitraptor_api::methods,
    gitraptor_api::rpc::{Id, Request, code},
    std::ffi::OsString,
    std::io::{BufRead, BufReader, Write},
    std::process::{Command, Stdio},
    std::time::Instant,
};

/// How long a call waits for its answer.
#[cfg(unix)]
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

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
            Self::TransportUnsupported => f.write_str(crate::channel::TRANSPORT_UNSUPPORTED),
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

/// How to start a daemon that is not running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launcher {
    /// Run `<exe> daemon` with a clean environment.
    Installed(PathBuf),
    /// Never start one.
    Never,
}

impl Launcher {
    /// The `raptor` binary installed next to the running executable (the
    /// CLI itself, or the sibling of `raptor-mcp`).
    pub fn installed() -> Self {
        match std::env::current_exe() {
            Ok(exe) => Self::Installed(exe.with_file_name(if cfg!(windows) {
                "raptor.exe"
            } else {
                "raptor"
            })),
            Err(_) => Self::Never,
        }
    }
}

/// Inputs of [`ensure_daemon`].
#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub dirs: ProfileDirs,
    pub kind: ClientKind,
    pub protocol: u32,
    pub launcher: Launcher,
    /// From launch to completed handshake.
    pub start_timeout: Duration,
}

impl ClientOptions {
    pub fn new(dirs: ProfileDirs, kind: ClientKind) -> Self {
        Self {
            dirs,
            kind,
            protocol: PROTOCOL_VERSION,
            launcher: Launcher::installed(),
            start_timeout: Duration::from_secs(5),
        }
    }
}

/// A greeted connection to the daemon. On Windows it cannot be built.
#[cfg_attr(not(unix), allow(dead_code))]
pub struct Client {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    reader: BufReader<std::os::unix::net::UnixStream>,
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
#[cfg(unix)]
enum Greeting {
    Ready(HelloResult),
    Incompatible(IncompatibleData),
}

impl Client {
    /// Connects and greets. Does not start a daemon.
    #[cfg(unix)]
    pub fn connect(
        dirs: &ProfileDirs,
        kind: ClientKind,
        protocol: u32,
    ) -> Result<Self, ClientError> {
        let mut client = Self::open(dirs)?;
        match client.greet(kind, protocol)? {
            Greeting::Ready(hello) => {
                client.hello = hello;
                Ok(client)
            }
            Greeting::Incompatible(data) if data.daemon_protocol < protocol => {
                Err(ClientError::Incompatible(data))
            }
            Greeting::Incompatible(data) => Err(ClientError::ClientTooOld(data)),
        }
    }

    #[cfg(not(unix))]
    pub fn connect(
        _dirs: &ProfileDirs,
        _kind: ClientKind,
        _protocol: u32,
    ) -> Result<Self, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    /// Connects to the channel in a runtime folder fixed beforehand (the constant of a
    /// Guardrails dispatcher, ADR-GRD-003 § 4). Before sending anything, `verify` gets the
    /// server's pid as the kernel reports it; if it refuses, nothing is sent
    /// ([`ClientError::NotAuthentic`]). Does not start a daemon.
    #[cfg(unix)]
    pub fn connect_runtime(
        runtime: &std::path::Path,
        kind: ClientKind,
        protocol: u32,
        verify: impl FnOnce(u32) -> bool,
    ) -> Result<Self, ClientError> {
        let mut client = Self::open_at(runtime)?;
        let pid = crate::channel::peer::peer_cred(&client.stream)?.pid;
        if !verify(pid) {
            return Err(ClientError::NotAuthentic);
        }
        match client.greet(kind, protocol)? {
            Greeting::Ready(hello) => {
                client.hello = hello;
                Ok(client)
            }
            Greeting::Incompatible(data) if data.daemon_protocol < protocol => {
                Err(ClientError::Incompatible(data))
            }
            Greeting::Incompatible(data) => Err(ClientError::ClientTooOld(data)),
        }
    }

    #[cfg(not(unix))]
    pub fn connect_runtime(
        _runtime: &std::path::Path,
        _kind: ClientKind,
        _protocol: u32,
        _verify: impl FnOnce(u32) -> bool,
    ) -> Result<Self, ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    #[cfg(unix)]
    fn open(dirs: &ProfileDirs) -> Result<Self, ClientError> {
        let runtime = dirs
            .runtime
            .as_deref()
            .ok_or(ClientError::Unsupported("no runtime folder"))?;
        Self::open_at(runtime)
    }

    #[cfg(unix)]
    fn open_at(runtime: &std::path::Path) -> Result<Self, ClientError> {
        let stream = crate::channel::transport::connect(runtime)?;
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
                profile: gitraptor_api::messages::ConnectionProfile::Full,
                max_message_bytes: 0,
                methods: Vec::new(),
                requester: None,
            },
        })
    }

    #[cfg(unix)]
    fn greet(&mut self, kind: ClientKind, protocol: u32) -> Result<Greeting, ClientError> {
        let hello = Hello {
            protocol,
            client: kind,
            client_version: crate::version().to_owned(),
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

    /// Sends a request and waits for its answer. Notifications that arrive
    /// meanwhile are kept for [`Client::next_notification`].
    #[cfg(unix)]
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
    #[cfg(unix)]
    pub fn send_raw(&mut self, line: &[u8]) -> Result<(), ClientError> {
        self.stream.write_all(line)?;
        self.stream.write_all(b"\n")?;
        Ok(())
    }

    #[cfg(unix)]
    fn send_line(&mut self, message: &impl Serialize) -> Result<(), ClientError> {
        let line = serde_json::to_vec(message).map_err(|_| ClientError::Protocol("encode"))?;
        self.send_raw(&line)
    }

    /// The next message from the daemon; `None` at end of stream.
    #[cfg(unix)]
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
    #[cfg(unix)]
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
    #[cfg(unix)]
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
    #[cfg(unix)]
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
    #[cfg(unix)]
    pub fn stop_daemon(&mut self) -> Result<StopResult, ClientError> {
        self.call(methods::DAEMON_STOP, NoParams {})
    }

    /// Reads lines until the daemon closes the connection (after a stop).
    #[cfg(unix)]
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
#[cfg(unix)]
fn decode(bytes: &[u8]) -> Result<ServerMessage, ClientError> {
    serde_json::from_slice(bytes).map_err(|_| ClientError::Protocol("not a contract message"))
}

/// Connects to the daemon, starting it if needed (ADR-GRP-005 § 3) and
/// replacing an older one (SEC-13).
#[cfg(unix)]
pub fn ensure_daemon(options: &ClientOptions) -> Result<Client, ClientError> {
    match Client::connect(&options.dirs, options.kind, options.protocol) {
        Ok(client) => return Ok(client),
        Err(ClientError::NotRunning) => {}
        Err(ClientError::Incompatible(data)) => replace(options, &data)?,
        Err(err) => return Err(err),
    }
    launch(options)?;
    let deadline = Instant::now() + options.start_timeout;
    loop {
        match Client::connect(&options.dirs, options.kind, options.protocol) {
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
#[cfg(not(unix))]
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

#[cfg(not(unix))]
pub fn ensure_daemon(options: &ClientOptions) -> Result<Client, ClientError> {
    Client::connect(&options.dirs, options.kind, options.protocol)
}

/// Asks an older daemon to step down for this (installed) binary and waits
/// until it released the instance lock.
#[cfg(unix)]
fn replace(options: &ClientOptions, data: &IncompatibleData) -> Result<(), ClientError> {
    let mut old = Client::open(&options.dirs)?;
    match old.greet(options.kind, options.protocol)? {
        Greeting::Incompatible(_) => {}
        Greeting::Ready(_) => return Ok(()),
    }
    let stop: StopResult = old
        .call(
            methods::DAEMON_REPLACE,
            ReplaceParams {
                protocol: options.protocol,
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
    if wait_until_released(&options.dirs.state, Duration::from_secs(10)).unwrap_or(false) {
        Ok(())
    } else {
        Err(ClientError::Incompatible(data.clone()))
    }
}

/// Starts `<raptor> daemon` detached, with a clean environment built by
/// allowlist and the working folder in the profile (SEC-10).
#[cfg(unix)]
fn launch(options: &ClientOptions) -> Result<(), ClientError> {
    let Launcher::Installed(exe) = &options.launcher else {
        return Err(ClientError::NotRunning);
    };
    let cwd = if options.dirs.state.is_dir() {
        options.dirs.state.clone()
    } else {
        PathBuf::from("/")
    };
    let mut child = Command::new(exe)
        .arg("daemon")
        .env_clear()
        .envs(clean_env())
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it whenever it exits (a second daemon exits at once with code 3).
    let _ = std::thread::Builder::new()
        .name("raptor-daemon-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

/// The fixed `PATH` of an on-demand daemon: the client's is never passed.
pub const DAEMON_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// Environment of an on-demand daemon. `HOME`, `USER` and `LOGNAME` come
/// from the user database, not from the client: a client with a hostile
/// `HOME` cannot move the daemon to another profile. Debug builds also pass
/// the test overrides of the profile, of the agent classifier and of the
/// sessions' clock and the resource targets.
#[cfg(unix)]
pub fn clean_env() -> Vec<(OsString, OsString)> {
    let mut env = vec![(OsString::from("PATH"), OsString::from(DAEMON_PATH))];
    if let Ok(Some(user)) = nix::unistd::User::from_uid(nix::unistd::getuid()) {
        env.push(("HOME".into(), user.dir.into_os_string()));
        env.push(("USER".into(), user.name.clone().into()));
        env.push(("LOGNAME".into(), user.name.into()));
    }
    if cfg!(debug_assertions) {
        for name in [
            crate::profile::PROFILE_DIR_ENV,
            crate::daemon::AGENT_EXECUTABLES_ENV,
            crate::daemon::CLOCK_SKEW_FILE_ENV,
            crate::resources::RESOURCE_TARGETS_ENV,
        ] {
            if let Some(value) = std::env::var_os(name) {
                env.push((name.into(), value));
            }
        }
    }
    env
}

/// The runtime folder the client connects to, for diagnostics.
pub fn socket_path(dirs: &ProfileDirs) -> Option<PathBuf> {
    dirs.runtime
        .as_deref()
        .map(crate::channel::transport::socket_path)
}
