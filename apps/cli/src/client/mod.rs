//! Client of the channel for the TUI (ADR-CKP-003 § 4).
//!
//! The channel thread connects, takes a coherent snapshot of each scope and
//! subscribes from the next sequence, then reads the stream, stamping
//! `t_client_recv` on every frame before decoding it (§ 6). It reconnects
//! with a growing wait when the channel drops. Sequence checks are not
//! here: they are pure and run in `update` ([`sequence`]).
//!
//! The transport comes through [`Connector`] and [`Link`], so this module
//! does not import the engine: the binary plugs in the client library
//! (today in `crates/core`, to be moved to `crates/api`).

pub mod sequence;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::clock::monotonic_ns;
use gitraptor_api::messages::{
    EventsHistoryParams, EventsHistoryResult, MAX_HISTORY_PAGE, SessionsListParams,
    SessionsListResult,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::{Notification, ServerMessage};
use gitraptor_api::scope::{
    ConnectionRequester, RepoLocateParams, RepoLocateResult, Scope, ScopeEventNotification,
    ScopeResyncNotification, ScopeSnapshot, ScopeSnapshotParams, ScopeSubscribeParams,
};
use serde_json::Value;

use crate::model::{ConnEvent, ConnState, EngineMsg, Msg, Stamped};
use crate::present;
use crate::queue::{Closed, Outlet};

/// First wait before reconnecting (⚠️ ASSUMPTION of ADR-CKP-003 § 4).
pub const BACKOFF_MIN: Duration = Duration::from_millis(250);
/// Longest wait between attempts; there is no limit of attempts.
pub const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// How long one read of the stream waits before looking at the commands.
const READ_SLICE: Duration = Duration::from_millis(20);

/// Opens greeted connections, starting the daemon if needed (§ 5).
pub trait Connector: Send + 'static {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError>;

    /// [`Connector::connect`], calling `starting` when no engine was running and one is being
    /// started, so the TUI says "starting the engine" apart from "connecting".
    fn connect_starting(&mut self, starting: &mut dyn FnMut()) -> Result<Box<dyn Link>, LinkError> {
        let _ = starting;
        self.connect()
    }
}

/// One greeted connection.
pub trait Link: Send {
    /// Who the daemon sees on this connection, from the handshake (N5).
    fn requester(&self) -> Option<ConnectionRequester>;
    /// Whether the connection has a capability (ADR-GRP-016 § 1). A link that cannot tell has
    /// none of them.
    fn has(&self, _capability: &str) -> bool {
        false
    }
    /// A request and its answer.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError>;
    /// The next message, waiting up to `timeout`; `None` on timeout.
    fn next(&mut self, timeout: Duration) -> Result<Option<Incoming>, LinkError>;
}

/// A message read from the channel, stamped when its frame was read.
#[derive(Debug)]
pub enum Incoming {
    /// Read while a call waited for its answer, already decoded.
    Decoded {
        recv_ns: u64,
        notification: Notification,
    },
    /// A complete frame, not decoded yet.
    Frame { recv_ns: u64, bytes: Vec<u8> },
}

/// Why a connection failed or ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// No daemon, and it could not be started.
    EngineUnavailable,
    /// Socket folder or server failed the peer check: nothing was sent.
    Rejected,
    Incompatible,
    /// No transport on this platform.
    Unsupported,
    /// The daemon answered a request with an error.
    Refused,
    /// The connection dropped or sent something outside the contract.
    Lost,
}

/// Commands for the channel thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCmd {
    Resync {
        scope: Scope,
        resubscribe: bool,
    },
    /// Show this repo, chosen because the folder is in none (kept across reconnections).
    Open {
        repo_id: String,
    },
    Reconnect,
    Shutdown,
}

/// The running channel thread.
#[derive(Debug)]
pub struct ClientThread {
    pub cmds: Sender<LinkCmd>,
    handle: Option<JoinHandle<()>>,
}

impl ClientThread {
    /// Asks the thread to stop and waits for it.
    pub fn shutdown(mut self) {
        let _ = self.cmds.send(LinkCmd::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Starts the channel thread. `cwd` selects the repo that contains it
/// (N4); the TUI never reads Git to find it.
pub fn spawn(connector: impl Connector, cwd: Option<PathBuf>, out: Outlet) -> ClientThread {
    let (cmds, rx) = std::sync::mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("raptor-channel".into())
        .spawn(move || run(connector, cwd, &out, &rx))
        .ok();
    ClientThread { cmds, handle }
}

/// Wait before attempt `attempt` (1-based): 250 ms, 500 ms, 1 s… up to 5 s.
pub fn backoff(attempt: u32) -> Duration {
    let factor = 1u32 << attempt.saturating_sub(1).min(5);
    (BACKOFF_MIN * factor).min(BACKOFF_MAX)
}

/// How a session ended.
enum End {
    Reconnect,
    Shutdown,
}

fn run(
    mut connector: impl Connector,
    cwd: Option<PathBuf>,
    out: &Outlet,
    cmds: &Receiver<LinkCmd>,
) {
    let mut attempt: u32 = 0;
    // The repo the developer chose (or the only one) when the folder is in none.
    let mut chosen: Option<String> = None;
    loop {
        let state = if attempt == 0 {
            ConnState::Connecting
        } else {
            ConnState::Reconnecting { attempt }
        };
        if send_state(out, state).is_err() {
            return;
        }
        let mut starting = || {
            let _ = send_state(out, ConnState::Starting);
        };
        let failure = match connector.connect_starting(&mut starting) {
            Ok(mut link) => match session(link.as_mut(), cwd.as_ref(), &mut chosen, out, cmds) {
                Ok(End::Shutdown) | Err(Closed) => return,
                Ok(End::Reconnect) => {
                    attempt = 0;
                    None
                }
            },
            Err(err) => Some(err),
        };
        attempt = attempt.saturating_add(1);
        let state = match failure {
            Some(LinkError::Unsupported) => {
                // There is nothing to retry on this platform.
                let _ = send_state(out, ConnState::Unsupported);
                wait_for_shutdown(cmds);
                return;
            }
            Some(LinkError::EngineUnavailable) => Some(ConnState::EngineUnavailable),
            Some(LinkError::Rejected) => Some(ConnState::Rejected),
            Some(LinkError::Incompatible) => Some(ConnState::Incompatible),
            Some(LinkError::Refused | LinkError::Lost) | None => None,
        };
        if let Some(state) = state
            && send_state(out, state).is_err()
        {
            return;
        }
        match cmds.recv_timeout(backoff(attempt)) {
            Ok(LinkCmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(LinkCmd::Reconnect) => attempt = 0,
            Ok(LinkCmd::Open { repo_id }) => chosen = Some(repo_id),
            Ok(LinkCmd::Resync { .. }) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn wait_for_shutdown(cmds: &Receiver<LinkCmd>) {
    while let Ok(cmd) = cmds.recv() {
        if cmd == LinkCmd::Shutdown {
            return;
        }
    }
}

fn send_state(out: &Outlet, state: ConnState) -> Result<(), Closed> {
    out.send(Msg::Conn(ConnEvent::State(state)))
}

/// One connection: initial snapshots, then the stream until it drops.
fn session(
    link: &mut dyn Link,
    cwd: Option<&PathBuf>,
    chosen: &mut Option<String>,
    out: &Outlet,
    cmds: &Receiver<LinkCmd>,
) -> Result<End, Closed> {
    let requester = link.requester().map(|r| present::ingest::requester(&r));
    out.send(Msg::Conn(ConnEvent::Requester(requester)))?;
    out.send(Msg::Conn(ConnEvent::Activity(
        link.has(methods::CAP_SCOPE_ACTIVITY.name),
    )))?;
    send_state(out, ConnState::Syncing)?;
    if sync(link, &Scope::Global, true, out)?.is_err() {
        return Ok(End::Reconnect);
    }
    match cwd
        .and_then(|path| locate(link, path))
        .or_else(|| chosen.clone())
    {
        Some(repo_id) => {
            if sync(link, &Scope::Repo { repo_id }, true, out)?.is_err() {
                return Ok(End::Reconnect);
            }
        }
        // `update` opens the only observed repo or lets the developer choose one.
        None => out.send(Msg::Conn(ConnEvent::Unlocated))?,
    }
    send_state(out, ConnState::Live)?;
    loop {
        while let Ok(cmd) = cmds.try_recv() {
            match cmd {
                LinkCmd::Shutdown => return Ok(End::Shutdown),
                LinkCmd::Reconnect => return Ok(End::Reconnect),
                LinkCmd::Resync { scope, resubscribe } => {
                    if sync(link, &scope, resubscribe, out)?.is_err() {
                        return Ok(End::Reconnect);
                    }
                }
                LinkCmd::Open { repo_id } => {
                    *chosen = Some(repo_id.clone());
                    if sync(link, &Scope::Repo { repo_id }, true, out)?.is_err() {
                        return Ok(End::Reconnect);
                    }
                }
            }
        }
        match link.next(READ_SLICE) {
            Ok(None) => {}
            Ok(Some(incoming)) => {
                if let Some(msg) = decode(incoming) {
                    out.send(Msg::Engine(msg))?;
                }
            }
            Err(_) => return Ok(End::Reconnect),
        }
    }
}

/// A snapshot of `scope` and, when `subscribe`, a subscription from the
/// next sequence of the same daemon run (N1). The inner error asks for a
/// reconnection.
fn sync(
    link: &mut dyn Link,
    scope: &Scope,
    subscribe: bool,
    out: &Outlet,
) -> Result<Result<(), LinkError>, Closed> {
    let params = ScopeSnapshotParams {
        scope: scope.clone(),
    };
    let value = match link.call(methods::SCOPE_SNAPSHOT, to_value(&params)) {
        Ok(value) => value,
        Err(err) => return Ok(Err(err)),
    };
    let recv_ns = monotonic_ns();
    let Ok(snapshot) = serde_json::from_value::<ScopeSnapshot>(value) else {
        return Ok(Err(LinkError::Lost));
    };
    let next = ScopeSubscribeParams {
        scope: scope.clone(),
        from_seq: Some(snapshot.scope_seq() + 1),
        run_id: Some(snapshot.run_id().to_owned()),
    };
    out.send(Msg::Engine(Stamped {
        recv_ns,
        decoded_ns: monotonic_ns(),
        msg: EngineMsg::Snapshot(Box::new(snapshot)),
    }))?;
    if subscribe && let Err(err) = link.call(methods::SCOPE_SUBSCRIBE, to_value(&next)) {
        return Ok(Err(err));
    }
    if let Scope::Repo { repo_id } = scope {
        if let Err(err) = sessions(link, repo_id, out)? {
            return Ok(Err(err));
        }
        return history(link, repo_id, out);
    }
    Ok(Ok(()))
}

/// The latest Git events of the repo, asked after its sessions, for the last commit of each
/// worktree and its authorship (US-CKP-026). Every `git.event` after the snapshot comes on the
/// stream, and `update` keeps the most recent commit per worktree either way. A commit older
/// than this page has no line; a refusal (an older daemon) leaves none.
fn history(
    link: &mut dyn Link,
    repo_id: &str,
    out: &Outlet,
) -> Result<Result<(), LinkError>, Closed> {
    let params = EventsHistoryParams {
        repo_id: repo_id.to_owned(),
        limit: Some(MAX_HISTORY_PAGE),
        ..EventsHistoryParams::default()
    };
    let value = match link.call(methods::EVENTS_HISTORY, to_value(&params)) {
        Ok(value) => value,
        Err(LinkError::Refused) => return Ok(Ok(())),
        Err(err) => return Ok(Err(err)),
    };
    let recv_ns = monotonic_ns();
    // An optional line: a page this client cannot read leaves no lines, never a reconnection
    // (it would ask for the same page again).
    let Ok(result) = serde_json::from_value::<EventsHistoryResult>(value) else {
        return Ok(Ok(()));
    };
    out.send(Msg::Engine(Stamped {
        recv_ns,
        decoded_ns: monotonic_ns(),
        msg: EngineMsg::History {
            repo_id: repo_id.to_owned(),
            events: result.events,
        },
    }))?;
    Ok(Ok(()))
}

/// The sessions of the repo, asked after its snapshot and subscription: the snapshot does not
/// carry them (US-CKP-001, D1; ADR-CKP-003 § 4 amendment). Every `session.state` after the
/// snapshot's sequence comes on the stream after this answer, and `update` merges both with
/// the same upsert, so they converge. A refusal leaves the agents "not available".
fn sessions(
    link: &mut dyn Link,
    repo_id: &str,
    out: &Outlet,
) -> Result<Result<(), LinkError>, Closed> {
    let params = SessionsListParams {
        repo_id: Some(repo_id.to_owned()),
        // The present sessions and the latest ended one of each worktree; with the ended ones
        // too, a page could leave out a present session that started long ago.
        include_ended: false,
        limit: None,
    };
    let value = match link.call(methods::SESSIONS_LIST, to_value(&params)) {
        Ok(value) => value,
        Err(LinkError::Refused) => return Ok(Ok(())),
        Err(err) => return Ok(Err(err)),
    };
    let recv_ns = monotonic_ns();
    let Ok(result) = serde_json::from_value::<SessionsListResult>(value) else {
        return Ok(Err(LinkError::Lost));
    };
    out.send(Msg::Engine(Stamped {
        recv_ns,
        decoded_ns: monotonic_ns(),
        msg: EngineMsg::Sessions {
            repo_id: repo_id.to_owned(),
            result: Box::new(result),
        },
    }))?;
    Ok(Ok(()))
}

/// The observed repo that contains `path`, as the daemon canonicalizes it.
fn locate(link: &mut dyn Link, path: &std::path::Path) -> Option<String> {
    let params = RepoLocateParams {
        path: path.to_string_lossy().into_owned(),
    };
    let value = link.call(methods::REPO_LOCATE, to_value(&params)).ok()?;
    serde_json::from_value::<RepoLocateResult>(value)
        .ok()
        .map(|r| r.repo_id)
}

fn to_value(params: &impl serde::Serialize) -> Value {
    serde_json::to_value(params).unwrap_or(Value::Null)
}

/// Decodes a stream message. What is not a scope event or resync is
/// ignored (an answer that arrived late, another kind of notification).
pub fn decode(incoming: Incoming) -> Option<Stamped<EngineMsg>> {
    let (recv_ns, notification) = match incoming {
        Incoming::Decoded {
            recv_ns,
            notification,
        } => (recv_ns, notification),
        Incoming::Frame { recv_ns, bytes } => match serde_json::from_slice(&bytes) {
            Ok(ServerMessage::Notification(n)) => (recv_ns, n),
            Ok(ServerMessage::Response(_)) | Err(_) => return None,
        },
    };
    let msg = match notification.method.as_str() {
        methods::NOTIFY_SCOPE_EVENT => {
            let n: ScopeEventNotification = serde_json::from_value(notification.params).ok()?;
            EngineMsg::Event {
                scope: n.scope,
                scope_seq: n.scope_seq,
                event: Box::new(n.event),
            }
        }
        methods::NOTIFY_SCOPE_RESYNC => {
            let n: ScopeResyncNotification = serde_json::from_value(notification.params).ok()?;
            EngineMsg::Resync {
                scope: n.scope,
                reason: n.reason,
            }
        }
        _ => return None,
    };
    Some(Stamped {
        recv_ns,
        decoded_ns: monotonic_ns(),
        msg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_to_five_seconds() {
        assert_eq!(backoff(1), Duration::from_millis(250));
        assert_eq!(backoff(2), Duration::from_millis(500));
        assert_eq!(backoff(3), Duration::from_secs(1));
        assert_eq!(backoff(5), Duration::from_secs(4));
        assert_eq!(backoff(6), BACKOFF_MAX);
        assert_eq!(backoff(60), BACKOFF_MAX);
    }
}
