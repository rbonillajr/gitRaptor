//! The connection loop between the app and the engine, on its own thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_api::actor::Actor;
use gitraptor_api::discovery::CandidatesResult;
use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS, visible};
use gitraptor_api::messages::{
    ConnectionProfile, HelloResult, McpSnapshot, ResyncNotification, ResyncReason as ApiResync,
    Snapshot,
};
use gitraptor_api::methods::{self, CAP_DISCOVERY_EVENTS, CAP_SCOPE_ACTIVITY};
use gitraptor_api::rpc::Notification;
use gitraptor_api::scope::{
    ConnectionRequester, Scope, ScopeEventNotification, ScopeResyncNotification, ScopeSnapshot,
    ScopeSnapshotParams, ScopeSubscribeParams,
};
use serde_json::{Value, json};

use super::ingest;
use super::link::{
    ConnectFailure, Connector, Incompatibility, IncompatibilityKind, Link, LinkError,
    REQUIRED_CAPABILITIES,
};
use super::seq::{SeqTrack, Verdict};
use super::wire::{
    CandidateDto, ConnState, EngineMessage, IncompatibleAction, IncompatibleWhich, ResyncReason,
    StoppedReason, stop_command,
};

/// Where the supervisor delivers its messages.
pub enum Sink {
    Channel(tauri::ipc::Channel<EngineMessage>),
    Collect(Sender<EngineMessage>),
}

impl Sink {
    /// Delivers `msg`; a listener that is gone is ignored (the next `watch` replaces it).
    pub fn send(&self, msg: EngineMessage) {
        match self {
            Self::Channel(channel) => drop(channel.send(msg)),
            Self::Collect(tx) => drop(tx.send(msg)),
        }
    }
}

/// An order for the supervisor thread.
pub enum HubCmd {
    Watch(Sink),
    Retry { start: bool },
    Open { repo_id: String },
    Shutdown,
}

/// The running supervisor: dropping it shuts the thread down and waits for it.
pub struct SupervisorHandle {
    tx: Sender<HubCmd>,
    thread: Option<JoinHandle<()>>,
}

impl SupervisorHandle {
    /// Sends an order; `false` when the thread is gone.
    pub fn send(&self, cmd: HubCmd) -> bool {
        self.tx.send(cmd).is_ok()
    }

    /// Shuts the thread down and waits for it.
    pub fn join(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        drop(self.tx.send(HubCmd::Shutdown));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the supervisor thread.
pub fn spawn(
    connector: Box<dyn Connector>,
    own_raptor: Option<PathBuf>,
    sink: Sink,
) -> SupervisorHandle {
    let (tx, rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("gitraptor-engine".to_owned())
        .spawn(move || run(connector, own_raptor, sink, rx))
        .ok();
    SupervisorHandle { tx, thread }
}

/// The wait before reconnection attempt `attempt` (1-based): 250 ms doubling, capped at 5 s.
pub fn backoff(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(5);
    Duration::from_millis(250 * (1u64 << exp)).min(Duration::from_secs(5))
}

/// How long the supervisor reads the link before it looks at its orders again.
const READ_SLICE: Duration = Duration::from_millis(20);

/// Failed attempts after a live connection that still read as "reconnecting", not "stopped".
const LOST_GRACE_ATTEMPTS: u32 = 4;

/// How a session with the engine ended.
enum End {
    /// The link closed: try again, starting the wait from the first step.
    Reconnect,
    /// The engine dropped this client (`events.resync`): the wait keeps growing.
    Streak,
    /// The engine cannot be used: show why, then keep trying.
    Failed(ConnectFailure),
    /// The user asked to connect now.
    Retry(bool),
    Shutdown,
}

/// What the supervisor knows between sessions: the listener and the last things it told it.
struct Ctx {
    sink: Sink,
    own_raptor: Option<PathBuf>,
    last_conn: Option<EngineMessage>,
    last_session: Option<EngineMessage>,
    last_engine_only: Option<EngineMessage>,
    discovered: Vec<CandidateDto>,
    /// Wall time (ms) of the last `live`, while it lasts.
    live_wall: Option<u64>,
    /// Wall time (ms) since which the data may be old.
    stale: Option<u64>,
    resync: Option<ResyncReason>,
}

impl Ctx {
    fn send(&mut self, msg: EngineMessage) {
        match &msg {
            EngineMessage::Discovered {
                candidates,
                replace,
            } => {
                if *replace {
                    self.discovered.clear();
                }
                self.discovered.extend(candidates.iter().cloned());
            }
            EngineMessage::EngineOnly { .. } => self.last_engine_only = Some(msg.clone()),
            EngineMessage::Session { .. } => {
                self.last_engine_only = None;
                self.discovered.clear();
                self.last_session = Some(msg.clone());
            }
            _ => {}
        }
        self.sink.send(msg);
    }

    /// Tells the connection state, unless it is the one the listener already has.
    fn show(&mut self, state: ConnState) {
        let msg = EngineMessage::Conn {
            state,
            stale_since_ms: self.stale,
            resync: self.resync,
        };
        if self.last_conn.as_ref() == Some(&msg) {
            return;
        }
        self.last_conn = Some(msg.clone());
        self.sink.send(msg);
    }

    fn go_live(&mut self) {
        self.stale = None;
        self.resync = None;
        self.live_wall = Some(wall_ms());
        self.show(ConnState::Live);
    }

    /// The link is gone: whatever was shown may be old since the last `live`.
    fn lost(&mut self) {
        if let Some(since) = self.live_wall.take() {
            self.stale = Some(since);
        }
    }

    /// A new listener (the webview reloaded): it gets what it missed.
    fn on_watch(&mut self, sink: Sink) {
        self.sink = sink;
        for msg in [
            self.last_session.clone(),
            self.last_conn.clone(),
            self.last_engine_only.clone(),
        ]
        .into_iter()
        .flatten()
        {
            self.sink.send(msg);
        }
    }

    fn resend_discovered(&mut self) {
        if !self.discovered.is_empty() {
            let candidates = self.discovered.clone();
            self.sink.send(EngineMessage::Discovered {
                candidates,
                replace: true,
            });
        }
    }
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

fn millis(wait: Duration) -> u64 {
    u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)
}

fn run(
    mut connector: Box<dyn Connector>,
    own_raptor: Option<PathBuf>,
    sink: Sink,
    rx: Receiver<HubCmd>,
) {
    let mut ctx = Ctx {
        sink,
        own_raptor,
        last_conn: None,
        last_session: None,
        last_engine_only: None,
        discovered: Vec::new(),
        live_wall: None,
        stale: None,
        resync: None,
    };
    let mut attempt: u32 = 0;
    // The engine is started only when the app opens and when the user asks (D5).
    let mut start = true;
    loop {
        if start {
            ctx.show(ConnState::Starting);
        } else if attempt == 0 {
            ctx.show(ConnState::Connecting);
        }
        let ended = match connector.connect(std::mem::take(&mut start)) {
            Ok(mut link) => session(link.as_mut(), &mut ctx, &rx),
            Err(failure) => End::Failed(failure),
        };
        ctx.lost();
        let failure = match ended {
            End::Shutdown => return,
            End::Retry(starts) => {
                attempt = 0;
                start = starts;
                continue;
            }
            End::Reconnect => {
                attempt = 0;
                None
            }
            End::Streak => None,
            End::Failed(failure) => Some(failure),
        };
        attempt = attempt.saturating_add(1);
        let state = match failure {
            Some(ConnectFailure::Unsupported) => {
                ctx.show(ConnState::Unsupported);
                wait_for_shutdown(&mut ctx, &rx);
                return;
            }
            // An engine that was live and is gone may be restarting (an update, the login
            // autostart): only after a few attempts is it "stopped".
            Some(ConnectFailure::NotRunning)
                if ctx.stale.is_some() && attempt <= LOST_GRACE_ATTEMPTS =>
            {
                ConnState::Reconnecting {
                    attempt,
                    retry_in_ms: millis(backoff(attempt)),
                }
            }
            Some(ConnectFailure::NotRunning) => ConnState::Stopped {
                reason: StoppedReason::NotRunning,
            },
            Some(ConnectFailure::LaunchFailed) => ConnState::Stopped {
                reason: StoppedReason::LaunchFailed,
            },
            Some(ConnectFailure::Incompatible(inc)) => {
                incompatible(&inc, ctx.own_raptor.as_deref())
            }
            Some(ConnectFailure::Rejected) => ConnState::Rejected,
            Some(ConnectFailure::Lost) | None => ConnState::Reconnecting {
                attempt,
                retry_in_ms: millis(backoff(attempt)),
            },
        };
        ctx.show(state);
        match wait(&mut ctx, &rx, backoff(attempt)) {
            Wake::Shutdown => return,
            Wake::Retry(starts) => {
                attempt = 0;
                start = starts;
            }
            Wake::Elapsed => {}
        }
    }
}

enum Wake {
    Elapsed,
    Retry(bool),
    Shutdown,
}

/// Waits `delay` for an order that cuts it short.
fn wait(ctx: &mut Ctx, rx: &Receiver<HubCmd>, delay: Duration) -> Wake {
    let deadline = Instant::now() + delay;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(HubCmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => return Wake::Shutdown,
            Ok(HubCmd::Retry { start }) => return Wake::Retry(start),
            Ok(HubCmd::Watch(sink)) => ctx.on_watch(sink),
            Ok(HubCmd::Open { .. }) => {}
            Err(RecvTimeoutError::Timeout) => return Wake::Elapsed,
        }
    }
}

fn wait_for_shutdown(ctx: &mut Ctx, rx: &Receiver<HubCmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            HubCmd::Shutdown => return,
            HubCmd::Watch(sink) => ctx.on_watch(sink),
            HubCmd::Retry { .. } | HubCmd::Open { .. } => {}
        }
    }
}

/// The state of a daemon the app cannot use, with what the user can do about it (D7). Texts
/// from the daemon are sanitized here; the stop command is built from the raw path.
fn incompatible(inc: &Incompatibility, own_raptor: Option<&Path>) -> ConnState {
    let which = match inc.which {
        IncompatibilityKind::DaemonOlder => IncompatibleWhich::DaemonOlder,
        IncompatibilityKind::AppOlder => IncompatibleWhich::AppOlder,
        IncompatibilityKind::MissingCapabilities => IncompatibleWhich::MissingCapabilities,
    };
    let stop_command = inc.daemon_path.as_deref().and_then(stop_command);
    let own = is_own_engine(inc.daemon_path.as_deref(), own_raptor);
    let action = if own && which != IncompatibleWhich::AppOlder {
        IncompatibleAction::Start
    } else if which == IncompatibleWhich::AppOlder {
        IncompatibleAction::UpdateApp
    } else if stop_command.is_some() {
        IncompatibleAction::CopyStop
    } else {
        IncompatibleAction::None
    };
    ConnState::Incompatible {
        which,
        daemon_version: inc
            .daemon_version
            .as_deref()
            .map(|v| visible(v, NAME_MAX_CHARS)),
        daemon_path: inc
            .daemon_path
            .as_deref()
            .map(|p| visible(p, TEXT_MAX_CHARS)),
        stop_command: stop_command.filter(|_| action != IncompatibleAction::Start),
        action,
    }
}

/// Whether the daemon runs the app's own `raptor`. Fails closed: a path that cannot be resolved
/// is not the app's own.
fn is_own_engine(daemon_path: Option<&str>, own_raptor: Option<&Path>) -> bool {
    let (Some(daemon), Some(own)) = (daemon_path, own_raptor) else {
        return false;
    };
    match (std::fs::canonicalize(daemon), std::fs::canonicalize(own)) {
        (Ok(daemon), Ok(own)) => daemon == own,
        _ => false,
    }
}

fn resync_reason(reason: ApiResync) -> ResyncReason {
    match reason {
        ApiResync::SlowConsumer => ResyncReason::SlowConsumer,
        ApiResync::ReplayUnavailable => ResyncReason::ReplayUnavailable,
        ApiResync::DaemonRestarted => ResyncReason::DaemonRestarted,
        ApiResync::ScopeClosed => ResyncReason::ScopeClosed,
    }
}

/// Whether the window must not show repos: an agent opened it, or it is not known who did
/// (D6, fails closed).
fn is_restricted(hello: &HelloResult) -> bool {
    hello.profile == ConnectionProfile::Mcp
        || match &hello.requester {
            Some(ConnectionRequester::Resolved { actor, .. }) => {
                matches!(actor, Actor::Agent { .. })
            }
            Some(ConnectionRequester::Unverified) | None => true,
        }
}

/// One connection: what the daemon says, a snapshot, and the stream until the link drops.
fn session(link: &mut dyn Link, ctx: &mut Ctx, rx: &Receiver<HubCmd>) -> End {
    let hello = link.hello().clone();
    let known = hello.capabilities.clone().unwrap_or_default();
    let has = |name: &str| known.iter().any(|c| c == name);
    let missing: Vec<String> = REQUIRED_CAPABILITIES
        .iter()
        .filter(|name| !has(name))
        .map(|name| (*name).to_owned())
        .collect();
    if !missing.is_empty() {
        return End::Failed(ConnectFailure::Incompatible(Incompatibility {
            which: IncompatibilityKind::MissingCapabilities,
            daemon_version: Some(hello.binary_version),
            daemon_path: None,
            missing,
        }));
    }
    let restricted = is_restricted(&hello);
    ctx.send(EngineMessage::Session {
        daemon_version: visible(&hello.binary_version, NAME_MAX_CHARS),
        protocol: hello.protocol,
        restricted,
        activity: has(CAP_SCOPE_ACTIVITY.name),
        discovery: has(CAP_DISCOVERY_EVENTS.name),
    });
    ctx.show(ConnState::Syncing);
    let mut track = SeqTrack::default();
    if restricted {
        if restricted_snapshot(link, ctx, hello.profile).is_err() {
            return End::Reconnect;
        }
    } else {
        if sync_global(link, ctx, &mut track, true).is_err() {
            return End::Reconnect;
        }
        if has(CAP_DISCOVERY_EVENTS.name) {
            match link.call(methods::DISCOVERY_CANDIDATES, json!({})) {
                Ok(value) => {
                    if let Ok(result) = serde_json::from_value::<CandidatesResult>(value) {
                        ctx.send(EngineMessage::Discovered {
                            candidates: result.candidates.iter().map(ingest::candidate).collect(),
                            replace: true,
                        });
                    }
                }
                Err(LinkError::Refused { .. }) => {}
                Err(LinkError::Lost) => return End::Reconnect,
            }
        }
    }
    ctx.go_live();
    stream(link, ctx, rx, &mut track, restricted)
}

/// The engine state alone, for a window that must not see repos.
fn restricted_snapshot(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    profile: ConnectionProfile,
) -> Result<(), LinkError> {
    let value = link.call(methods::ENGINE_SNAPSHOT, json!({}))?;
    let engine = match profile {
        ConnectionProfile::Mcp => serde_json::from_value::<McpSnapshot>(value)
            .map(|s| ingest::engine_state(s.engine_state, None)),
        ConnectionProfile::Full => {
            serde_json::from_value::<Snapshot>(value).map(|s| ingest::engine(&s.engine))
        }
    }
    .map_err(|_| LinkError::Lost)?;
    ctx.send(EngineMessage::EngineOnly { engine });
    Ok(())
}

/// A snapshot of the global scope and, when `subscribe`, a subscription from the next sequence
/// of the same daemon run.
fn sync_global(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    track: &mut SeqTrack,
    subscribe: bool,
) -> Result<(), LinkError> {
    let params = to_value(&ScopeSnapshotParams {
        scope: Scope::Global,
    })?;
    let value = link.call(methods::SCOPE_SNAPSHOT, params)?;
    let Ok(ScopeSnapshot::Global(snapshot)) = serde_json::from_value::<ScopeSnapshot>(value) else {
        return Err(LinkError::Lost);
    };
    ctx.send(EngineMessage::Global(ingest::global(&snapshot)));
    track.reset(&snapshot.run_id, snapshot.scope_seq);
    if subscribe {
        let params = to_value(&ScopeSubscribeParams {
            scope: Scope::Global,
            from_seq: Some(snapshot.scope_seq + 1),
            run_id: Some(snapshot.run_id),
        })?;
        link.call(methods::SCOPE_SUBSCRIBE, params)?;
    }
    Ok(())
}

fn to_value<T: serde::Serialize>(params: &T) -> Result<Value, LinkError> {
    serde_json::to_value(params).map_err(|_| LinkError::Lost)
}

/// Reads the link in slices, attending the orders between them, until it ends.
fn stream(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    rx: &Receiver<HubCmd>,
    track: &mut SeqTrack,
    restricted: bool,
) -> End {
    loop {
        loop {
            match rx.try_recv() {
                Ok(HubCmd::Shutdown) | Err(TryRecvError::Disconnected) => return End::Shutdown,
                Ok(HubCmd::Retry { start }) => return End::Retry(start),
                Ok(HubCmd::Watch(sink)) => {
                    ctx.on_watch(sink);
                    if !restricted {
                        if sync_global(link, ctx, track, false).is_err() {
                            return End::Reconnect;
                        }
                        ctx.resend_discovered();
                    }
                }
                Ok(HubCmd::Open { .. }) => {}
                Err(TryRecvError::Empty) => break,
            }
        }
        match link.next(READ_SLICE) {
            Ok(None) => {}
            Ok(Some((_recv_ns, notification))) => {
                if !restricted && let Some(end) = apply(link, ctx, track, &notification) {
                    return end;
                }
            }
            Err(_) => return End::Reconnect,
        }
    }
}

/// Applies one notification of the stream; `Some` when the session must end.
fn apply(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    track: &mut SeqTrack,
    notification: &Notification,
) -> Option<End> {
    match notification.method.as_str() {
        methods::NOTIFY_SCOPE_EVENT => {
            let Ok(event) =
                serde_json::from_value::<ScopeEventNotification>(notification.params.clone())
            else {
                return None;
            };
            if event.scope != Scope::Global {
                return None;
            }
            match track.accept(event.scope_seq) {
                Verdict::Apply => {
                    for msg in ingest::global_event(&event.event) {
                        ctx.send(msg);
                    }
                }
                Verdict::Duplicate | Verdict::Drop => {}
                Verdict::Gap => return resync(link, ctx, track),
            }
        }
        methods::NOTIFY_SCOPE_RESYNC => {
            let Ok(resync_note) =
                serde_json::from_value::<ScopeResyncNotification>(notification.params.clone())
            else {
                return None;
            };
            if resync_note.scope == Scope::Global {
                ctx.resync = Some(resync_reason(resync_note.reason));
                return resync(link, ctx, track);
            }
        }
        methods::NOTIFY_RESYNC => {
            if let Ok(note) =
                serde_json::from_value::<ResyncNotification>(notification.params.clone())
            {
                ctx.resync = Some(resync_reason(note.reason));
            }
            // The engine is about to close this connection: the wait must keep growing.
            return Some(End::Streak);
        }
        _ => {}
    }
    None
}

/// Takes the global scope again and goes back to `live`.
fn resync(link: &mut dyn Link, ctx: &mut Ctx, track: &mut SeqTrack) -> Option<End> {
    ctx.show(ConnState::Syncing);
    if sync_global(link, ctx, track, true).is_err() {
        return Some(End::Reconnect);
    }
    ctx.go_live();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_250_ms_and_caps_at_5_s() {
        let ms: Vec<u128> = (1..=8).map(|n| backoff(n).as_millis()).collect();
        assert_eq!(ms, [250, 500, 1000, 2000, 4000, 5000, 5000, 5000]);
        assert_eq!(backoff(0).as_millis(), 250);
        assert_eq!(backoff(u32::MAX).as_millis(), 5000);
    }
}
