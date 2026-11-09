//! The connection loop between the app and the engine, on its own thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_api::actor::Actor;
use gitraptor_api::clock;
use gitraptor_api::discovery::CandidatesResult;
use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS, visible};
use gitraptor_api::messages::{
    ConnectionProfile, HelloResult, McpSnapshot, ResyncNotification, ResyncReason as ApiResync,
    SessionsListParams, SessionsListResult, Snapshot, UnsubscribeParams,
};
use gitraptor_api::methods::{self, CAP_DISCOVERY_EVENTS, CAP_SCOPE_ACTIVITY};
use gitraptor_api::prediction::{
    ListPredictionParams, PairDetail, PredictionChanged, PredictionPage,
};
use gitraptor_api::rpc::Notification;
use gitraptor_api::scope::{
    ConnectionRequester, Scope, ScopeEventNotification, ScopeResyncNotification, ScopeSnapshot,
    ScopeSnapshotParams, ScopeSubscribeParams, ScopeSubscribeResult,
};
use serde_json::{Value, json};

use super::ingest;
use super::launcher;
use super::link::{
    ConnectFailure, Connector, Incompatibility, IncompatibilityKind, Link, LinkError,
    REQUIRED_CAPABILITIES,
};
use super::prediction::PredictionTracker;
use super::seq::{SeqTrack, Verdict};
use super::shell_command;
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
    GuardBlocks { repo_id: String },
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

    /// Asks the thread to end and lets go of it: an in-flight connect or call must not hold the
    /// process exit.
    pub fn shutdown_detached(mut self) {
        drop(self.tx.send(HubCmd::Shutdown));
        drop(self.thread.take());
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
    /// The last incompatibility shown, to keep what an attempt that lost its details had.
    last_incompat: Option<Incompatibility>,
    /// The repo the window follows (raw id): it survives reconnections until the engine drops it.
    open: Option<String>,
    /// Raw paths of the repos of the last global snapshot.
    global_roots: Vec<PathBuf>,
}

/// The open repo's scope on one connection.
#[derive(Default)]
struct RepoScope {
    track: SeqTrack,
    /// The engine's subscription id, to cancel it when the window moves to another repo.
    subscription: Option<u32>,
    /// The predicted pairs and which ones still lack their detail.
    tracker: PredictionTracker,
    /// Since when something is waiting to be asked of the engine (the debounce of D2).
    pending_since: Option<Instant>,
    /// Changes every time the scope is taken again: a cursor walk that sees it move is stale, so
    /// two walks never run at once.
    generation: u64,
    /// Raw paths of the open repo and its worktrees.
    roots: Vec<PathBuf>,
}

/// Wait after the first id that lacks its detail before asking for it.
const DETAIL_DEBOUNCE: Duration = Duration::from_millis(100);

/// Most pages and bytes of answers one cursor walk takes.
const WALK_MAX_PAGES: usize = 20;
const WALK_MAX_BYTES: usize = 2 * 1024 * 1024;

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

    /// An incompatibility that repeats the last one without its path keeps that path.
    fn remember_incompat(&mut self, mut inc: Incompatibility) -> Incompatibility {
        if let Some(prev) = &self.last_incompat
            && inc.daemon_path.is_none()
            && prev.which == inc.which
            && prev.daemon_version == inc.daemon_version
        {
            inc.daemon_path.clone_from(&prev.daemon_path);
        }
        self.last_incompat = Some(inc.clone());
        inc
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
        last_incompat: None,
        open: None,
        global_roots: Vec::new(),
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
                let inc = ctx.remember_incompat(inc);
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
            Ok(HubCmd::Open { repo_id }) => ctx.open = Some(repo_id),
            Ok(HubCmd::GuardBlocks { .. }) => {}
            Err(RecvTimeoutError::Timeout) => return Wake::Elapsed,
        }
    }
}

fn wait_for_shutdown(ctx: &mut Ctx, rx: &Receiver<HubCmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            HubCmd::Shutdown => return,
            HubCmd::Watch(sink) => ctx.on_watch(sink),
            HubCmd::Open { repo_id } => ctx.open = Some(repo_id),
            HubCmd::Retry { .. } | HubCmd::GuardBlocks { .. } => {}
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
    let mut repo = RepoScope::default();
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
    if !restricted && open_repo(link, ctx, &mut repo).is_err() {
        return End::Reconnect;
    }
    stream(link, ctx, rx, &mut track, &mut repo, restricted)
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
    ctx.global_roots = snapshot
        .repos
        .iter()
        .map(|repo| PathBuf::from(repo.path.raw()))
        .collect();
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
    repo: &mut RepoScope,
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
                Ok(HubCmd::Open { repo_id }) => {
                    ctx.open = Some(repo_id);
                    if !restricted && open_repo(link, ctx, repo).is_err() {
                        return End::Reconnect;
                    }
                }
                Ok(HubCmd::GuardBlocks { .. }) => {}
                Err(TryRecvError::Empty) => break,
            }
        }
        let idle = match link.next(READ_SLICE) {
            Ok(None) => true,
            Ok(Some((recv_ns, notification))) => {
                if !restricted
                    && let Some(end) = apply(link, ctx, (track, repo), recv_ns, &notification)
                {
                    return end;
                }
                false
            }
            Err(_) => return End::Reconnect,
        };
        if repo.pending_since.is_none() && repo.tracker.has_work() {
            repo.pending_since = Some(Instant::now());
        }
        let due = repo
            .pending_since
            .is_some_and(|since| idle || since.elapsed() >= DETAIL_DEBOUNCE);
        if due && let Some(end) = fetch_prediction(link, ctx, (track, repo)) {
            return end;
        }
    }
}

/// Asks the engine for what the pairs still lack: the rest of a truncated snapshot first, then
/// the detail of the conflict and overlap pairs. `Some` when the link is lost.
fn fetch_prediction(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    (track, repo): (&mut SeqTrack, &mut RepoScope),
) -> Option<End> {
    repo.pending_since = None;
    let repo_id = ctx.open.clone()?;
    if repo.tracker.needs_fill() {
        let generation = repo.generation;
        if let Some(end) = fill_prediction(link, ctx, (track, repo), &repo_id) {
            return Some(end);
        }
        if repo.generation != generation {
            return None;
        }
    }
    loop {
        let ids = repo.tracker.wanted();
        if ids.is_empty() {
            return None;
        }
        repo.tracker.asked(&ids);
        let params = ListPredictionParams {
            repo_id: repo_id.clone(),
            pair_ids: Some(ids),
            ..ListPredictionParams::default()
        };
        match list_prediction(link, &params) {
            Ok((page, _)) => deliver_details(ctx, repo, &repo_id, &page.pairs, false),
            // The pairs stay without detail until their `rev` moves.
            Err(LinkError::Refused { .. }) => {}
            Err(LinkError::Lost) => return Some(End::Reconnect),
        }
    }
}

/// One `prediction.list` call and the bytes of its answer.
fn list_prediction(
    link: &mut dyn Link,
    params: &ListPredictionParams,
) -> Result<(PredictionPage, usize), LinkError> {
    let value = link.call(methods::PREDICTION_LIST, to_value(params)?)?;
    let size = value.to_string().len();
    serde_json::from_value::<PredictionPage>(value)
        .map(|page| (page, size))
        .map_err(|_| LinkError::Refused { code: 0 })
}

/// Remembers `details` and sends them to the window; an empty page is sent only when it closes a
/// walk.
fn deliver_details(
    ctx: &mut Ctx,
    repo: &mut RepoScope,
    repo_id: &str,
    details: &[PairDetail],
    filled: bool,
) {
    repo.tracker.adopt(details);
    if !details.is_empty() || filled {
        ctx.send(ingest::pair_details(
            repo_id,
            details,
            filled,
            clock::monotonic_ns(),
        ));
    }
}

/// Walks the cursor of `prediction.list` for the pairs a truncated snapshot left out: at most
/// [`WALK_MAX_PAGES`] pages and [`WALK_MAX_BYTES`] of answers. Only a walk that ends for lack of a
/// next cursor says `filled`. Between pages it attends what the engine sent; if that took the
/// scope again (a resync), the walk stops there and the next one starts from the new snapshot.
fn fill_prediction(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    (track, repo): (&mut SeqTrack, &mut RepoScope),
    repo_id: &str,
) -> Option<End> {
    let generation = repo.generation;
    let mut cursor: Option<String> = None;
    let mut bytes = 0usize;
    for _ in 0..WALK_MAX_PAGES {
        let params = ListPredictionParams {
            repo_id: repo_id.to_owned(),
            cursor: cursor.take(),
            ..ListPredictionParams::default()
        };
        let (page, size) = match list_prediction(link, &params) {
            Ok(answer) => answer,
            Err(LinkError::Refused { .. }) => break,
            Err(LinkError::Lost) => return Some(End::Reconnect),
        };
        bytes = bytes.saturating_add(size);
        let finished = page.next_cursor.is_none();
        deliver_details(ctx, repo, repo_id, &page.pairs, finished);
        if finished || bytes >= WALK_MAX_BYTES {
            break;
        }
        cursor = page.next_cursor;
        match link.next(Duration::from_millis(1)) {
            Ok(None) => {}
            Ok(Some((recv_ns, notification))) => {
                if let Some(end) = apply(link, ctx, (track, repo), recv_ns, &notification) {
                    return Some(end);
                }
                if repo.generation != generation || ctx.open.as_deref() != Some(repo_id) {
                    return None;
                }
            }
            Err(_) => return Some(End::Reconnect),
        }
    }
    repo.tracker.walk_done();
    None
}

/// Applies one notification of the stream; `Some` when the session must end.
fn apply(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    (track, repo): (&mut SeqTrack, &mut RepoScope),
    recv_ns: u64,
    notification: &Notification,
) -> Option<End> {
    match notification.method.as_str() {
        methods::NOTIFY_SCOPE_EVENT => {
            let event =
                serde_json::from_value::<ScopeEventNotification>(notification.params.clone())
                    .ok()?;
            match &event.scope {
                Scope::Global => apply_global_event(link, ctx, track, &event),
                Scope::Repo { repo_id } if ctx.open.as_deref() == Some(repo_id) => {
                    apply_repo_event(link, ctx, repo, recv_ns, &event)
                }
                Scope::Repo { .. } => None,
            }
        }
        methods::NOTIFY_SCOPE_RESYNC => {
            let note =
                serde_json::from_value::<ScopeResyncNotification>(notification.params.clone())
                    .ok()?;
            match &note.scope {
                Scope::Global => {
                    ctx.resync = Some(resync_reason(note.reason));
                    resync(link, ctx, track, true)
                }
                Scope::Repo { repo_id } if ctx.open.as_deref() == Some(repo_id) => {
                    if note.reason == ApiResync::ScopeClosed {
                        close_repo(ctx, repo);
                        None
                    } else {
                        reopen(link, ctx, repo)
                    }
                }
                Scope::Repo { .. } => None,
            }
        }
        methods::NOTIFY_RESYNC => {
            if let Ok(note) =
                serde_json::from_value::<ResyncNotification>(notification.params.clone())
            {
                ctx.resync = Some(resync_reason(note.reason));
            }
            // The engine is about to close this connection: the wait must keep growing.
            Some(End::Streak)
        }
        _ => None,
    }
}

fn apply_global_event(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    track: &mut SeqTrack,
    event: &ScopeEventNotification,
) -> Option<End> {
    match track.accept(event.scope_seq) {
        Verdict::Apply => {
            for msg in ingest::global_event(&event.event) {
                ctx.send(msg);
            }
            None
        }
        Verdict::Duplicate | Verdict::Drop => None,
        Verdict::Gap => resync(link, ctx, track, false),
    }
}

/// An event of the open repo: the kinds the fleet shows go to the window, the rest only advance
/// the sequence.
fn apply_repo_event(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    repo: &mut RepoScope,
    recv_ns: u64,
    event: &ScopeEventNotification,
) -> Option<End> {
    match repo.track.accept(event.scope_seq) {
        Verdict::Apply => {
            if event.event.kind == methods::PREDICTION_CHANGED
                && let Ok(changed) =
                    serde_json::from_value::<PredictionChanged>(event.event.data.clone())
            {
                repo.tracker.apply(&changed);
            }
            if let Some(msg) = ingest::repo_event(&event.event, recv_ns) {
                ctx.send(msg);
            }
            None
        }
        Verdict::Duplicate | Verdict::Drop => None,
        Verdict::Gap => reopen(link, ctx, repo),
    }
}

/// Takes the open repo again; `Some` when the link is lost.
fn reopen(link: &mut dyn Link, ctx: &mut Ctx, repo: &mut RepoScope) -> Option<End> {
    open_repo(link, ctx, repo).err().map(|_| End::Reconnect)
}

/// The window no longer follows the repo: tell it and forget it. It cancels nothing: after a
/// `scope-closed` the engine closed the scope itself, and after a refusal the caller already
/// left the subscription.
fn close_repo(ctx: &mut Ctx, repo: &mut RepoScope) {
    if let Some(repo_id) = ctx.open.take() {
        ctx.send(EngineMessage::RepoClosed {
            repo_id: visible(&repo_id, NAME_MAX_CHARS),
        });
    }
    *repo = RepoScope {
        generation: repo.generation.wrapping_add(1),
        ..RepoScope::default()
    };
}

/// Follows the open repo on this connection: leaves the previous subscription, takes a snapshot,
/// subscribes from the next sequence and lists the sessions. A repo the engine refuses is
/// closed; only a lost link is an error.
fn open_repo(link: &mut dyn Link, ctx: &mut Ctx, repo: &mut RepoScope) -> Result<(), LinkError> {
    let Some(repo_id) = ctx.open.clone() else {
        return Ok(());
    };
    leave_repo(link, repo)?;
    match take_repo(link, ctx, repo, &repo_id) {
        // The engine may have kept a subscription taken before the refusal: cancel it.
        Err(LinkError::Refused { .. }) => {
            leave_repo(link, repo)?;
            close_repo(ctx, repo);
            Ok(())
        }
        other => other,
    }
}

/// Cancels the subscription to the previous scope, if any (a refusal changes nothing).
fn leave_repo(link: &mut dyn Link, repo: &mut RepoScope) -> Result<(), LinkError> {
    repo.track = SeqTrack::Waiting;
    let Some(subscription) = repo.subscription.take() else {
        return Ok(());
    };
    let params = to_value(&UnsubscribeParams { subscription })?;
    match link.call(methods::EVENTS_UNSUBSCRIBE, params) {
        Ok(_) | Err(LinkError::Refused { .. }) => Ok(()),
        Err(LinkError::Lost) => Err(LinkError::Lost),
    }
}

fn take_repo(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    repo: &mut RepoScope,
    repo_id: &str,
) -> Result<(), LinkError> {
    let scope = Scope::Repo {
        repo_id: repo_id.to_owned(),
    };
    let params = to_value(&ScopeSnapshotParams {
        scope: scope.clone(),
    })?;
    let value = link.call(methods::SCOPE_SNAPSHOT, params)?;
    let Ok(ScopeSnapshot::Repo(snapshot)) = serde_json::from_value::<ScopeSnapshot>(value) else {
        return Err(LinkError::Lost);
    };
    repo.roots = std::iter::once(snapshot.repo.path.raw())
        .chain(snapshot.repo.worktrees.iter().map(|w| w.path.raw()))
        .map(PathBuf::from)
        .collect();
    let raptor = {
        let watched: Vec<&Path> = ctx
            .global_roots
            .iter()
            .chain(&repo.roots)
            .map(PathBuf::as_path)
            .collect();
        shell_command::raptor_path(
            link.daemon_exe(),
            launcher::bundled_raptor().as_deref(),
            &watched,
        )
    };
    repo.tracker.reset(snapshot.repo.prediction.as_ref());
    repo.generation = repo.generation.wrapping_add(1);
    repo.pending_since = None;
    ctx.send(EngineMessage::Repo {
        fleet: ingest::repo(&snapshot, raptor.as_deref()),
    });
    let params = to_value(&ScopeSubscribeParams {
        scope,
        from_seq: Some(snapshot.scope_seq + 1),
        run_id: Some(snapshot.run_id.clone()),
    })?;
    let value = link.call(methods::SCOPE_SUBSCRIBE, params)?;
    let subscribed =
        serde_json::from_value::<ScopeSubscribeResult>(value).map_err(|_| LinkError::Lost)?;
    repo.subscription = Some(subscribed.subscription);
    repo.track.reset(&snapshot.run_id, snapshot.scope_seq);
    list_sessions(link, ctx, repo_id)
}

fn list_sessions(link: &mut dyn Link, ctx: &mut Ctx, repo_id: &str) -> Result<(), LinkError> {
    let params = to_value(&SessionsListParams {
        repo_id: Some(repo_id.to_owned()),
        include_ended: false,
        limit: None,
    })?;
    let value = link.call(methods::SESSIONS_LIST, params)?;
    let list = serde_json::from_value::<SessionsListResult>(value).map_err(|_| LinkError::Lost)?;
    ctx.send(EngineMessage::Sessions {
        repo_id: visible(repo_id, NAME_MAX_CHARS),
        detection_available: list.detection_available,
        sessions: list.sessions.iter().filter_map(ingest::session).collect(),
    });
    Ok(())
}

/// Takes the global scope again and goes back to `live`.
///
/// Only `scope.resync` subscribes again: after a gap the subscription is still open.
fn resync(
    link: &mut dyn Link,
    ctx: &mut Ctx,
    track: &mut SeqTrack,
    subscribe: bool,
) -> Option<End> {
    ctx.show(ConnState::Syncing);
    if sync_global(link, ctx, track, subscribe).is_err() {
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
