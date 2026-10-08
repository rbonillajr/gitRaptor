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

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::clock::monotonic_ns;
use gitraptor_api::messages::{
    EventsHistoryParams, EventsHistoryResult, MAX_HISTORY_PAGE, RepoAddParams, RepoAddResult,
    RepoRejectedData, RepoRejection, SessionsListParams, SessionsListResult,
};
use gitraptor_api::methods;
use gitraptor_api::rpc::{Notification, ServerMessage, code};
use gitraptor_api::scope::{
    ConnectionRequester, RepoLocateParams, RepoLocateResult, Scope, ScopeEventNotification,
    ScopeResyncNotification, ScopeSnapshot, ScopeSnapshotParams, ScopeSubscribeParams,
};
use serde_json::Value;

use crate::model::{Candidate, ConnEvent, ConnState, EngineMsg, Msg, ObserveFailure, Stamped};
use crate::present;
use crate::present::SafeText;
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
    /// [`Link::call`] keeping the engine's typed refusal (its code and data, never its
    /// message). A link that cannot tell says only "refused".
    fn call_refusal(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.call(method, params).map_err(|err| match err {
            LinkError::Refused => Refusal::Engine {
                code: 0,
                data: None,
            },
            other => Refusal::Link(other),
        })
    }
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

/// Why a request of [`Link::call_refusal`] failed.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The engine answered with an error: its code and data.
    Engine {
        code: i64,
        data: Option<Value>,
    },
    Link(LinkError),
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
    /// Observe the repo of the folder (`repo.add`) and show it (US-CKP-025).
    Observe {
        root: PathBuf,
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
            Ok(LinkCmd::Observe { .. }) => {
                let event = ConnEvent::ObserveFailed(ObserveFailure::Disconnected);
                if out.send(Msg::Conn(event)).is_err() {
                    return;
                }
            }
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
    let located = match cwd.map(|path| locate(link, path)).transpose() {
        Ok(located) => located.flatten(),
        Err(_) => return Ok(End::Reconnect),
    };
    match located.or_else(|| chosen.clone()) {
        Some(repo_id) => {
            if sync(link, &Scope::Repo { repo_id }, true, out)?.is_err() {
                return Ok(End::Reconnect);
            }
        }
        // In a repo the engine does not observe, `update` may offer to observe it (US-CKP-025);
        // otherwise it opens the only observed repo or lets the developer choose one.
        None => match cwd.and_then(|path| candidate(path)) {
            Some(here) => out.send(Msg::Conn(ConnEvent::Unobserved(here)))?,
            None => out.send(Msg::Conn(ConnEvent::Unlocated))?,
        },
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
                LinkCmd::Observe { root } => match observe(link, &root) {
                    Ok(repo_id) => {
                        *chosen = Some(repo_id.clone());
                        if sync(link, &Scope::Repo { repo_id }, true, out)?.is_err() {
                            return Ok(End::Reconnect);
                        }
                    }
                    Err(Refusal::Engine { code, data }) => {
                        let failure = observe_failure(code, data);
                        out.send(Msg::Conn(ConnEvent::ObserveFailed(failure)))?;
                    }
                    Err(Refusal::Link(_)) => {
                        let failure = ObserveFailure::Disconnected;
                        out.send(Msg::Conn(ConnEvent::ObserveFailed(failure)))?;
                        return Ok(End::Reconnect);
                    }
                },
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

/// The observed repo that contains `path`, as the daemon canonicalizes it. `None` when the
/// engine says it is in none (any refusal); a broken connection is an error, never "in none".
fn locate(link: &mut dyn Link, path: &Path) -> Result<Option<String>, LinkError> {
    let params = RepoLocateParams {
        path: path.to_string_lossy().into_owned(),
    };
    match link.call(methods::REPO_LOCATE, to_value(&params)) {
        Ok(value) => Ok(serde_json::from_value::<RepoLocateResult>(value)
            .ok()
            .map(|r| r.repo_id)),
        Err(LinkError::Refused) => Ok(None),
        Err(err) => Err(err),
    }
}

/// `repo.add` of the worktree root (reserved: the engine authorizes the caller again,
/// BR-AUTH-001). An already observed repo is a success too: someone added it meanwhile.
fn observe(link: &mut dyn Link, root: &Path) -> Result<String, Refusal> {
    let params = RepoAddParams {
        path: root.to_string_lossy().into_owned(),
    };
    let value = link.call_refusal(methods::REPO_ADD, to_value(&params))?;
    // An answer this client cannot read is the engine's, not a broken connection.
    serde_json::from_value::<RepoAddResult>(value)
        .map(|r| r.repo.repo_id)
        .map_err(|_| Refusal::Engine {
            code: 0,
            data: None,
        })
}

/// The typed reason of a refused `repo.add`; the engine's message is never shown (SEC-12).
fn observe_failure(code: i64, data: Option<Value>) -> ObserveFailure {
    match code {
        code::RESERVED_REFUSED => ObserveFailure::Refused,
        code::REPO_REJECTED => {
            // As `raptor repo add` reads them (`support::repo_error`).
            match data
                .and_then(|d| serde_json::from_value::<RepoRejectedData>(d).ok())
                .map(|d| d.reason)
            {
                Some(RepoRejection::NotARepo) => ObserveFailure::NotARepo,
                Some(RepoRejection::Untrusted) => ObserveFailure::NotTrusted,
                Some(RepoRejection::Unreadable) | None => ObserveFailure::Unreadable,
                Some(RepoRejection::UnknownRepo | RepoRejection::NotObserved) => {
                    ObserveFailure::Unknown
                }
            }
        }
        _ => ObserveFailure::Unknown,
    }
}

/// The repo the folder `cwd` is in, when it may be one the engine does not observe
/// (US-CKP-025): from the folder's real path, upwards, the first folder with a `.git` entry,
/// without leaving the folder's file system. Nothing of Git is read but the `.git` file of a
/// linked worktree, to name the repo it belongs to. The engine checks it again on `repo.add`.
pub fn candidate(cwd: &Path) -> Option<Candidate> {
    let real = std::fs::canonicalize(cwd).ok()?;
    let device = device(&real);
    let root = real
        .ancestors()
        .take_while(|dir| device.is_none() || device == self::device(dir))
        .find(|dir| std::fs::symlink_metadata(dir.join(".git")).is_ok())?
        .to_path_buf();
    // A linked worktree names the repo it belongs to: its main worktree.
    let main = main_worktree(&root).unwrap_or_else(|| root.clone());
    let name = main
        .file_name()
        .map_or_else(|| main.to_string_lossy(), |n| n.to_string_lossy());
    Some(Candidate {
        name: SafeText::name(&name),
        path: SafeText::text(&root.to_string_lossy()),
        root,
    })
}

/// The main worktree of the linked worktree at `root`: its `.git` file says
/// `gitdir: <common>/worktrees/<id>`, and that folder's `commondir` leads to the common Git
/// directory, whose parent is the main worktree. `None` for anything else (a submodule, a
/// main worktree, a bare repo).
fn main_worktree(root: &Path) -> Option<PathBuf> {
    let file = std::fs::read_to_string(root.join(".git")).ok()?;
    let gitdir = root.join(file.strip_prefix("gitdir:")?.trim());
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = std::fs::canonicalize(gitdir.join(common.trim())).ok()?;
    if common.file_name()? != ".git" {
        return None;
    }
    common.parent().map(Path::to_path_buf)
}

/// The file system of `path`, to stop at its boundary; `None` where it cannot be told.
#[cfg(unix)]
fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn device(_: &Path) -> Option<u64> {
    None
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

    fn dir(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path).unwrap();
        std::fs::canonicalize(path).unwrap()
    }

    /// US-CKP-025: from a folder inside a repo, upwards, the first folder with `.git`.
    #[test]
    fn candidate_root_is_the_first_folder_with_git_upwards() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = dir(&tmp.path().join("notes"));
        dir(&notes.join(".git"));
        let deep = dir(&notes.join("src/deep"));
        let here = candidate(&deep).unwrap();
        assert_eq!(here.root, notes);
        assert_eq!(here.name.as_str(), "notes");
        assert_eq!(here.path.as_str(), notes.to_string_lossy());
        assert_eq!(candidate(&notes).unwrap().root, notes);
        // A folder in no repo: nothing to offer.
        let plain = dir(&tmp.path().join("plain"));
        assert_eq!(candidate(&plain), None);
        // A folder that does not exist: nothing either.
        assert_eq!(candidate(&tmp.path().join("gone")), None);
    }

    /// The real path: a folder reached through a symlink is offered where it really is.
    #[cfg(unix)]
    #[test]
    fn candidate_root_follows_the_real_path() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = dir(&tmp.path().join("notes"));
        dir(&notes.join(".git"));
        let src = dir(&notes.join("src"));
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        assert_eq!(candidate(&link).unwrap().root, notes);
    }

    /// A linked worktree (its `.git` is a file) is offered at its own root, which `repo.add`
    /// takes, and named after the repo it belongs to. A submodule-like `.git` file without
    /// `commondir` keeps its own name.
    #[test]
    fn candidate_root_of_a_linked_worktree_names_its_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = dir(&tmp.path().join("notes"));
        let admin = dir(&notes.join(".git/worktrees/wt"));
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        let wt = dir(&tmp.path().join("notes-wt"));
        std::fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", admin.to_string_lossy()),
        )
        .unwrap();
        let here = candidate(&dir(&wt.join("sub"))).unwrap();
        assert_eq!(here.root, wt);
        assert_eq!(here.name.as_str(), "notes");
        assert_eq!(here.path.as_str(), wt.to_string_lossy());

        let module = dir(&tmp.path().join("lib"));
        let modules = dir(&notes.join(".git/modules/lib"));
        std::fs::write(
            module.join(".git"),
            format!("gitdir: {}\n", modules.to_string_lossy()),
        )
        .unwrap();
        let here = candidate(&module).unwrap();
        assert_eq!(here.root, module);
        assert_eq!(here.name.as_str(), "lib");
    }
}
