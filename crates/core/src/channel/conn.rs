//! One client connection: accept checks, handshake, dispatch and output.

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::capability::{self, AcceptParams, AcceptResult, CAPABILITIES_PROTOCOL};
use gitraptor_api::catalog::{
    self as catalog, CancelParams, CancelResult, Layer, PrepareParams, PrepareResult, RejectedData,
    RunParams,
};
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::framing::{FrameError, MAX_MESSAGE_BYTES, decode_request, read_frame};
use gitraptor_api::guard::{EvaluateParams, GuardRejectedData, GuardRepoParams};
use gitraptor_api::mcp_view::{MCP_READ_BURST, MCP_READS_PER_MINUTE};
use gitraptor_api::messages::{
    AuditEntry, AuditListParams, AuditListResult, AuditOutcome, ClientIdentity, ClientKind,
    ConnectionProfile, DeclaredAgent, EventsHistoryParams, EventsHistoryResult, Hello, HelloResult,
    IncompatibleData, McpRepoView, McpSnapshot, NoParams, RefusalReason, RefusedData,
    RegistrationRegisterParams, RegistrationRegisterResult, RegistrationRejectedData,
    RegistrationRejection, RegistrationWithdrawParams, RegistrationWithdrawResult, ReplaceParams,
    RepoAddParams, RepoAddResult, RepoRejectedData, RepoRejection, RepoRetireParams,
    RepoRetireResult, Snapshot, StopResult, SubscribeParams, SubscribeResult, UnsubscribeParams,
};
use gitraptor_api::messages::{
    GitEventKind, HeadView, MAX_HISTORY_PAGE, MAX_SESSIONS_PAGE, SessionsListParams, WorktreeStatus,
};
use gitraptor_api::methods::{self, METHODS, MethodSpec};
use gitraptor_api::rpc::{
    ErrorObject, Id, InvalidData, InvalidReason, Request, Response, ScopeRefusal, ScopeRefusedData,
    code,
};
use gitraptor_api::scope::{
    AttentionView, AutostartView, ConnectionRequester, GlobalSnapshot, RepoLocateParams,
    RepoLocateResult, RepoSnapshot, RepoSummaryView, Scope, ScopeSnapshot, ScopeSnapshotParams,
    ScopeSubscribeParams, ScopeSubscribeResult,
};
use gitraptor_api::timemachine::EntryOrigin;
use gitraptor_api::timemachine::{
    Invalid as TmInvalid, MAX_REPORTED_PATHS, MAX_REPORTED_REFS, McpRequesterView, NotRestored,
    NotRestoredReason, OperationRunResult, PriorFailedData, RedoParams, RequestChannel,
    RequesterView, ResolveParams, RestoreParams, RestoreResult, SnapshotParams, Surface,
    TIMELINE_DEFAULT_LIMIT, TimelineParams, TmRejectedData, UndoParams, UndoResult, parse_since,
};
use gitraptor_git::{ReaderOptions, RepoReader};

use super::authz::{AcceptedPeer, ChainLink, Verdict, check_reserved};
use super::bus::{Outbox, Subscribed};
use super::peer::{ProcInfo, peer_cred, process_cwd};
use super::requester::{self, Resolution};
use super::transport::Stream;
use super::validate;
use super::{ServerCtx, file_id};
use crate::daemon::{
    CHANGE_LIST_BUDGET, Field, GuardLogReply, GuardReply, GuardRequest, RegisterRequest,
    RegistrationError, RepoAddRequest, RepoCommandError, StopCause, WithdrawRequest, now_ms,
};
use crate::executor::{
    Caller, ExecError, PrepareInput, RunEnv, RunInput, layer_for, oplog_channel,
};
use crate::profile::{Agent, AgentKind, AuditRow, Author};
use crate::timemachine::apply::ApplyReport;
use crate::timemachine::apply::{ApplyWarning, PathIssue};
use crate::timemachine::manual::{self, ManualError, QuotaHit};
use crate::timemachine::oplog::Requester;
use crate::timemachine::oplog::{AbsentStore, SnapshotRefs};
use crate::timemachine::protected::scope::{
    operation_in, require_attributed, scope_for, snapshot_in,
};
use crate::timemachine::protected::{
    ProtectedError, RepoHandle, ScopeError, failure_text, registered_worktrees, worktree_key,
};
use crate::timemachine::restore::{RestoreDone, RestoreError, restore_to};
use crate::timemachine::timeline::{
    AgentFilter, EngineSide, FILES_BUDGET, OplogRead, PathsCache, PathsSource, SessionActors,
    TimelineQuery, build_timeline_from, fill_files,
};
use crate::timemachine::undo::{RawSide, UndoDone, UndoEnv, UndoError, tm_scope_for, undo_last};

/// Longest audit page.
const MAX_AUDIT_PAGE: u32 = 500;

/// Reserved-command attempts per connection: one per second, bursts of
/// this many. Each attempt writes an audit row, so the audit cannot be
/// flooded from one connection.
const RESERVED_BURST: u32 = 5;

/// `daemon.replace` from anything but the installed binary: a reserved stop.
const REPLACE_AS_STOP: MethodSpec = MethodSpec {
    name: methods::DAEMON_REPLACE,
    reserved: true,
    mcp: false,
    implemented_by: None,
    writes: methods::RepoWrite::None,
    since: gitraptor_api::MIN_COMPATIBLE_PROTOCOL,
};

struct ConnEntry {
    id: u64,
    client: (u32, u64),
    stream: Stream,
    outbox: Arc<Outbox>,
    writer: Option<std::thread::JoinHandle<()>>,
}

/// Open connections.
#[derive(Default)]
pub(crate) struct ConnTable {
    next_id: u64,
    entries: Vec<ConnEntry>,
}

/// Checks a freshly accepted socket and, if it passes, serves it on its own
/// thread.
pub(crate) fn accept(ctx: &Arc<ServerCtx>, stream: Stream) {
    // On the clock of the start times it is compared with.
    let accepted_us = super::peer::proc_clock_us();
    let Ok(cred) = peer_cred(&stream) else {
        ctx.logger
            .warn("client_rejected", &[("reason", "peer-unreadable".into())]);
        return;
    };
    if cred.uid != ctx.uid {
        // SEC-01: another user. Closed before reading a single byte.
        ctx.logger
            .warn("client_rejected", &[("reason", "uid".into())]);
        return;
    }
    let info = ctx.procs.read(cred.pid).ok();
    let start_us = info.as_ref().map_or(0, |i| i.start_us);
    let limits = ctx.config.limits;
    let peer = AcceptedPeer {
        pid: cred.pid,
        start_us,
        accepted_us,
    };

    let total = ctx
        .conns
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries
        .len();
    // Past the general limit only the developer gets in: a client that would
    // pass the reserved checks (a pty alone is not enough, an agent can open
    // one), so a flood of agent connections cannot lock the developer out.
    let developer =
        total >= limits.max_connections && check_reserved(peer, &ctx.checks()).refused.is_none();
    let mut table = ctx.conns.lock().unwrap_or_else(|e| e.into_inner());
    let total = table.entries.len();
    let same_client = table
        .entries
        .iter()
        .filter(|c| c.client == (cred.pid, start_us))
        .count();
    let room = total < limits.max_connections
        || (developer && total < limits.max_connections + limits.terminal_slots);
    if !room || same_client >= limits.per_client {
        drop(table);
        ctx.logger
            .warn("client_rejected", &[("reason", "connection-limit".into())]);
        refuse_connection(&stream, ctx.config.limits.write_timeout);
        return;
    }
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let outbox = Outbox::new(limits.outbox);
    let writer = spawn_writer(&stream, Arc::clone(&outbox), limits.write_timeout);
    table.next_id += 1;
    let id = table.next_id;
    table.entries.push(ConnEntry {
        id,
        client: (cred.pid, start_us),
        stream,
        outbox: Arc::clone(&outbox),
        writer,
    });
    drop(table);

    let thread_ctx = Arc::clone(ctx);
    let spawned = std::thread::Builder::new()
        .name("raptor-conn".into())
        .spawn(move || {
            let mut conn = Connection {
                ctx: &thread_ctx,
                id,
                peer,
                info,
                outbox: Arc::clone(&outbox),
                profile: ConnectionProfile::Full,
                protocol: thread_ctx.config.protocol,
                capabilities: std::collections::BTreeSet::new(),
                accepted: false,
                phase: Phase::Handshake,
                subscriptions: Vec::new(),
                next_subscription: 1,
                bucket: Bucket::new(
                    thread_ctx.config.limits.rate_per_sec,
                    thread_ctx.config.limits.burst,
                ),
                reserved_bucket: Bucket::new(1, RESERVED_BURST),
                mcp_read_bucket: Bucket::per_minute(MCP_READS_PER_MINUTE, MCP_READ_BURST),
                mcp_write_bucket: std::sync::Mutex::new(Bucket::per_minute(
                    gitraptor_api::mcp_view::MCP_WRITES_PER_MINUTE,
                    gitraptor_api::mcp_view::MCP_WRITE_BURST,
                )),
            };
            conn.serve(reader_stream);
            if let Some(wiring) = &thread_ctx.protected {
                wiring.executor.drop_connection(id);
            }
            thread_ctx.bus.drop_outbox(&outbox);
            outbox.close();
            remove(&thread_ctx, id);
        });
    if spawned.is_err() {
        remove(ctx, id);
    }
}

fn refuse_connection(stream: &Stream, timeout: Duration) {
    let _ = stream.set_write_timeout(Some(timeout));
    let error = Response::err(
        None,
        ErrorObject::new(code::LIMIT_REACHED, "too many connections"),
    );
    if let Ok(line) = serde_json::to_string(&error) {
        let mut stream = stream;
        let _ = stream.write_all(line.as_bytes());
        let _ = stream.write_all(b"\n");
    }
}

fn spawn_writer(
    stream: &Stream,
    outbox: Arc<Outbox>,
    timeout: Duration,
) -> Option<std::thread::JoinHandle<()>> {
    let mut stream = stream.try_clone().ok()?;
    let _ = stream.set_write_timeout(Some(timeout));
    std::thread::Builder::new()
        .name("raptor-conn-out".into())
        .spawn(move || {
            while let Some(line) = outbox.next() {
                if stream.write_all(line.as_bytes()).is_err() || stream.write_all(b"\n").is_err() {
                    break;
                }
            }
            outbox.close();
            let _ = stream.shutdown(std::net::Shutdown::Both);
        })
        .ok()
}

fn remove(ctx: &ServerCtx, id: u64) {
    let entry = {
        let mut table = ctx.conns.lock().unwrap_or_else(|e| e.into_inner());
        let pos = table.entries.iter().position(|c| c.id == id);
        pos.map(|p| table.entries.remove(p))
    };
    if let Some(mut entry) = entry {
        entry.outbox.close();
        if let Some(writer) = entry.writer.take() {
            let _ = writer.join();
        }
    }
}

/// Closes every connection after its queued output is written, waiting at
/// most `grace` for the writers.
pub(crate) fn close_all(ctx: &ServerCtx, grace: Duration) {
    let entries: Vec<ConnEntry> = {
        let mut table = ctx.conns.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut table.entries)
    };
    for entry in &entries {
        entry.outbox.close();
        // Unblocks the reader thread.
        let _ = entry.stream.shutdown(std::net::Shutdown::Read);
    }
    let deadline = Instant::now() + grace;
    for mut entry in entries {
        if let Some(writer) = entry.writer.take() {
            while !writer.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if writer.is_finished() {
                let _ = writer.join();
            }
        }
        let _ = entry.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Token bucket (SEC-08).
struct Bucket {
    tokens: f64,
    rate: f64,
    burst: f64,
    last: Instant,
}

impl Bucket {
    fn new(rate_per_sec: u32, burst: u32) -> Self {
        Self {
            tokens: f64::from(burst),
            rate: f64::from(rate_per_sec),
            burst: f64::from(burst),
            last: Instant::now(),
        }
    }

    fn per_minute(rate_per_min: u32, burst: u32) -> Self {
        Self {
            rate: f64::from(rate_per_min) / 60.0,
            ..Self::new(0, burst)
        }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Handshake,
    /// `hello` failed on the protocol version: only `daemon.replace`.
    Incompatible,
    Ready,
}

struct Connection<'a> {
    ctx: &'a Arc<ServerCtx>,
    /// Id of the connection in the table: plans are bound to it (M-04).
    id: u64,
    peer: AcceptedPeer,
    info: Option<ProcInfo>,
    outbox: Arc<Outbox>,
    profile: ConnectionProfile,
    /// Protocol negotiated in `hello`: the connection sees the methods and
    /// shapes of this version (DS-TS-GRP-004 E-D1).
    protocol: u32,
    /// The shapes the connection understands: those its protocol implies,
    /// and from protocol 9 the ones it accepted (ADR-GRP-016 § 1).
    capabilities: std::collections::BTreeSet<&'static str>,
    /// `connection.accept` was answered: it is taken once.
    accepted: bool,
    phase: Phase,
    subscriptions: Vec<u32>,
    next_subscription: u32,
    bucket: Bucket,
    reserved_bucket: Bucket,
    /// Reads of an `mcp` connection (US-MCP-005, ADR-MCP-001 § 6): its own
    /// budget, so an agent in a loop cannot starve the other connections.
    mcp_read_bucket: Bucket,
    /// Writes of an `mcp` connection (ADR-MCP-001 § 6): spent after the snapshot quota answered,
    /// so a looping agent gets its real wait. A lock only because `prepare` takes `&self`.
    mcp_write_bucket: std::sync::Mutex<Bucket>,
}

/// What a handled request leads to.
enum After {
    Continue,
    Close,
}

impl Connection<'_> {
    fn serve(&mut self, stream: Stream) {
        let limits = self.ctx.config.limits;
        // The whole handshake has one deadline, not one per read.
        let handshake_deadline = Instant::now() + limits.handshake_timeout;
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            if self.ctx.stopping.load(Ordering::SeqCst) || self.outbox.is_lagged() {
                return;
            }
            let timeout = match self.phase {
                Phase::Ready if !self.subscriptions.is_empty() => None,
                // An idle connection without subscriptions is closed.
                Phase::Ready => Some(limits.idle_timeout),
                _ => {
                    let left = handshake_deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return;
                    }
                    Some(left)
                }
            };
            let _ = reader.get_ref().set_read_timeout(timeout);
            match read_frame(&mut reader, &mut buf, MAX_MESSAGE_BYTES) {
                Ok(true) => {}
                Ok(false) | Err(FrameError::Io(_)) => return,
                Err(FrameError::TooLarge) => {
                    self.send(Response::err(
                        None,
                        ErrorObject::new(code::INVALID_REQUEST, "message too large"),
                    ));
                    if self.phase != Phase::Ready {
                        return;
                    }
                    continue;
                }
            }
            let request = match decode_request(&buf) {
                Ok(request) => request,
                Err(error) => {
                    self.send(Response::err(None, error));
                    if self.phase != Phase::Ready {
                        return;
                    }
                    continue;
                }
            };
            let after = match self.phase {
                Phase::Handshake => self.handshake(&request),
                Phase::Incompatible => self.incompatible(&request),
                Phase::Ready => {
                    if self.bucket.take() {
                        self.dispatch(&request)
                    } else {
                        self.send(Response::err(
                            Some(request.id.clone()),
                            ErrorObject::new(code::RATE_LIMITED, "rate limited"),
                        ));
                        After::Continue
                    }
                }
            };
            if matches!(after, After::Close) {
                return;
            }
        }
    }

    fn send(&self, response: Response) {
        self.outbox.push(&response);
    }

    fn reply<T: serde::Serialize>(&self, id: &Id, result: Result<T, ErrorObject>) {
        self.send(match result {
            Ok(value) => Response::ok(id.clone(), value),
            Err(error) => Response::err(Some(id.clone()), error),
        });
    }

    fn handshake(&mut self, request: &Request) -> After {
        if request.method != methods::HELLO {
            self.send(Response::err(
                Some(request.id.clone()),
                ErrorObject::new(code::HANDSHAKE_REQUIRED, "send hello first"),
            ));
            return After::Close;
        }
        let hello: Hello = match request.params() {
            Ok(hello) => hello,
            Err(error) => {
                self.send(Response::err(Some(request.id.clone()), error));
                return After::Close;
            }
        };
        if hello.client_version.len() > gitraptor_api::messages::MAX_CLIENT_VERSION_LEN {
            self.send(Response::err(
                Some(request.id.clone()),
                ErrorObject::new(code::INVALID_PARAMS, "client_version too long"),
            ));
            return After::Close;
        }
        // The peer's executable, not its claim, decides the MCP profile.
        let peer_is_mcp = self
            .info
            .as_ref()
            .and_then(|i| i.exe.as_deref())
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "raptor-mcp" || n == "raptor-mcp.exe");
        self.profile = if peer_is_mcp || hello.client == ClientKind::Mcp {
            ConnectionProfile::Mcp
        } else {
            ConnectionProfile::Full
        };
        let window = self.ctx.config.min_protocol..=self.ctx.config.protocol;
        if !window.contains(&hello.protocol) {
            self.send(Response::err(
                Some(request.id.clone()),
                ErrorObject::new(code::INCOMPATIBLE_PROTOCOL, "incompatible protocol").with_data(
                    IncompatibleData {
                        daemon_protocol: self.ctx.config.protocol,
                        binary_version: self.ctx.daemon.binary_version.clone(),
                    },
                ),
            ));
            self.phase = Phase::Incompatible;
            return After::Continue;
        }
        self.protocol = hello.protocol;
        self.capabilities = capability::implied(self.protocol)
            .into_iter()
            .filter(|c| self.ctx.config.capabilities.contains(c))
            .collect();
        self.apply_capabilities();
        // N5: who the daemon sees, for the Cockpit's "you act as". Only for
        // `cli` clients of protocol 6 on a full connection: the hook client
        // connects often and does not need the process walk.
        let requester = (self.has(methods::CAP_REQUESTER.name)
            && self.profile == ConnectionProfile::Full
            && hello.client == ClientKind::Cli)
            .then(|| self.connection_requester());
        let result = HelloResult {
            protocol: self.protocol,
            binary_version: self.ctx.daemon.binary_version.clone(),
            instance_id: self.ctx.instance_id.clone(),
            daemon_pid: self.ctx.daemon.pid,
            profile: self.profile,
            max_message_bytes: MAX_MESSAGE_BYTES as u64,
            methods: METHODS
                .iter()
                .filter(|m| self.offered(m))
                .map(|m| m.name.to_owned())
                .collect(),
            requester,
            // Protocol 5 to 8 reject unknown fields: only 9 hears of them.
            capabilities: (self.protocol >= CAPABILITIES_PROTOCOL).then(|| {
                self.ctx
                    .config
                    .capabilities
                    .iter()
                    .map(|c| (*c).to_owned())
                    .collect()
            }),
        };
        self.reply(&request.id, Ok(result));
        self.phase = Phase::Ready;
        After::Continue
    }

    /// Whether the connection understands the shape `capability` names.
    fn has(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }

    /// Declared authorship in Git events (US-GRD-019): only with the
    /// capability, and never for `raptor-mcp`, whose view carries no names
    /// or emails (amendment of ADR-GRP-013), even if it asked for it.
    fn authorship_events(&self) -> bool {
        self.profile == ConnectionProfile::Full && self.has(methods::CAP_EVENTS_AUTHORSHIP.name)
    }

    /// What the connection's capabilities change in what it is sent.
    fn apply_capabilities(&self) {
        // A connection without it cannot read Git events of kind `reset`
        // (protocol 8, US-TMC-004).
        self.outbox
            .set_before_reset(!self.has(methods::CAP_GIT_RESET.name));
        // Nor the last activity and fetch of `worktree.state` without
        // `scope.activity` (DEP-CKP-4).
        self.outbox
            .set_without_activity(!self.has(methods::CAP_SCOPE_ACTIVITY.name));
        // Nor the observation tiers without `observation.tiers` (TS-GRP-006).
        self.outbox
            .set_without_tiers(!self.has(methods::CAP_OBSERVATION_TIERS.name));
        // Nor the discovered repos without `discovery.events`, and never to
        // `raptor-mcp` (US-GRP-020, SEC-MCP-01).
        self.outbox.set_without_discovery(
            self.profile != ConnectionProfile::Full
                || !self.has(methods::CAP_DISCOVERY_EVENTS.name),
        );
        // Nor the loss of the protection without `guard.protection` (US-GRD-004).
        // Never to `raptor-mcp` either (BR-AUTH-004).
        self.outbox.set_without_protection(
            self.profile != ConnectionProfile::Full
                || !self.has(methods::CAP_GUARD_PROTECTION.name),
        );
        // Nor the declared authorship of commits without `events.authorship`
        // (US-GRD-019): `raptor-mcp` never asks for it.
        self.outbox
            .set_without_authorship(!self.authorship_events());
    }

    /// `connection.accept` (protocol 9): the client's capabilities, once and
    /// before its first subscription, so no event it already received
    /// changes shape. Names the daemon does not serve are ignored.
    fn connection_accept(&mut self, params: AcceptParams) -> Result<AcceptResult, ErrorObject> {
        if self.accepted || !self.subscriptions.is_empty() {
            return Err(ErrorObject::new(
                code::INVALID_REQUEST,
                "capabilities are accepted once, before any subscription",
            ));
        }
        if params.capabilities.len() > capability::MAX_ACCEPTED
            || params
                .capabilities
                .iter()
                .any(|c| c.is_empty() || c.len() > capability::MAX_NAME_LEN)
        {
            return Err(ErrorObject::new(
                code::INVALID_PARAMS,
                "invalid capabilities",
            ));
        }
        self.accepted = true;
        let served = &self.ctx.config.capabilities;
        self.capabilities.extend(
            served
                .iter()
                .copied()
                .filter(|c| params.capabilities.iter().any(|asked| asked == c)),
        );
        self.apply_capabilities();
        Ok(AcceptResult {
            capabilities: self.capabilities.iter().map(|c| (*c).to_owned()).collect(),
        })
    }

    fn offered(&self, m: &MethodSpec) -> bool {
        m.name != methods::HELLO
            && m.exists_in(self.protocol)
            && (self.profile == ConnectionProfile::Full || m.mcp)
    }

    /// The requester and layer of this connection as of now (N5).
    fn connection_requester(&self) -> ConnectionRequester {
        match self.resolve() {
            Ok(r) => ConnectionRequester::Resolved {
                actor: r.who.actor.clone(),
                layer: self.layer(&r),
                confirmable: r.confirmable,
            },
            Err(_) => ConnectionRequester::Unverified,
        }
    }

    fn incompatible(&mut self, request: &Request) -> After {
        if request.method == methods::DAEMON_REPLACE {
            return self.replace(request);
        }
        self.send(Response::err(
            Some(request.id.clone()),
            ErrorObject::new(code::INCOMPATIBLE_PROTOCOL, "incompatible protocol"),
        ));
        After::Close
    }

    fn dispatch(&mut self, request: &Request) -> After {
        let Some(spec) = methods::spec(&request.method) else {
            self.reply::<()>(&request.id, Err(not_found()));
            return After::Continue;
        };
        if spec.name == methods::HELLO {
            self.reply::<()>(
                &request.id,
                Err(ErrorObject::new(code::INVALID_REQUEST, "already greeted")),
            );
            return After::Continue;
        }
        // A reserved command asked over MCP is refused (and audited); any
        // other method outside the MCP allowlist does not exist for it. A
        // method newer than the connection's protocol does not exist either,
        // reserved or not (US-GRD-001: `guard.*` is protocol 7).
        if !spec.exists_in(self.protocol) || (!self.offered(spec) && !spec.reserved) {
            self.reply::<()>(&request.id, Err(not_found()));
            return After::Continue;
        }
        let reserved_like = spec.reserved || spec.name == methods::DAEMON_REPLACE;
        if reserved_like && !self.reserved_bucket.take() {
            self.ctx
                .logger
                .warn("reserved_rate_limited", &[("op", Field::Text(spec.name))]);
            self.reply::<()>(
                &request.id,
                Err(ErrorObject::new(code::RATE_LIMITED, "rate limited")),
            );
            return After::Continue;
        }
        // An agent in a loop hits its connection's read limit before any
        // check of its scope (US-MCP-005). The daemon counts per connection;
        // per requester arrives with the first MCP writes (S-03).
        if self.is_mcp()
            && !reserved_like
            && write_route(spec.name).is_none()
            && !self.mcp_read_bucket.take()
        {
            self.reply::<()>(
                &request.id,
                Err(ErrorObject::new(code::RATE_LIMITED, "rate limited")),
            );
            return After::Continue;
        }
        // Every method that may modify a repo is served by one route, and
        // the only route that runs anything is the protected operation.
        if let Some(route) = write_route(spec.name) {
            let result = match route {
                WriteRoute::Protected => self.operation_run(request),
                WriteRoute::TimeMachine(_) if spec.name == methods::TM_UNDO => {
                    self.tm_undo(spec, request)
                }
                WriteRoute::TimeMachine(_) if spec.name == methods::TM_RESTORE => {
                    self.tm_restore(spec, request)
                }
                WriteRoute::TimeMachine(story) => self.tm_command(spec, request, story),
            };
            self.reply(&request.id, result);
            return After::Continue;
        }
        match spec.name {
            methods::OPERATION_DESCRIBE => {
                let result = request
                    .params::<NoParams>()
                    .map(|_| catalog::describe(self.is_mcp()));
                self.reply(&request.id, result);
            }
            methods::OPERATION_PREPARE => {
                let result = self.operation_prepare(request);
                self.reply(&request.id, result);
            }
            methods::OPERATION_CANCEL => {
                let result = self.operation_cancel(request);
                self.reply(&request.id, result);
            }
            methods::REQUESTER_RESOLVE => {
                let result = self.requester_resolve(request);
                self.reply(&request.id, result);
            }
            methods::TM_SNAPSHOT => {
                let story = spec.implemented_by.unwrap_or("US-TMC-006");
                let result = self.tm_command(spec, request, story);
                self.reply(&request.id, result);
            }
            methods::TM_TIMELINE => {
                let result = self.tm_timeline(request);
                self.reply(&request.id, result);
            }
            methods::PING => self.reply(&request.id, request.params::<NoParams>().map(|_| "pong")),
            methods::CONNECTION_ACCEPT => {
                let result = request.params().and_then(|p| self.connection_accept(p));
                self.reply(&request.id, result);
            }
            methods::SCOPE_SNAPSHOT => {
                let result = request.params().and_then(|p| self.scope_snapshot(p));
                self.reply(&request.id, result);
            }
            methods::SCOPE_SUBSCRIBE => {
                let result = request.params().and_then(|p| self.scope_subscribe(p));
                self.reply(&request.id, result);
            }
            methods::REPO_LOCATE => {
                let result = request.params().and_then(|p| self.repo_locate(p));
                self.reply(&request.id, result);
            }
            methods::ENGINE_SNAPSHOT => {
                let result = request.params::<NoParams>().map(|_| self.snapshot());
                self.reply(&request.id, result);
            }
            methods::ENGINE_RESOURCES => {
                let result = request.params::<NoParams>().map(|_| {
                    let mut result = self.ctx.resources.read();
                    if !self.has(methods::CAP_OBSERVATION_TIERS.name) {
                        result.observation = None;
                    }
                    result
                });
                self.reply(&request.id, result);
            }
            methods::EVENTS_SUBSCRIBE => {
                let result = request.params().and_then(|p| self.subscribe(p));
                self.reply(&request.id, result);
            }
            methods::EVENTS_UNSUBSCRIBE => {
                let result = request.params().and_then(|p| self.unsubscribe(p));
                self.reply(&request.id, result);
            }
            methods::AUDIT_LIST => {
                let result = request.params().and_then(|p| self.audit_list(p));
                self.reply(&request.id, result);
            }
            methods::EVENTS_HISTORY => {
                let result = request.params().and_then(|p| self.events_history(p));
                self.reply(&request.id, result);
            }
            methods::SESSIONS_LIST => {
                let result = request.params().and_then(|p| self.sessions_list(p));
                self.reply(&request.id, result);
            }
            methods::DAEMON_STOP => {
                let result = request
                    .params::<NoParams>()
                    .and_then(|_| self.reserved(spec, None));
                return self.stop_after(request, result, |requested_by| StopCause::StopCommand {
                    requested_by,
                });
            }
            methods::DAEMON_REPLACE => return self.replace(request),
            methods::REPO_ADD => {
                let result = self.repo_add(spec, request);
                self.reply(&request.id, result);
            }
            methods::REPO_RETIRE => {
                let result = self.repo_retire(spec, request);
                self.reply(&request.id, result);
            }
            // US-GRP-020 and US-GRP-022: with `repo`, because accepting a
            // discovered repo is `repo.add`.
            methods::DISCOVERY_ROOTS
            | methods::DISCOVERY_CANDIDATES
            | methods::DISCOVERY_ROOT_ADD
            | methods::DISCOVERY_ROOT_REMOVE
            | methods::DISCOVERY_DISMISS => {
                let result = self.discovery(spec, request);
                self.reply(&request.id, result);
            }
            methods::MCP_ENABLE | methods::MCP_DISABLE => {
                let result = self.mcp_mark(spec, request);
                self.reply(&request.id, result);
            }
            methods::MCP_ALLOWLIST => {
                let result = request
                    .params::<NoParams>()
                    .map(|_| methods::McpAllowlistResult {
                        repo_ids: self.ctx.mcp_repos.ids(),
                    });
                self.reply(&request.id, result);
            }
            methods::MCP_STATUS => {
                let result = request.params::<NoParams>().and_then(|_| self.mcp_status());
                self.reply(&request.id, result);
            }
            methods::REGISTRATION_REGISTER => {
                let result = self.registration_register(spec, request);
                self.reply(&request.id, result);
            }
            methods::REGISTRATION_WITHDRAW => {
                let result = self.registration_withdraw(spec, request);
                self.reply(&request.id, result);
            }
            // On this thread, never through the loop: the hook of an executor's own `git`
            // must not wait for anything (ADR-GRD-003, Enmienda Cockpit).
            methods::GUARD_EVALUATE => {
                let result = request.params::<EvaluateParams>().map(|p| {
                    let caller = self.guard_caller(&p);
                    let (decision, policy) =
                        crate::guardrails::evaluate::serve_logged(&self.ctx.guard, &p, &caller);
                    // One decision per operation (ADR-GRD-003 § 6): the second line of the
                    // same `git` reuses the one `commit-msg` gave.
                    if let (
                        Some(git),
                        gitraptor_api::guard::Operation::Commit { stage },
                        Some(facts),
                    ) = (caller.git, &p.operation, &p.authorship)
                        && *stage == gitraptor_api::guard::CommitStage::CommitMsg
                        && caller.authorship
                        && decision.applied_effect == gitraptor_api::guard::Effect::Allow
                    {
                        self.ctx.commit_decisions.record(git, facts);
                    }
                    // Before the answer: the entry reaches the loop ahead of any later query,
                    // and the actor is read while the hook client is alive (US-GRD-005, D3).
                    self.log_decision(&p, &decision, &caller, policy);
                    self.hook_claims(&p, &caller, &decision);
                    decision
                });
                self.reply(&request.id, result);
            }
            methods::GUARD_LOG => {
                let result = self.guard_log(request);
                self.reply(&request.id, result);
            }
            methods::GUARD_PLAN
            | methods::GUARD_STATUS
            | methods::GUARD_INSTALL
            | methods::GUARD_DECLINE
            | methods::GUARD_UNINSTALL
            | methods::GUARD_CANCEL => {
                let result = self.guard(spec, request);
                self.reply(&request.id, result);
            }
            _ => {
                // Declared ahead of their stories: validated, authorized and
                // audited, then "not implemented".
                // Their parameters are not defined yet: none are accepted.
                let result = request
                    .params::<NoParams>()
                    .and_then(|_| self.reserved(spec, None));
                self.reply(&request.id, result);
            }
        }
        After::Continue
    }

    /// S4 (DS-US-GRP-007 § 7), before the reply while the hook's `git` waits: an allowed
    /// `reference-transaction` of an agent leaves its claims for the detector, and a commit the
    /// second line denies takes its `git`'s claims back.
    fn hook_claims(
        &self,
        params: &EvaluateParams,
        caller: &crate::guardrails::evaluate::Caller,
        decision: &gitraptor_api::guard::Decision,
    ) {
        use gitraptor_api::guard::{CommitStage, Effect, Operation};
        let allowed = decision.applied_effect == Effect::Allow;
        if let (
            Operation::Commit {
                stage: CommitStage::SecondLine,
            },
            false,
            Some(git),
        ) = (&params.operation, allowed, caller.git)
        {
            self.ctx.hook_claims.revoke(git);
        }
        // The form of the observed worktree roots, as for `mcp.status`.
        let cwd =
            || process_cwd(self.peer.pid).and_then(|p| gitraptor_git::paths::canonicalize(&p).ok());
        self.ctx.hook_claims.observe(
            self.peer,
            &self.ctx.checks(),
            params,
            allowed,
            cwd,
            gitraptor_api::clock::monotonic_ns(),
        );
    }

    /// What the daemon knows of a hook client (US-GRD-018): the actor and the worktree are
    /// resolved only for a commit, the one operation whose rules read them.
    fn guard_caller(&self, params: &EvaluateParams) -> crate::guardrails::evaluate::Caller {
        let authorship = self.has(methods::CAP_GUARD_AUTHORSHIP.name);
        // US-GRD-008: the actor of a branch movement or a push, for the rules that tell an agent
        // from the person. Resolved only for a connection that asked for it.
        if self.has(methods::CAP_GUARD_POLICIES.name)
            && matches!(
                params.operation,
                gitraptor_api::guard::Operation::RefTransaction { .. }
                    | gitraptor_api::guard::Operation::Push { .. }
            )
        {
            let checks = self.ctx.checks();
            return crate::guardrails::evaluate::Caller {
                actor: crate::guardrails::actor::resolve(self.peer, &checks, Some(&self.ctx.marks)),
                cwd: process_cwd(self.peer.pid),
                authorship,
                policies: true,
                ..Default::default()
            };
        }
        if !authorship
            || !matches!(
                params.operation,
                gitraptor_api::guard::Operation::Commit { .. }
            )
        {
            return crate::guardrails::evaluate::Caller {
                authorship,
                ..Default::default()
            };
        }
        use crate::guardrails::second_line;
        use gitraptor_api::guard::{CommitStage, Operation};
        let checks = self.ctx.checks();
        let actor = crate::guardrails::actor::resolve(self.peer, &checks, Some(&self.ctx.marks));
        // The `git` behind the hook only matters for an agent's commit.
        let git = actor.and_then(|_| second_line::nearest_git(self.peer, &checks));
        let second_line_skip = match params.operation {
            Operation::Commit {
                stage: CommitStage::SecondLine,
            } => {
                !self.has(methods::CAP_GUARD_AUTHORSHIP_SECOND_LINE.name)
                    || git
                        .zip(params.authorship.as_ref())
                        .is_some_and(|(g, f)| self.ctx.commit_decisions.contains(g, f))
                    || !second_line::evaluates(git, &checks)
            }
            _ => false,
        };
        crate::guardrails::evaluate::Caller {
            actor,
            cwd: process_cwd(self.peer.pid),
            authorship,
            git,
            second_line_skip,
            policies: false,
        }
    }

    /// Hands the decision log entry of a decision to the loop, never waiting (US-GRD-005):
    /// over the in-flight cap the occurrence is only counted (D4).
    fn log_decision(
        &self,
        params: &EvaluateParams,
        decision: &gitraptor_api::guard::Decision,
        caller: &crate::guardrails::evaluate::Caller,
        policy: Option<&'static str>,
    ) {
        use crate::guardrails::log;
        if decision.applied_effect == gitraptor_api::guard::Effect::Allow
            && decision.notices.is_empty()
            && policy != Some("flexible")
        {
            return;
        }
        // Git runs the hook in the repo it acts on: a client whose working directory is not in
        // the repo of `commonDir` does not write to that repo's log.
        let Some(cwd) = caller
            .cwd
            .clone()
            .or_else(|| process_cwd(self.peer.pid))
            // The drive form on Windows (no `\\?\` prefix) in what the log shows and compares.
            .and_then(|cwd| cwd.canonicalize().ok())
            .map(gitraptor_policy::guard::fastpath::simplified)
        else {
            return;
        };
        let Some(reader) =
            crate::guardrails::authorship::worktree_reader(&cwd, Path::new(&params.common_dir))
        else {
            return;
        };
        let branch = reader.head().ok().and_then(|head| head.branch);
        let checks = self.ctx.checks();
        let (actor, under_executor) =
            crate::guardrails::actor::resolve_logged(self.peer, &checks, Some(&self.ctx.marks));
        let (at_ms, utc_offset_s) = crate::watch::wall_now();
        let ctx = log::LogContext {
            actor,
            under_executor,
            worktree: Some(cwd.to_string_lossy().into_owned()),
            branch,
            authorship_policy: policy.map(str::to_owned),
            at_ms,
            utc_offset_s,
        };
        let Some(entry) = log::entry(params, decision, &ctx) else {
            return;
        };
        let sink = self.ctx.guard.log();
        if !sink.try_reserve() {
            sink.overflow(&entry);
        } else if !self.ctx.control.guard_record(entry) {
            sink.release();
        }
    }

    /// `repo.add` (US-GRP-001): parameters checked lexically, then the
    /// daemon authorizes and audits, and only then is the path read. An
    /// agent cannot make the daemon probe the file system.
    fn repo_add(&self, spec: &MethodSpec, request: &Request) -> Result<RepoAddResult, ErrorObject> {
        let params: RepoAddParams = request.params()?;
        let path = validate::client_path(&params.path).map_err(invalid)?;
        self.reserved(spec, None)?;
        let t_recv = gitraptor_api::clock::monotonic_ns();
        let common_dir = match crate::observe::locate(&path) {
            Ok(common_dir) => common_dir,
            Err(reason) => {
                // A discovered repo deleted before it was accepted stops
                // being proposed (US-GRP-022); one that is only unreadable
                // or untrusted stays.
                if reason == RepoRejection::NotARepo {
                    self.ctx.control.discovery_forget(path);
                }
                return Err(rejected(reason));
            }
        };
        // Against the base branch without the profile; the daemon loop
        // counts again if the repo's store keeps a confirmed one.
        let read = crate::observe::reconcile(&common_dir, &crate::observe::base_branch(None))
            .map_err(|_| rejected(RepoRejection::Unreadable))?;
        let t_computed = gitraptor_api::clock::monotonic_ns();
        self.ctx
            .control
            .repo_add(RepoAddRequest {
                common_dir,
                read,
                t_recv,
                t_computed,
            })
            .map(|mut result| {
                if !self.has(methods::CAP_SCOPE_ACTIVITY.name) {
                    result.repo.without_activity();
                }
                if !self.has(methods::CAP_OBSERVATION_TIERS.name) {
                    result.repo.without_tier();
                }
                self.kept_temps(std::slice::from_mut(&mut result.repo));
                result
            })
            .map_err(repo_command_error)
    }

    /// `discovery.*` (US-GRP-020, US-GRP-022). The reserved ones are
    /// authorized and audited before the daemon reads anything at the path,
    /// like `repo.add`; the loop validates the root (SEC-15) and owns the
    /// profile.
    fn discovery(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<serde_json::Value, ErrorObject> {
        use crate::daemon::{DiscoveryError, DiscoveryRequest};
        use gitraptor_api::discovery::{
            CandidatesResult, DismissResult, PathParams, RootAddParams, RootsResult,
        };
        fn unavailable() -> ErrorObject {
            ErrorObject::new(code::INTERNAL, "discovery unavailable")
        }
        let failed = |err: DiscoveryError| match err {
            DiscoveryError::Rejected(data) => {
                ErrorObject::new(methods::ROOT_REJECTED.code, "root rejected").with_data(data)
            }
            DiscoveryError::Broad(data) => {
                ErrorObject::new(methods::ROOT_BROAD.code, "broad root").with_data(data)
            }
            DiscoveryError::UnknownRoot => {
                ErrorObject::new(methods::ROOT_UNKNOWN.code, "not a declared root")
            }
            DiscoveryError::NotACandidate => {
                ErrorObject::new(methods::NOT_A_CANDIDATE.code, "not a discovered repo")
            }
            DiscoveryError::Failed => unavailable(),
        };
        let control = &self.ctx.control;
        let value = match spec.name {
            methods::DISCOVERY_ROOTS => {
                let roots = control
                    .discovery(DiscoveryRequest::Roots)
                    .flatten()
                    .ok_or_else(unavailable)?;
                serde_json::to_value(RootsResult { roots })
            }
            methods::DISCOVERY_CANDIDATES => {
                let candidates = control
                    .discovery(DiscoveryRequest::Candidates)
                    .flatten()
                    .ok_or_else(unavailable)?;
                serde_json::to_value(CandidatesResult { candidates })
            }
            methods::DISCOVERY_ROOT_ADD => {
                let params: RootAddParams = request.params()?;
                let path = validate::client_path(&params.path).map_err(invalid)?;
                self.reserved(spec, None)?;
                let ctx = control
                    .discovery(DiscoveryRequest::Context)
                    .ok_or_else(unavailable)?;
                let root = crate::daemon::prepare_root(&path, &ctx, params.confirm_broad)
                    .map_err(failed)?;
                let result = control
                    .discovery(|reply| DiscoveryRequest::RootAdd {
                        root: Box::new(root),
                        reply,
                    })
                    .ok_or_else(unavailable)?
                    .map_err(failed)?;
                serde_json::to_value(result)
            }
            methods::DISCOVERY_ROOT_REMOVE => {
                let params: PathParams = request.params()?;
                let path = validate::client_path(&params.path).map_err(invalid)?;
                self.reserved(spec, None)?;
                let result = control
                    .discovery(|reply| DiscoveryRequest::RootRemove { path, reply })
                    .ok_or_else(unavailable)?
                    .map_err(failed)?;
                serde_json::to_value(result)
            }
            _ => {
                let params: PathParams = request.params()?;
                let path = validate::client_path(&params.path).map_err(invalid)?;
                self.reserved(spec, None)?;
                let path = control
                    .discovery(|reply| DiscoveryRequest::Dismiss { path, reply })
                    .ok_or_else(unavailable)?
                    .map_err(failed)?;
                serde_json::to_value(DismissResult { path })
            }
        };
        value.map_err(|_| unavailable())
    }

    /// `guard.*` (US-GRD-001, US-GRD-003): the reserved ones are authorized and audited before
    /// the path is read, like `repo.add`; the loop then acts on an observed repo only. The
    /// accepted `guard.uninstall` audit is the announcement every client receives (D5).
    fn guard(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<serde_json::Value, ErrorObject> {
        use gitraptor_api::guard::{GuardUninstallParams, GuardUninstallRefusedData};
        let (path, confirm) = if spec.name == methods::GUARD_UNINSTALL {
            let params: GuardUninstallParams = request.params()?;
            (params.path, params.confirm)
        } else {
            let params: GuardRepoParams = request.params()?;
            (params.path, None)
        };
        let path = validate::client_path(&path).map_err(invalid)?;
        if spec.reserved {
            self.reserved(spec, None)?;
        }
        let common_dir = crate::observe::locate(&path).map_err(rejected)?;
        let requester = crate::guardrails::pending::Requester {
            pid: self.peer.pid,
            start_us: self.peer.start_us,
        };
        let kind = match spec.name {
            methods::GUARD_PLAN => GuardRequest::Plan,
            methods::GUARD_INSTALL => GuardRequest::Install {
                repair: self.has(methods::CAP_GUARD_PROTECTION.name),
            },
            methods::GUARD_DECLINE => GuardRequest::Decline,
            methods::GUARD_UNINSTALL => match confirm {
                None => GuardRequest::UninstallRequest(requester),
                Some(action_id) => GuardRequest::UninstallApply {
                    action_id,
                    requester,
                },
            },
            methods::GUARD_CANCEL => GuardRequest::Cancel(requester),
            _ => GuardRequest::Status,
        };
        let value =
            match self.ctx.control.guard(common_dir, kind) {
                GuardReply::Plan(mut plan) => {
                    self.guard_shape_plan(&mut plan);
                    serde_json::to_value(*plan)
                }
                GuardReply::Status(mut status) => {
                    self.guard_shape_status(&mut status);
                    serde_json::to_value(*status)
                }
                GuardReply::Uninstall(mut result) => {
                    self.guard_shape_status(&mut result.status);
                    serde_json::to_value(*result)
                }
                GuardReply::NotObserved => return Err(rejected(RepoRejection::NotObserved)),
                GuardReply::Rejected(blockers) => {
                    return Err(ErrorObject::new(code::GUARD_REJECTED, "install refused")
                        .with_data(GuardRejectedData {
                            blockers: self.guard_blockers(blockers),
                        }));
                }
                GuardReply::UninstallRefused(refused) => {
                    return Err(ErrorObject::new(
                        methods::GUARD_UNINSTALL_REFUSED.code,
                        "uninstall refused",
                    )
                    .with_data(GuardUninstallRefusedData {
                        reason: refused.reason,
                        remaining_ms: refused.remaining_ms,
                    }));
                }
                GuardReply::Failed => {
                    return Err(ErrorObject::new(code::INTERNAL, "guardrails unavailable"));
                }
            };
        value.map_err(|_| ErrorObject::new(code::INTERNAL, "encode"))
    }

    /// `chain-impossible` reads `prior-hooks` for a connection without `guard.prior-hooks`.
    fn guard_blockers(
        &self,
        blockers: Vec<gitraptor_api::guard::InstallBlocker>,
    ) -> Vec<gitraptor_api::guard::InstallBlocker> {
        use gitraptor_api::guard::InstallBlocker;
        if self.has(methods::CAP_GUARD_PRIOR_HOOKS.name) {
            return blockers;
        }
        let mut out: Vec<InstallBlocker> = Vec::new();
        for b in blockers {
            let b = if b == InstallBlocker::ChainImpossible {
                InstallBlocker::PriorHooks
            } else {
                b
            };
            if !out.contains(&b) {
                out.push(b);
            }
        }
        out
    }

    /// The fields of `guard.status` a connection has the capability for.
    fn guard_shape_status(&self, status: &mut gitraptor_api::guard::GuardStatus) {
        if !self.has(methods::CAP_GUARD_PENDING_ACTION.name) {
            status.pending = None;
        }
        if !self.has(methods::CAP_GUARD_PROTECTION.name) {
            status.hooks = None;
            status.diagnostics.clear();
            status.minimum_set = None;
        }
        status.last_refusal = self.guard_blockers(std::mem::take(&mut status.last_refusal));
    }

    /// The fields of `guard.plan` a connection has the capability for.
    fn guard_shape_plan(&self, plan: &mut gitraptor_api::guard::GuardPlan) {
        if !self.has(methods::CAP_GUARD_PRIOR_HOOKS.name) {
            plan.prior = None;
        }
        if !self.has(methods::CAP_GUARD_PROTECTION.name) {
            plan.repair = None;
        }
        plan.blockers = self.guard_blockers(std::mem::take(&mut plan.blockers));
        self.guard_shape_status(&mut plan.status);
    }

    /// `guard.log` (US-GRD-005): read-only and not reserved; the loop answers, after
    /// writing what the connections counted.
    fn guard_log(
        &self,
        request: &Request,
    ) -> Result<gitraptor_api::guard::GuardLogResult, ErrorObject> {
        use gitraptor_api::guard::{GuardLogParams, MAX_LOG_PAGE};
        let params: GuardLogParams = request.params()?;
        let path = validate::client_path(&params.path).map_err(invalid)?;
        let common_dir = crate::observe::locate(&path).map_err(rejected)?;
        let limit = params.limit.unwrap_or(50).clamp(1, MAX_LOG_PAGE);
        match self
            .ctx
            .control
            .guard_log(common_dir, params.since_ms, limit)
        {
            GuardLogReply::Log(log) => {
                let mut log = *log;
                // A connection without `guard.protection` never sees the changes of state.
                if !self.has(methods::CAP_GUARD_PROTECTION.name) {
                    log.entries
                        .retain(|e| e.kind != gitraptor_api::guard::LogKind::ProtectionState);
                }
                Ok(log)
            }
            GuardLogReply::NotObserved => Err(rejected(RepoRejection::NotObserved)),
            GuardLogReply::Failed => {
                Err(ErrorObject::new(code::INTERNAL, "guardrails unavailable"))
            }
        }
    }

    /// `repo.retire` (US-GRP-001): stops observing; the data is kept.
    fn repo_retire(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<RepoRetireResult, ErrorObject> {
        let params: RepoRetireParams = request.params()?;
        if !valid_repo_id(&params.repo_id) {
            return Err(ErrorObject::new(code::INVALID_PARAMS, "invalid repo_id"));
        }
        self.reserved(spec, Some(params.repo_id.clone()))?;
        self.ctx
            .control
            .repo_retire(params.repo_id)
            .map_err(repo_command_error)
    }

    /// `mcp.enable` and `mcp.disable` (US-MCP-002): like `repo.add`, the
    /// parameters are checked lexically, then the daemon authorizes and
    /// audits, and only then is the path read.
    fn mcp_mark(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<methods::McpRepoResult, ErrorObject> {
        let params: methods::McpRepoParams = request.params()?;
        let path = validate::client_path(&params.path).map_err(invalid)?;
        self.reserved(spec, None)?;
        let common_dir = crate::observe::locate(&path).map_err(rejected)?;
        self.ctx
            .control
            .mcp_mark(common_dir, spec.name == methods::MCP_ENABLE)
            .map_err(|err| match err {
                crate::daemon::McpMarkError::NotObserved => rejected(RepoRejection::NotObserved),
                crate::daemon::McpMarkError::Internal => {
                    ErrorObject::new(code::INTERNAL, "profile unavailable")
                }
            })
    }

    /// `mcp.status` (US-MCP-003, ADR-MCP-001 § 2): the repo and the worktree
    /// come from the caller's working folder, read by the daemon between two
    /// checks of the caller's identity. Outside an enabled repo the refusal
    /// carries no data of any repo.
    fn mcp_status(&self) -> Result<methods::McpStatus, ErrorObject> {
        self.resolve()?;
        // The form of the observed worktree roots (on Windows, the drive form, not `\\?\C:\…`).
        let cwd =
            process_cwd(self.peer.pid).and_then(|p| gitraptor_git::paths::canonicalize(&p).ok());
        let who = self.resolve()?.who;
        let cwd = cwd.ok_or_else(|| scope_refused(ScopeError::NotObserved))?;
        let (_, shared) = self.ctx.bus.snapshot();
        let (r, w) = super::mcp_scope::locate(&cwd, &shared.repos)
            .ok_or_else(|| scope_refused(ScopeError::NotObserved))?;
        let repo = &shared.repos[r];
        if !crate::timemachine::protected::McpAllowlist::allows(
            self.ctx.mcp_repos.as_ref(),
            &repo.repo_id,
        ) {
            return Err(scope_refused(ScopeError::NotAllowlisted));
        }
        let worktree = &repo.worktrees[w];
        let name = Path::new(worktree.path.raw())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let action = matches!(who.actor, gitraptor_api::Actor::Unattributed)
            .then_some(methods::McpStatusAction::RegisterToWrite);
        let branch = match &worktree.status {
            WorktreeStatus::Ready {
                head: HeadView::Branch { name } | HeadView::Unborn { name },
                ..
            } if self.has(methods::CAP_MCP_STATUS_BRANCH.name) => Some(name.clone()),
            _ => None,
        };
        Ok(methods::McpStatus {
            repo_id: repo.repo_id.clone(),
            repo_state: repo.state,
            worktree: gitraptor_api::UntrustedName::new(name),
            branch,
            main: worktree.main,
            requester: who.actor,
            action,
        })
    }

    /// `registration.register` (US-GRP-009, ADR-GRP-005 § 6.6). Not
    /// reserved: whoever passes the checks of the reserved commands is the
    /// developer and names the worktree; anyone else, and every MCP
    /// connection, is an agent registering itself in the worktree of its
    /// working folder, as the agent it is. Its refusals for naming another
    /// worktree or another agent are audited.
    fn registration_register(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<RegistrationRegisterResult, ErrorObject> {
        let params: RegistrationRegisterParams = request.params()?;
        let agent = declared_agent(&params.agent)?;
        let named = params
            .worktree
            .as_deref()
            .map(validate::client_path)
            .transpose()
            .map_err(invalid)?;
        let verdict = (!self.is_mcp()).then(|| check_reserved(self.peer, &self.ctx.checks()));
        if verdict.as_ref().is_some_and(|v| v.refused.is_none()) {
            let folder = named
                .or_else(|| process_cwd(self.peer.pid))
                .ok_or_else(|| registration_rejected(RegistrationRejection::NoWorkingFolder))?;
            return self
                .ctx
                .control
                .register(RegisterRequest {
                    agent,
                    folder,
                    named: None,
                    author: Author::Developer,
                    caller_session: None,
                })
                .map_err(registration_error);
        }
        let verdict = verdict.unwrap_or_else(|| Verdict {
            client: self.identity_without_walk(),
            chain: Vec::new(),
            refused: Some(RefusalReason::NotAvailableToMcp),
        });
        let refuse = |reason: RefusalReason, rejection: RegistrationRejection| {
            self.audit(
                spec.name,
                None,
                AuditOutcome::Rejected,
                Some(reason),
                &verdict.client,
                &verdict.chain,
            )?;
            Err(registration_rejected(rejection))
        };
        // An agent declares the agent it is (M7): Claude Code only from a
        // detected Claude Code session, and such a session not as another.
        let who = self.resolve()?.who;
        let caller_session = match &who.requester {
            Requester::Agent { session_id, .. } => Some(session_id.clone()),
            Requester::Unattributed => None,
        };
        let caller_is_claude = matches!(
            who.actor,
            gitraptor_api::Actor::Agent {
                kind: gitraptor_api::AgentKind::ClaudeCode,
                ..
            }
        );
        if caller_is_claude != (agent.kind == AgentKind::ClaudeCode) {
            return refuse(
                RefusalReason::AgentMismatch,
                RegistrationRejection::AgentMismatch,
            );
        }
        let folder = process_cwd(self.peer.pid)
            .ok_or_else(|| registration_rejected(RegistrationRejection::NoWorkingFolder))?;
        match self.ctx.control.register(RegisterRequest {
            agent,
            folder,
            named,
            author: Author::Agent,
            caller_session,
        }) {
            Err(RegistrationError::Rejected(RegistrationRejection::WorktreeMismatch)) => refuse(
                RefusalReason::WorktreeMismatch,
                RegistrationRejection::WorktreeMismatch,
            ),
            other => other.map_err(registration_error),
        }
    }

    /// `registration.withdraw` (US-GRP-009): a reserved command. An agent
    /// withdrawing its own registration is `unregister_agent` of US-MCP-006.
    fn registration_withdraw(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<RegistrationWithdrawResult, ErrorObject> {
        let params: RegistrationWithdrawParams = request.params()?;
        let agent = declared_agent(&params.agent)?;
        let folder = validate::client_path(&params.worktree).map_err(invalid)?;
        self.reserved(spec, None)?;
        self.ctx
            .control
            .withdraw(WithdrawRequest { agent, folder })
            .map_err(registration_error)
    }

    /// The kept temporary entries still there, for a connection with
    /// `timemachine.kept-temps` (DS-TS-TMC-003, Enmienda T2).
    fn kept_temps(&self, repos: &mut [gitraptor_api::messages::RepoView]) {
        if let Some(deps) = &self.ctx.tm_engine
            && self.has(methods::CAP_TM_KEPT_TEMPS.name)
        {
            crate::timemachine::kept::refresh_kept_temps(&deps.repos, repos);
        }
    }

    fn snapshot(&self) -> serde_json::Value {
        let (seq, shared) = self.ctx.bus.snapshot();
        let run_id = self.ctx.bus.run_id().to_owned();
        match self.profile {
            ConnectionProfile::Full => {
                let mut snapshot = Snapshot {
                    run_id,
                    seq,
                    engine: shared.engine,
                    daemon: self.ctx.daemon.clone(),
                    repos: shared.repos,
                };
                if !self.has(methods::CAP_SCOPE_ACTIVITY.name) {
                    snapshot
                        .repos
                        .iter_mut()
                        .for_each(gitraptor_api::messages::RepoView::without_activity);
                }
                if !self.has(methods::CAP_OBSERVATION_TIERS.name) {
                    snapshot
                        .repos
                        .iter_mut()
                        .for_each(gitraptor_api::messages::RepoView::without_tier);
                }
                self.kept_temps(&mut snapshot.repos);
                // The ahead/behind as of now (US-GRP-012, D5).
                crate::observe::refresh_divergence(
                    &mut snapshot.repos,
                    &shared.divergence,
                    &self.ctx.divergence,
                );
                // Under the message limit: past the budget the lists go and
                // the counts stay (US-GRP-001).
                if serde_json::to_vec(&snapshot).map_or(0, |v| v.len()) > CHANGE_LIST_BUDGET {
                    for repo in &mut snapshot.repos {
                        crate::observe::without_change_lists(&mut repo.worktrees);
                    }
                }
                serde_json::to_value(snapshot)
            }
            ConnectionProfile::Mcp => {
                // The same scope as `mcp.status`: the deepest observed
                // worktree of the canonical cwd, and outside the allowlist
                // not even its key (US-MCP-003).
                let caller_repo = process_cwd(self.peer.pid)
                    .and_then(|cwd| gitraptor_git::paths::canonicalize(&cwd).ok())
                    .and_then(|cwd| super::mcp_scope::locate(&cwd, &shared.repos))
                    .map(|(r, _)| &shared.repos[r])
                    .filter(|r| {
                        crate::timemachine::protected::McpAllowlist::allows(
                            self.ctx.mcp_repos.as_ref(),
                            &r.repo_id,
                        )
                    })
                    .map(|r| McpRepoView {
                        repo_id: r.repo_id.clone(),
                        state: r.state,
                    });
                serde_json::to_value(McpSnapshot {
                    run_id,
                    seq,
                    engine_state: shared.engine.state,
                    caller_repo,
                })
            }
        }
        .unwrap_or(serde_json::Value::Null)
    }

    /// `scope.snapshot` (N1, N3): one scope with its own sequence, read
    /// under the lock events are published with.
    fn scope_snapshot(&self, params: ScopeSnapshotParams) -> Result<ScopeSnapshot, ErrorObject> {
        valid_scope(&params.scope)?;
        self.wake_scope(&params.scope);
        let (scope_seq, shared) = self
            .ctx
            .bus
            .scope_snapshot(&params.scope)
            .ok_or_else(not_found_id)?;
        let run_id = self.ctx.bus.run_id().to_owned();
        let tiers = self.has(methods::CAP_OBSERVATION_TIERS.name);
        Ok(match params.scope {
            Scope::Global => ScopeSnapshot::Global(GlobalSnapshot {
                run_id,
                scope_seq,
                engine: shared.engine,
                daemon: self.ctx.daemon.clone(),
                autostart: match &self.ctx.config.autostart {
                    Some(a) if a.is_registered() => AutostartView::Registered,
                    Some(_) => AutostartView::NotRegistered,
                    None => AutostartView::Unknown,
                },
                repos: shared
                    .repos
                    .into_iter()
                    .map(|r| RepoSummaryView {
                        repo_id: r.repo_id,
                        state: r.state,
                        path: r.path,
                        // Published by the predictor, Guardrails and
                        // US-GRP-005: until then "not available".
                        attention: AttentionView::unpublished(),
                        tier: r.tier.filter(|_| tiers),
                    })
                    .collect(),
            }),
            Scope::Repo { repo_id } => {
                let mut repos: Vec<_> = shared
                    .repos
                    .into_iter()
                    .filter(|r| r.repo_id == repo_id)
                    .collect();
                crate::observe::refresh_divergence(
                    &mut repos,
                    &shared.divergence,
                    &self.ctx.divergence,
                );
                let mut repo = repos.pop().ok_or_else(not_found_id)?;
                if !self.has(methods::CAP_SCOPE_ACTIVITY.name) {
                    repo.without_activity();
                }
                if !self.has(methods::CAP_OBSERVATION_TIERS.name) {
                    repo.without_tier();
                }
                self.kept_temps(std::slice::from_mut(&mut repo));
                if serde_json::to_vec(&repo).map_or(0, |v| v.len()) > CHANGE_LIST_BUDGET {
                    crate::observe::without_change_lists(&mut repo.worktrees);
                }
                ScopeSnapshot::Repo(RepoSnapshot {
                    run_id,
                    scope_seq,
                    repo,
                })
            }
        })
    }

    /// `scope.subscribe` (N1, N2). Counts against the subscriptions of the
    /// connection and shares their ids with `events.subscribe`.
    fn scope_subscribe(
        &mut self,
        params: ScopeSubscribeParams,
    ) -> Result<ScopeSubscribeResult, ErrorObject> {
        valid_scope(&params.scope)?;
        self.wake_scope(&params.scope);
        if self.subscriptions.len() >= self.ctx.config.limits.subscriptions_per_connection {
            return Err(ErrorObject::new(
                code::LIMIT_REACHED,
                "too many subscriptions",
            ));
        }
        let id = self.next_subscription;
        match self.ctx.bus.subscribe_scope(
            &self.outbox,
            id,
            &params.scope,
            params.from_seq,
            params.run_id.as_deref(),
        ) {
            Subscribed::From(from_seq) => {
                self.next_subscription += 1;
                self.subscriptions.push(id);
                Ok(ScopeSubscribeResult {
                    subscription: id,
                    scope: params.scope,
                    from_seq,
                })
            }
            Subscribed::Resync => Err(ErrorObject::new(
                code::RESYNC_REQUIRED,
                "take a new snapshot of the scope and subscribe again",
            )),
            Subscribed::UnknownScope => Err(not_found_id()),
        }
    }

    /// `repo.locate` (N4): lexical checks before touching the file system,
    /// then the canonical path against the observed worktrees; the deepest
    /// root wins (a linked worktree inside the main one). Anything else is
    /// "not found", whether the path exists or not.
    /// A client that opens one repo (the TUI there, `raptor status` inside
    /// it) wakes it if it is dormant (TS-GRP-006, N4); the fleet does not.
    fn wake_scope(&self, scope: &Scope) {
        if let Scope::Repo { repo_id } = scope {
            self.ctx
                .control
                .wake(repo_id, crate::watch::WakeCause::Sentinel);
        }
    }

    fn repo_locate(&self, params: RepoLocateParams) -> Result<RepoLocateResult, ErrorObject> {
        let path = validate::client_path(&params.path).map_err(invalid)?;
        // The form of the observed worktree roots (on Windows, the drive form).
        let canonical = gitraptor_git::paths::canonicalize(&path).map_err(|_| not_found_id())?;
        let (_, shared) = self.ctx.bus.snapshot();
        shared
            .repos
            .iter()
            .flat_map(|r| r.worktrees.iter().map(move |w| (r, w)))
            .filter(|(_, w)| canonical.starts_with(w.path.raw()))
            .max_by_key(|(_, w)| w.path.raw().len())
            .map(|(r, w)| RepoLocateResult {
                repo_id: r.repo_id.clone(),
                worktree: w.path.clone(),
            })
            .ok_or_else(not_found_id)
    }

    fn subscribe(&mut self, params: SubscribeParams) -> Result<SubscribeResult, ErrorObject> {
        if self.subscriptions.len() >= self.ctx.config.limits.subscriptions_per_connection {
            return Err(ErrorObject::new(
                code::LIMIT_REACHED,
                "too many subscriptions",
            ));
        }
        let id = self.next_subscription;
        self.next_subscription += 1;
        match self.ctx.bus.subscribe(
            &self.outbox,
            id,
            params.from_seq,
            params.run_id.as_deref(),
            self.profile == ConnectionProfile::Mcp,
        ) {
            Subscribed::From(from_seq) => {
                self.subscriptions.push(id);
                Ok(SubscribeResult {
                    subscription: id,
                    from_seq,
                })
            }
            Subscribed::Resync => Err(ErrorObject::new(
                code::RESYNC_REQUIRED,
                "take a new snapshot and subscribe again",
            )),
            Subscribed::UnknownScope => Err(not_found_id()),
        }
    }

    fn unsubscribe(&mut self, params: UnsubscribeParams) -> Result<bool, ErrorObject> {
        self.subscriptions.retain(|s| *s != params.subscription);
        Ok(self.ctx.bus.unsubscribe(&self.outbox, params.subscription))
    }

    /// `events.history` (US-GRP-002): one page of a repo's Git events.
    fn events_history(
        &self,
        params: EventsHistoryParams,
    ) -> Result<EventsHistoryResult, ErrorObject> {
        if !valid_repo_id(&params.repo_id) {
            return Err(ErrorObject::new(code::INVALID_PARAMS, "invalid repo_id"));
        }
        if params
            .worktree
            .as_ref()
            .is_some_and(|w| w.is_empty() || w.len() > 4096 || w.contains('\0'))
        {
            return Err(ErrorObject::new(code::INVALID_PARAMS, "invalid worktree"));
        }
        let before_reset = !self.has(methods::CAP_GIT_RESET.name);
        let without_authorship = !self.authorship_events();
        self.ctx
            .control
            .event_history(params)
            .map(|mut events| {
                // Names and emails only with `events.authorship` (US-GRD-019).
                if without_authorship {
                    events = events
                        .into_iter()
                        .map(gitraptor_api::messages::GitEventView::without_authorship)
                        .collect();
                }
                // A client without the capability (older than protocol 8)
                // cannot read `reset` (US-TMC-004).
                if before_reset {
                    events.retain(|e| e.kind != gitraptor_api::messages::GitEventKind::Reset);
                }
                EventsHistoryResult { events }
            })
            .map_err(repo_command_error)
    }

    /// `sessions.list` (US-GRP-007): the agent sessions of the observed
    /// repos.
    fn sessions_list(
        &self,
        params: gitraptor_api::messages::SessionsListParams,
    ) -> Result<gitraptor_api::messages::SessionsListResult, ErrorObject> {
        if params.repo_id.as_ref().is_some_and(|id| !valid_repo_id(id)) {
            return Err(ErrorObject::new(code::INVALID_PARAMS, "invalid repo_id"));
        }
        self.ctx
            .control
            .sessions_list(params)
            .map(
                |(detection_available, sessions)| gitraptor_api::messages::SessionsListResult {
                    detection_available,
                    sessions,
                },
            )
            .map_err(repo_command_error)
    }

    fn audit_list(&self, params: AuditListParams) -> Result<AuditListResult, ErrorObject> {
        let limit = params.limit.unwrap_or(100).min(MAX_AUDIT_PAGE);
        let rows = self
            .ctx
            .control
            .audit_list(params.after_id.unwrap_or(0), limit)
            .ok_or_else(|| ErrorObject::new(code::INTERNAL, "audit unavailable"))?;
        Ok(AuditListResult {
            entries: rows
                .into_iter()
                .filter_map(|(id, row)| audit_entry(id, &row))
                .collect(),
        })
    }

    /// `daemon.replace`: accepted without the reserved checks only from the
    /// file that now sits at the daemon's launch path, if that file changed
    /// since the daemon started (an upgrade). Anything else is a reserved
    /// stop (SEC-13).
    fn replace(&mut self, request: &Request) -> After {
        let result = request.params::<ReplaceParams>().and_then(|params| {
            // From protocol 9 the number no longer grows with each feature:
            // an upgrade of the same protocol replaces the daemon too, but
            // only from the installed binary (ADR-GRP-016 § 1, SEC-13).
            let same = params.protocol == self.ctx.config.protocol
                && params.protocol >= CAPABILITIES_PROTOCOL;
            if params.protocol < self.ctx.config.protocol
                || (params.protocol == self.ctx.config.protocol && !same)
            {
                return Err(ErrorObject::new(
                    code::INVALID_PARAMS,
                    "only a newer protocol replaces the daemon",
                ));
            }
            let installed = self.is_installed_replacement();
            if same && !installed {
                return Err(ErrorObject::new(
                    code::INVALID_PARAMS,
                    "only the installed binary replaces a daemon of its protocol",
                ));
            }
            if installed {
                self.audit(
                    methods::DAEMON_REPLACE,
                    None,
                    AuditOutcome::Accepted,
                    None,
                    &self.identity_without_walk(),
                    &[],
                )?;
                Ok(true)
            } else {
                self.reserved(&REPLACE_AS_STOP, None).map(|()| false)
            }
        });
        match result {
            Ok(installed) => self.stop_after(request, Ok(()), |requested_by| {
                if installed {
                    StopCause::Replace {
                        requested_by,
                        protocol: self.ctx.config.protocol,
                    }
                } else {
                    StopCause::StopCommand { requested_by }
                }
            }),
            Err(error) => {
                self.reply::<()>(&request.id, Err(error));
                if self.phase == Phase::Ready {
                    After::Continue
                } else {
                    After::Close
                }
            }
        }
    }

    fn is_installed_replacement(&self) -> bool {
        let launch = &self.ctx.launch;
        let Some(now) = file_id(&launch.path) else {
            return false;
        };
        if launch.file == Some(now) {
            return false;
        }
        match self.ctx.procs.read(self.peer.pid) {
            Ok(info) if info.start_us == self.peer.start_us => info
                .exe
                .as_deref()
                .and_then(file_id)
                .is_some_and(|caller| caller == now),
            _ => false,
        }
    }

    /// Answers an accepted stop, then asks the daemon loop to stop: the
    /// answer is written before the connections close.
    fn stop_after(
        &mut self,
        request: &Request,
        result: Result<(), ErrorObject>,
        cause: impl FnOnce(String) -> StopCause,
    ) -> After {
        match result {
            Ok(()) => {
                self.reply(&request.id, Ok(StopResult { stopping: true }));
                let requested_by = format!("client:{}:{}", self.peer.pid, self.peer.start_us);
                self.ctx.control.request(cause(requested_by));
                After::Close
            }
            Err(error) => {
                self.reply::<()>(&request.id, Err(error));
                After::Continue
            }
        }
    }

    fn identity_without_walk(&self) -> ClientIdentity {
        ClientIdentity {
            pid: self.peer.pid,
            start_us: self.peer.start_us,
            exe: self
                .info
                .as_ref()
                .and_then(|i| i.exe.as_deref())
                .map(|p| Untrusted::from_os(p.as_os_str())),
            agent_ancestor: false,
            daemon_descendant: false,
            controlling_terminal: self.info.as_ref().is_some_and(|i| i.controlling_terminal),
            chain_truncated: false,
        }
    }

    /// ADR-GRP-005 § 6: the daemon decides, audits every attempt and only
    /// then answers. A command whose audit cannot be written does not run.
    fn reserved(&self, spec: &MethodSpec, repo_id: Option<String>) -> Result<(), ErrorObject> {
        let verdict = if self.profile == ConnectionProfile::Mcp {
            Verdict {
                client: self.identity_without_walk(),
                chain: Vec::new(),
                refused: Some(RefusalReason::NotAvailableToMcp),
            }
        } else {
            check_reserved(self.peer, &self.ctx.checks())
        };
        let outcome = match (verdict.refused, spec.implemented_by) {
            (Some(_), _) => AuditOutcome::Rejected,
            (None, Some(_)) => AuditOutcome::NotImplemented,
            (None, None) => AuditOutcome::Accepted,
        };
        self.audit(
            spec.name,
            repo_id,
            outcome,
            verdict.refused,
            &verdict.client,
            &verdict.chain,
        )?;
        if let Some(reason) = verdict.refused {
            return Err(
                ErrorObject::new(code::RESERVED_REFUSED, "reserved to the developer")
                    .with_data(RefusedData { reason }),
            );
        }
        if let Some(story) = spec.implemented_by {
            return Err(
                ErrorObject::new(code::NOT_IMPLEMENTED, "not implemented yet")
                    .with_data(serde_json::json!({ "implemented_by": story })),
            );
        }
        Ok(())
    }

    fn audit(
        &self,
        operation: &'static str,
        repo_id: Option<String>,
        outcome: AuditOutcome,
        reason: Option<RefusalReason>,
        client: &ClientIdentity,
        chain: &[ChainLink],
    ) -> Result<(), ErrorObject> {
        let row = AuditRow {
            at_ms: now_ms(),
            operation: operation.to_owned(),
            repo_id,
            outcome: enum_text(&outcome),
            reason: reason.map(|r| enum_text(&r)),
            client: serde_json::to_string(client).unwrap_or_default(),
            chain: serde_json::to_string(chain).unwrap_or_default(),
        };
        let mut fields = vec![
            ("op", Field::Text(operation)),
            ("outcome", Field::Text(outcome_text(outcome))),
        ];
        if let Some(reason) = reason {
            fields.push(("reason", Field::Text(reason_text(reason))));
        }
        let Some(id) = self.ctx.control.audit(row.clone()) else {
            self.ctx.logger.error("reserved_audit_failed", &fields);
            return Err(ErrorObject::new(code::INTERNAL, "audit unavailable"));
        };
        self.ctx.logger.info("reserved_command", &fields);
        if let Some(entry) = audit_entry(id, &row) {
            self.ctx.bus.publish(RESERVED_AUDIT, entry, None, |_| {});
        }
        Ok(())
    }
}

fn not_found() -> ErrorObject {
    ErrorObject::new(code::METHOD_NOT_FOUND, "method not found")
}

/// How the channel serves a method that may modify a repo
/// (ADR-TMC-004 § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteRoute {
    /// Through [`ProtectedOperation`].
    Protected,
    /// A Time Machine command whose story plans it: validated, then "not
    /// implemented", without touching the oplog or the repo.
    TimeMachine(&'static str),
}

fn write_route(name: &str) -> Option<WriteRoute> {
    match name {
        methods::OPERATION_RUN => Some(WriteRoute::Protected),
        methods::TM_UNDO | methods::TM_REDO | methods::TM_RESTORE => Some(WriteRoute::TimeMachine(
            methods::spec(name)?.implemented_by.unwrap_or("US-TMC-002"),
        )),
        _ => None,
    }
}

/// The executor's refusals as contract errors.
fn exec_error(e: ExecError) -> ErrorObject {
    match e {
        ExecError::Rejected(reason) => {
            ErrorObject::new(code::OPERATION_REJECTED, "operation rejected")
                .with_data(RejectedData { reason })
        }
        ExecError::NotImplemented(story) => {
            ErrorObject::new(code::NOT_IMPLEMENTED, "not implemented yet")
                .with_data(serde_json::json!({ "implemented_by": story }))
        }
        ExecError::Invalid(why) => ErrorObject::new(code::INVALID_PARAMS, &why),
        ExecError::TooManyPlans => ErrorObject::new(code::LIMIT_REACHED, "too many open plans"),
        ExecError::Scope(why) => scope_refused(why),
        ExecError::Protected(ProtectedError::Prior {
            reason,
            operation_id,
            ..
        }) => ErrorObject::new(code::PRIOR_SNAPSHOT_FAILED, failure_text(reason)).with_data(
            PriorFailedData {
                reason,
                operation_id,
            },
        ),
        ExecError::Protected(ProtectedError::Oplog(_)) => {
            ErrorObject::new(code::INTERNAL, "oplog unavailable")
        }
        ExecError::Protected(ProtectedError::Step { operation_id, .. }) => {
            ErrorObject::new(code::OPERATION_FAILED, "the operation failed")
                .with_data(serde_json::json!({ "operation_id": operation_id }))
        }
        ExecError::Capture(why) => capture_error(why),
    }
}

/// A manual snapshot that was not taken, as a contract error. None of them leaves a point.
fn capture_error(why: ManualError) -> ErrorObject {
    use catalog::RejectReason as R;
    let rejected = |reason| {
        ErrorObject::new(code::OPERATION_REJECTED, "operation rejected")
            .with_data(RejectedData { reason })
    };
    match why {
        ManualError::Quota(hit) => quota_error(&hit, now_ms()),
        ManualError::NoSpace => ErrorObject::new(
            methods::OPERATION_SNAPSHOT_QUOTA.code,
            "snapshot quota exceeded",
        )
        .with_data(methods::SnapshotQuotaData {
            window: methods::QuotaWindow::Disk,
            retry_after_s: None,
            release_utc_ms: None,
        }),
        ManualError::TimeLimit => ErrorObject::new(
            methods::OPERATION_SNAPSHOT_TIME_LIMIT.code,
            "snapshot time limit",
        ),
        ManualError::InProgress => rejected(R::OperationInProgress),
        ManualError::InFlight => rejected(R::WriteInProgress),
        ManualError::Busy => rejected(R::GitBusy),
        ManualError::Discarded => rejected(R::StateChanged),
        ManualError::Unavailable | ManualError::Capture(_) => {
            ErrorObject::new(code::INTERNAL, "capture failed")
        }
    }
}

/// The `-33060` error of a full window, with the real wait.
fn quota_error(hit: &QuotaHit, now: i64) -> ErrorObject {
    let wait_ms = hit.release_at_ms.saturating_sub(now).max(0);
    ErrorObject::new(
        methods::OPERATION_SNAPSHOT_QUOTA.code,
        "snapshot quota exceeded",
    )
    .with_data(methods::SnapshotQuotaData {
        window: hit.window,
        retry_after_s: Some(u64::try_from(wait_ms).unwrap_or(0).div_ceil(1000)),
        release_utc_ms: Some(hit.release_at_ms),
    })
}

/// Why the MCP scope of a request was refused.
enum McpScopeError {
    Scope(ScopeError),
    Identity,
}

/// An undo's failures as contract errors.
fn undo_error(e: UndoError) -> ErrorObject {
    match e {
        UndoError::Rejected {
            reason,
            operation_id,
        } => {
            ErrorObject::new(code::OPERATION_REJECTED, "undo rejected").with_data(TmRejectedData {
                reason,
                operation_id,
            })
        }
        UndoError::Prior {
            reason,
            operation_id,
        } => ErrorObject::new(code::PRIOR_SNAPSHOT_FAILED, failure_text(reason)).with_data(
            PriorFailedData {
                reason,
                operation_id,
            },
        ),
        UndoError::Interrupted { operation_id, .. } => {
            ErrorObject::new(code::OPERATION_FAILED, "the undo was interrupted")
                .with_data(serde_json::json!({ "operation_id": operation_id }))
        }
        UndoError::Internal(_) => ErrorObject::new(code::INTERNAL, "time machine unavailable"),
    }
}

/// A restore's failures as contract errors.
fn restore_error(e: RestoreError) -> ErrorObject {
    match e {
        RestoreError::NotFound => not_found_id(),
        RestoreError::Rejected {
            reason,
            operation_id,
        } => ErrorObject::new(code::OPERATION_REJECTED, "restore rejected").with_data(
            TmRejectedData {
                reason,
                operation_id,
            },
        ),
        RestoreError::Prior {
            reason,
            operation_id,
        } => ErrorObject::new(code::PRIOR_SNAPSHOT_FAILED, failure_text(reason)).with_data(
            PriorFailedData {
                reason,
                operation_id,
            },
        ),
        RestoreError::Interrupted { operation_id, .. } => {
            ErrorObject::new(code::OPERATION_FAILED, "the restore was interrupted")
                .with_data(serde_json::json!({ "operation_id": operation_id }))
        }
        RestoreError::Internal(_) => ErrorObject::new(code::INTERNAL, "time machine unavailable"),
    }
}

/// What the client sees of a finished restore. Paths and branch names are
/// untrusted text, capped.
fn restore_result(done: RestoreDone, requester: RequesterView) -> RestoreResult {
    let paths = |list: &[PathBuf]| -> Vec<Untrusted> {
        list.iter()
            .take(MAX_REPORTED_PATHS)
            .map(|p| Untrusted::from_os(p.as_os_str()))
            .collect()
    };
    let names = |list: &[String]| -> Vec<Untrusted> {
        list.iter()
            .take(MAX_REPORTED_REFS)
            .map(|n| Untrusted::new(n.clone()))
            .collect()
    };
    RestoreResult {
        operation_id: done.operation_id,
        prior_snapshot_id: done.prior_snapshot_id,
        target_snapshot_id: done.target_snapshot_id,
        requester,
        worktrees: paths(&done.worktrees),
        recreated: paths(&done.recreated),
        refs: names(&done.refs),
        kept_branches: names(&done.kept_branches),
        not_returned_branches: names(&done.not_returned_branches),
        written: done.report.written as u64,
        removed: done.report.removed as u64,
        not_restored: not_restored_of(&done.report),
        warnings: done.report.warnings.iter().map(warning_code).collect(),
    }
}

/// The paths an application left as they were, capped.
fn not_restored_of(report: &ApplyReport) -> Vec<NotRestored> {
    report
        .paths
        .iter()
        .take(MAX_REPORTED_PATHS)
        .map(|p| {
            let (reason, kept_at) = match &p.issue {
                PathIssue::Overlap { kept_at } => (
                    NotRestoredReason::Overlap,
                    kept_at
                        .as_deref()
                        .map(|k| Untrusted::from_os(k.as_os_str())),
                ),
                PathIssue::NotGuaranteed => (NotRestoredReason::NotGuaranteed, None),
                PathIssue::Blocked(_) => (NotRestoredReason::Blocked, None),
            };
            NotRestored {
                path: Untrusted::new(p.path.clone()),
                reason,
                kept_at,
            }
        })
        .collect()
}

/// What the client sees of a finished undo. Paths are untrusted text.
fn undo_result(done: UndoDone, requester: RequesterView) -> UndoResult {
    let not_restored = not_restored_of(&done.report);
    UndoResult {
        operation_id: done.operation_id,
        prior_snapshot_id: done.prior_snapshot_id,
        undone_operation_id: done.undone.id,
        undone_subtype: done.undone.subtype.map(Untrusted::new),
        target_snapshot_id: done.target_snapshot_id,
        requester,
        written: done.report.written as u64,
        removed: done.report.removed as u64,
        not_restored,
        warnings: done
            .report
            .warnings
            .iter()
            .map(warning_code)
            .chain(done.warnings)
            .collect(),
    }
}

fn warning_code(w: &ApplyWarning) -> String {
    match w {
        ApplyWarning::IntentToAddNotRestored { .. } => "intent-to-add-not-restored",
        ApplyWarning::HeadWithoutReflog { .. } => "head-without-reflog",
        ApplyWarning::StashTopOnly => "stash-top-only",
        ApplyWarning::StashKept => "stash-kept",
        ApplyWarning::Excluded { .. } => "excluded",
    }
    .to_owned()
}

fn tm_invalid(why: TmInvalid) -> ErrorObject {
    ErrorObject::new(code::INVALID_PARAMS, &why.message())
}

fn scope_refused(why: ScopeError) -> ErrorObject {
    let reason = match why {
        ScopeError::NoWorkingFolder => ScopeRefusal::NoWorkingFolder,
        ScopeError::NotObserved => ScopeRefusal::NotObserved,
        ScopeError::NotAllowlisted => ScopeRefusal::NotAllowlisted,
        ScopeError::UnattributedOverMcp => ScopeRefusal::UnattributedOverMcp,
        ScopeError::ForeignWorktree => ScopeRefusal::ForeignWorktree,
    };
    ErrorObject::new(code::SCOPE_REFUSED, why.as_str()).with_data(ScopeRefusedData { reason })
}

/// A scope's repo id is checked like any other before it is looked up.
fn valid_scope(scope: &Scope) -> Result<(), ErrorObject> {
    match scope {
        Scope::Repo { repo_id } if !valid_repo_id(repo_id) => {
            Err(ErrorObject::new(code::INVALID_PARAMS, "invalid repo_id"))
        }
        _ => Ok(()),
    }
}

fn not_found_id() -> ErrorObject {
    ErrorObject::new(code::NOT_FOUND, "not found")
}

impl Connection<'_> {
    fn is_mcp(&self) -> bool {
        self.profile == ConnectionProfile::Mcp
    }

    /// The channel of a request: an MCP connection is always `mcp` and may
    /// not label itself.
    fn request_channel(&self, surface: Option<Surface>) -> Result<RequestChannel, ErrorObject> {
        match (self.is_mcp(), surface) {
            (true, None) => Ok(RequestChannel::Mcp),
            (true, Some(_)) => Err(ErrorObject::new(
                code::INVALID_PARAMS,
                "surface: not accepted over MCP",
            )),
            (false, s) => Ok(s.map_or(RequestChannel::Cli, RequestChannel::from)),
        }
    }

    /// The worktree a request names: required on a full connection, absent
    /// over MCP (it comes from the caller's working folder).
    fn named_worktree(&self, raw: Option<&str>) -> Result<Option<PathBuf>, ErrorObject> {
        match (self.is_mcp(), raw) {
            (true, None) => Ok(None),
            (true, Some(_)) => Err(ErrorObject::new(
                code::INVALID_PARAMS,
                "worktree: taken from the caller's working folder over MCP",
            )),
            (false, None) => Err(ErrorObject::new(code::INVALID_PARAMS, "worktree: required")),
            (false, Some(w)) => validate::client_path(w).map(Some).map_err(invalid),
        }
    }

    /// Who is asking, from the kernel's view of the peer (ADR-TMC-005 § 1).
    fn resolve(&self) -> Result<Resolution, ErrorObject> {
        requester::resolve(self.peer, &self.ctx.checks(), Some(&self.ctx.marks)).map_err(|_| {
            ErrorObject::new(
                code::IDENTITY_UNVERIFIED,
                "the caller's identity could not be verified",
            )
        })
    }

    fn requester_view(&self, r: &Resolution, channel: RequestChannel) -> RequesterView {
        RequesterView {
            actor: r.who.actor.clone(),
            channel,
            via: r.via,
            confirmable: r.confirmable,
        }
    }

    fn requester_resolve(&self, request: &Request) -> Result<serde_json::Value, ErrorObject> {
        let p: ResolveParams = request.params()?;
        let channel = self.request_channel(p.surface)?;
        let r = self.resolve()?;
        // Over MCP only the actor: how it was found, and whether it could
        // confirm, would be an unaudited oracle for evasion attempts.
        let value = if self.is_mcp() {
            serde_json::to_value(McpRequesterView {
                actor: r.who.actor,
                channel,
            })
        } else {
            serde_json::to_value(self.requester_view(&r, channel))
        };
        value.map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"))
    }

    fn repo_for(
        &self,
        channel: RequestChannel,
        named: Option<&Path>,
    ) -> Option<Result<RepoHandle, ErrorObject>> {
        let wiring = self.ctx.protected.as_ref()?;
        if channel == RequestChannel::Mcp {
            return Some(self.mcp_repo(wiring.backend.as_ref()).map_err(|e| match e {
                McpScopeError::Scope(why) => scope_refused(why),
                McpScopeError::Identity => ErrorObject::new(
                    code::IDENTITY_UNVERIFIED,
                    "the caller's identity could not be verified",
                ),
            }));
        }
        Some(scope_for(wiring.backend.as_ref(), channel, named, None).map_err(scope_refused))
    }

    /// The repo and worktree of an MCP request (ADR-MCP-001 § 2): the deepest observed
    /// worktree that contains the caller's working folder, read between two checks of the
    /// caller's identity; then the allowlist and the repo's availability.
    fn mcp_repo(
        &self,
        backend: &dyn crate::timemachine::protected::ProtectedBackend,
    ) -> Result<RepoHandle, McpScopeError> {
        let first = self.resolve().map_err(|_| McpScopeError::Identity)?;
        let cwd =
            process_cwd(self.peer.pid).and_then(|p| gitraptor_git::paths::canonicalize(&p).ok());
        let second = self.resolve().map_err(|_| McpScopeError::Identity)?;
        if first.who != second.who {
            return Err(McpScopeError::Identity);
        }
        let cwd = cwd.ok_or(McpScopeError::Scope(ScopeError::NoWorkingFolder))?;
        let (_, shared) = self.ctx.bus.snapshot();
        let (r, w) = super::mcp_scope::locate(&cwd, &shared.repos)
            .ok_or(McpScopeError::Scope(ScopeError::NotObserved))?;
        let view = &shared.repos[r];
        if !backend.allowlist().allows(&view.repo_id) {
            return Err(McpScopeError::Scope(ScopeError::NotAllowlisted));
        }
        if view.state == gitraptor_api::messages::RepoStateView::Unavailable {
            return Err(McpScopeError::Scope(ScopeError::NotObserved));
        }
        let root = PathBuf::from(view.worktrees[w].path.raw());
        let repo = backend.repo_of(&root).map_err(McpScopeError::Scope)?;
        if repo.repo_id != view.repo_id {
            return Err(McpScopeError::Scope(ScopeError::NotObserved));
        }
        Ok(repo)
    }

    fn caller(&self) -> Caller {
        Caller {
            connection: self.id,
            pid: self.peer.pid,
            start_us: self.peer.start_us,
            mcp: self.is_mcp(),
        }
    }

    /// The layer the daemon fixes (ADR-CKP-002 § 4). The test override never
    /// applies to a descendant of the executor (H-01).
    fn layer(&self, r: &Resolution) -> Layer {
        match &self.ctx.protected {
            Some(w) if r.via != gitraptor_api::timemachine::ResolvedVia::Executor => {
                w.test_layer_override.unwrap_or_else(|| layer_for(r))
            }
            _ => layer_for(r),
        }
    }

    fn wiring(&self) -> Result<super::ProtectedWiring, ErrorObject> {
        self.ctx.protected.clone().ok_or_else(|| {
            ErrorObject::new(code::NOT_IMPLEMENTED, "no operation executor yet")
                .with_data(serde_json::json!({ "implemented_by": "F-001-02" }))
        })
    }

    /// What a `snapshot` request passes before anything of the repo is read: the quota as the
    /// oplog stands (so a looping agent gets the real wait, before the write bucket answers a
    /// bare rate limit), then the connection's write budget. The capture counts again under its
    /// lock; this is the early answer.
    fn snapshot_admission(&self, r: &Resolution, repo: &RepoHandle) -> Result<(), ErrorObject> {
        if let (Requester::Agent { session_id, .. }, Some(deps)) =
            (&r.who.requester, &self.ctx.tm_engine)
            && let Some((oplog, Some(store))) = deps.repos.repo(&repo.repo_id)
            && let Ok((_, key)) = manual::resolve_worktree(&repo.worktree)
        {
            let now = manual::wall_now_ms();
            manual::precheck(&oplog, &store, session_id, &key, now)
                .map_err(|hit| quota_error(&hit, now))?;
        }
        if self.is_mcp()
            && !self
                .mcp_write_bucket
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        {
            return Err(ErrorObject::new(code::RATE_LIMITED, "rate limited"));
        }
        Ok(())
    }

    /// `operation.prepare`: the plan, without effects (ADR-CKP-002 § 2).
    fn operation_prepare(&self, request: &Request) -> Result<serde_json::Value, ErrorObject> {
        let p: PrepareParams = request.params()?;
        p.validate().map_err(tm_invalid)?;
        let channel = self.request_channel(p.surface)?;
        let named = self.named_worktree(p.worktree.as_deref())?;
        let snapshot = p.operation == catalog::OperationId::Snapshot;
        if snapshot && !self.has(methods::CAP_OPERATION_SNAPSHOT.name) {
            return Err(
                ErrorObject::new(code::NOT_IMPLEMENTED, "not implemented yet")
                    .with_data(serde_json::json!({ "implemented_by": "US-MCP-008" })),
            );
        }
        let r = self.resolve()?;
        let wiring = self.wiring()?;
        let layer = self.layer(&r);
        let repo = self
            .repo_for(channel, named.as_deref())
            .expect("wiring checked above")?;
        if snapshot {
            self.snapshot_admission(&r, &repo)?;
        }
        let refusal = super::requester::confirmation_refusal(
            self.peer,
            &self.ctx.checks(),
            Some(&self.ctx.marks),
        );
        let prepared = wiring
            .executor
            .prepare(
                wiring.backend.as_ref(),
                PrepareInput {
                    caller: self.caller(),
                    resolution: &r,
                    layer,
                    channel,
                    params: &p,
                    repo,
                    confirm_refusal: refusal,
                },
            )
            .map_err(exec_error)?;
        let result = PrepareResult {
            plan_id: prepared.plan_id,
            catalog_version: catalog::CATALOG_VERSION,
            operation: p.operation,
            layer: prepared.layer,
            requester: self.requester_view(&r, channel),
            fingerprint: prepared.fingerprint,
            warnings: prepared.warnings,
            decision: prepared.decision,
            challenge: prepared.challenge,
            expires_in_ms: prepared.expires_in_ms,
            diagnostics: prepared.diagnostics,
        };
        let value = if self.is_mcp() {
            serde_json::to_value(result.for_mcp())
        } else {
            serde_json::to_value(result)
        };
        value.map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"))
    }

    /// `operation.run`: executes a plan of this connection as a protected
    /// operation (ADR-CKP-002 § 2, step 4).
    fn operation_run(&self, request: &Request) -> Result<serde_json::Value, ErrorObject> {
        let p: RunParams = request.params()?;
        let r = self.resolve()?;
        let wiring = self.wiring()?;
        let checks_again = || {
            let again = self
                .resolve()
                .map_err(|_| ExecError::Rejected(catalog::RejectReason::StateChanged))?;
            let layer = self.layer(&again);
            let refusal = super::requester::confirmation_refusal(
                self.peer,
                &self.ctx.checks(),
                Some(&self.ctx.marks),
            );
            Ok((again, layer, refusal))
        };
        let bus = Arc::clone(&self.ctx.bus);
        let publish = move |kind: &str, data: catalog::OperationEventData| {
            bus.publish(kind, data, None, |_| {});
        };
        // The intent's mark once the engine is calm, and the anchor after the
        // step (US-TMC-004). Without an engine (doubles), the bus sequence.
        let tm_engine = self.ctx.tm_engine.clone();
        let bus_mark = i64::try_from(self.ctx.bus.snapshot().0).unwrap_or(i64::MAX);
        let engine_mark = |repo_id: &str, worktrees: &[std::path::PathBuf]| match &tm_engine {
            Some(deps) => deps.engine.settle(
                repo_id,
                worktrees,
                crate::timemachine::continuous::ANCHOR_SETTLE_LIMIT,
            ),
            None => Some(bus_mark),
        };
        let after_step = |repo_id: &str, worktrees: &[std::path::PathBuf], operation_id: &str| {
            if let Some(deps) = &tm_engine {
                crate::timemachine::continuous::anchor(deps, repo_id, worktrees, operation_id);
            }
        };
        let env = RunEnv {
            marks: &self.ctx.marks,
            procs: self.ctx.procs.as_ref(),
            stopping: &self.ctx.stopping,
            prior_deadline: wiring.prior_deadline,
            engine_mark: &engine_mark,
            after_step: &after_step,
            publish: &publish,
            capture: &|ask: &manual::ManualAsk| match &tm_engine {
                Some(deps) => manual::capture(deps, ask, manual::wall_now_ms()),
                None => Err(ManualError::Unavailable),
            },
        };
        let rescope = |repo: &RepoHandle| -> Result<(), ExecError> {
            if !self.is_mcp() {
                return Ok(());
            }
            match self.mcp_repo(wiring.backend.as_ref()) {
                Ok(now) if now.repo_id == repo.repo_id && now.worktree == repo.worktree => Ok(()),
                Ok(_) | Err(McpScopeError::Identity) => Err(ExecError::Rejected(
                    catalog::RejectReason::RepoIdentityChanged,
                )),
                Err(McpScopeError::Scope(why)) => Err(ExecError::Scope(why)),
            }
        };
        let done = wiring
            .executor
            .run_any(
                wiring.backend.as_ref(),
                RunInput {
                    caller: self.caller(),
                    resolution: &r,
                    params: &p,
                    resolve_again: &checks_again,
                    rescope: &rescope,
                },
                &env,
            )
            .map_err(exec_error)?;
        let done = match done {
            crate::executor::RunDone::Protected(done) => done,
            crate::executor::RunDone::Captured(c) => {
                let name = c
                    .snapshot
                    .worktree
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut requester = self.requester_view(&r, c.channel);
                if self.is_mcp() {
                    // Over MCP, only who: how it was found is an oracle for evasion.
                    requester.via = gitraptor_api::timemachine::ResolvedVia::None;
                    requester.confirmable = false;
                }
                let result = catalog::SnapshotRunResult {
                    snapshot_id: c.snapshot.snapshot_id,
                    worktree: gitraptor_api::UntrustedName::new(name),
                    label: gitraptor_api::UntrustedName::new(c.label),
                    requester,
                    layer: c.layer,
                    outcome: catalog::OperationOutcome::Done,
                };
                return serde_json::to_value(result)
                    .map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"));
            }
        };
        let result = OperationRunResult {
            operation_id: done.outcome.operation_id,
            prior_snapshot_id: done.outcome.prior.snapshot_id,
            fast_path: done.outcome.prior.fast_path,
            requester: self.requester_view(&r, done.channel),
            changed_refs: done
                .outcome
                .output
                .changed_refs
                .into_iter()
                .take(MAX_REPORTED_REFS)
                .map(Untrusted::new)
                .collect(),
            outcome: done.result,
            layer: done.layer,
            git_output: done.git_output.map(Untrusted::new),
        };
        let value = if self.is_mcp() {
            serde_json::to_value(result.for_mcp())
        } else {
            serde_json::to_value(result)
        };
        value.map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"))
    }

    /// `operation.cancel` (BR-CKP-WF-008): layer `cockpit` only.
    fn operation_cancel(&self, request: &Request) -> Result<CancelResult, ErrorObject> {
        let p: CancelParams = request.params()?;
        gitraptor_api::timemachine::check_oplog_id("operation_id", &p.operation_id)
            .map_err(tm_invalid)?;
        let r = self.resolve()?;
        let wiring = self.wiring()?;
        let requested = wiring
            .executor
            .cancel(&r, self.layer(&r), &p.operation_id)
            .map_err(exec_error)?;
        Ok(CancelResult { requested })
    }

    /// `timemachine.undo` (US-TMC-002): the last operation of the named
    /// worktree, as a protected operation of the Time Machine. The
    /// selectors (`since`, `agent`, `operation_id`) stay with their stories.
    fn tm_undo(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<serde_json::Value, ErrorObject> {
        let p: UndoParams = request.params()?;
        p.validate().map_err(tm_invalid)?;
        let selector = if p.since.is_some() {
            Some("US-TMC-010")
        } else if p.agent.is_some() {
            Some("US-TMC-011")
        } else if p.operation_id.is_some() {
            Some("unassigned")
        } else {
            None
        };
        if let Some(story) = selector {
            return self.tm_command(spec, request, story);
        }
        let channel = self.request_channel(p.surface)?;
        let named = self.named_worktree(p.worktree.as_deref())?;
        let r = self.resolve()?;
        require_attributed(channel, r.who.is_agent()).map_err(scope_refused)?;
        let Some(tm) = &self.ctx.time_machine else {
            return Err(ErrorObject::new(code::NOT_IMPLEMENTED, "no repo layer")
                .with_data(serde_json::json!({ "implemented_by": "US-TMC-002" })));
        };
        let cwd = if self.is_mcp() {
            process_cwd(self.peer.pid)
        } else {
            None
        };
        let repo = tm_scope_for(
            tm.backend.as_ref(),
            self.is_mcp(),
            named.as_deref(),
            cwd.as_deref(),
        )
        .map_err(scope_refused)?;
        let env = UndoEnv {
            marks: &self.ctx.marks,
            procs: self.ctx.procs.as_ref(),
            stopping: &self.ctx.stopping,
            prior_deadline: tm.prior_deadline,
            git: tm.git.as_ref(),
            invoker: &tm.invoker,
            engine: self.ctx.tm_engine.as_ref(),
            fallback_mark: i64::try_from(self.ctx.bus.snapshot().0).unwrap_or(i64::MAX),
        };
        let done = undo_last(&repo, &r.who, oplog_channel(channel), &env).map_err(undo_error)?;
        let result = undo_result(done, self.requester_view(&r, channel));
        let value = if self.is_mcp() {
            serde_json::to_value(result.for_mcp())
        } else {
            serde_json::to_value(result)
        };
        value.map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"))
    }

    /// `timemachine.restore` (US-TMC-009): the named worktree back to a
    /// point of the timeline, as a protected operation of the Time Machine.
    /// Not offered over MCP.
    fn tm_restore(
        &self,
        _spec: &MethodSpec,
        request: &Request,
    ) -> Result<serde_json::Value, ErrorObject> {
        let p: RestoreParams = request.params()?;
        p.validate().map_err(tm_invalid)?;
        let channel = self.request_channel(p.surface)?;
        let named = self.named_worktree(p.worktree.as_deref())?;
        let r = self.resolve()?;
        require_attributed(channel, r.who.is_agent()).map_err(scope_refused)?;
        // An id of another repo is "not found", like an unknown one.
        if let Some(Ok(repo)) = self.repo_for(channel, named.as_deref())
            && snapshot_in(&repo, &p.snapshot_id).is_none()
        {
            return Err(not_found_id());
        }
        let Some(tm) = &self.ctx.time_machine else {
            return Err(ErrorObject::new(code::NOT_IMPLEMENTED, "no repo layer")
                .with_data(serde_json::json!({ "implemented_by": "US-TMC-009" })));
        };
        let repo = tm_scope_for(tm.backend.as_ref(), false, named.as_deref(), None)
            .map_err(scope_refused)?;
        let env = UndoEnv {
            marks: &self.ctx.marks,
            procs: self.ctx.procs.as_ref(),
            stopping: &self.ctx.stopping,
            prior_deadline: tm.prior_deadline,
            git: tm.git.as_ref(),
            invoker: &tm.invoker,
            engine: self.ctx.tm_engine.as_ref(),
            fallback_mark: i64::try_from(self.ctx.bus.snapshot().0).unwrap_or(i64::MAX),
        };
        let done = restore_to(&repo, &p.snapshot_id, &r.who, oplog_channel(channel), &env)
            .map_err(restore_error)?;
        serde_json::to_value(restore_result(done, self.requester_view(&r, channel)))
            .map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"))
    }

    /// `timemachine.timeline` (US-TMC-006): the whole repo's operations and Git events, oldest
    /// first, with their actor, protection and changed paths. A read: nothing is written, and
    /// neither commit messages nor contents are read (DS-US-TMC-006 T004). Not offered over MCP.
    fn tm_timeline(&self, request: &Request) -> Result<serde_json::Value, ErrorObject> {
        let p: TimelineParams = request.params()?;
        p.validate().map_err(tm_invalid)?;
        let named = self.named_worktree(p.worktree.as_deref())?;
        let Some(tm) = &self.ctx.time_machine else {
            return Err(ErrorObject::new(code::NOT_IMPLEMENTED, "no repo layer")
                .with_data(serde_json::json!({ "implemented_by": "US-TMC-006" })));
        };
        let repo = tm_scope_for(tm.backend.as_ref(), false, named.as_deref(), None)
            .map_err(scope_refused)?;
        let only_worktree = match &p.only_worktree {
            None => None,
            Some(raw) => {
                let folder = validate::client_path(raw).map_err(invalid)?;
                let other = tm.backend.repo_of(&folder).map_err(scope_refused)?;
                if other.repo.repo_id != repo.repo.repo_id {
                    return Err(scope_refused(ScopeError::ForeignWorktree));
                }
                Some(other.repo.worktree.to_string_lossy().into_owned())
            }
        };
        let now = now_ms();
        let since_ms =
            match p.since.as_deref() {
                None => None,
                Some(text) => {
                    let secs = parse_since(text).map_err(tm_invalid)?;
                    Some(now.saturating_sub(
                        i64::try_from(secs.saturating_mul(1000)).unwrap_or(i64::MAX),
                    ))
                }
            };
        let query = TimelineQuery {
            since_ms,
            agent: p.agent.as_deref().map(AgentFilter::parse),
            only_worktree,
            limit: usize::try_from(p.limit.unwrap_or(TIMELINE_DEFAULT_LIMIT)).unwrap_or(usize::MAX),
        };

        // The engine's side first: its history of the repo, the sessions that say who is who now
        // and, for the echo of GitRaptor's own operations, the raw events of each worktree.
        let history = self
            .ctx
            .control
            .event_history(EventsHistoryParams {
                repo_id: repo.repo.repo_id.clone(),
                // The page is of the worktree asked for: a filter applied after a full page
                // would find fewer entries than exist.
                worktree: query.only_worktree.clone(),
                after_seq: None,
                limit: Some(MAX_HISTORY_PAGE),
            })
            .ok();
        let sessions = self
            .ctx
            .control
            .sessions_list(SessionsListParams {
                repo_id: Some(repo.repo.repo_id.clone()),
                include_ended: true,
                limit: Some(MAX_SESSIONS_PAGE),
            })
            .ok();
        let actors = sessions
            .as_ref()
            .map(|(_, s)| SessionActors::from_sessions(s))
            .unwrap_or_default();
        let engine = history.map(|mut events| {
            // Whether the page was full is a fact of the page, before anything is removed from it.
            let history_full =
                events.len() >= usize::try_from(MAX_HISTORY_PAGE).unwrap_or(usize::MAX);
            let history_oldest_ms = events.iter().map(|e| e.observed_utc_ms).min();
            // A client without the capability cannot read `reset` (US-TMC-004).
            if !self.has(methods::CAP_GIT_RESET.name) {
                events.retain(|e| e.kind != GitEventKind::Reset);
            }
            let mut side = EngineSide {
                detection_available: sessions.as_ref().is_some_and(|(d, _)| *d),
                history_full,
                history_oldest_ms,
                ..EngineSide::default()
            };
            if let Some(deps) = &self.ctx.tm_engine {
                let registered = registered_worktrees(&repo.repo.worktree).unwrap_or_default();
                let floor = deps.engine.generation_floor(&repo.repo.repo_id);
                for root in events.iter().map(|e| e.worktree.raw()) {
                    if side.raw.contains_key(root) {
                        continue;
                    }
                    let path = Path::new(root);
                    let Some(raw) = deps.engine.raw_events(&repo.repo.repo_id, path) else {
                        continue;
                    };
                    side.snapshot_keys
                        .insert(root.to_owned(), worktree_key(&registered, path, 0));
                    side.raw
                        .insert(root.to_owned(), RawSide { events: raw, floor });
                }
            }
            side.events = events;
            side
        });

        // The oplog only while the list is assembled: Git is read after it is let go.
        // The oplog is only read under its lock; the work on what was read happens after it.
        let read = {
            let oplog = repo.repo.oplog.lock().unwrap_or_else(|e| e.into_inner());
            let refs: &dyn SnapshotRefs = match repo.store.as_deref() {
                Some(store) => store,
                None => &AbsentStore,
            };
            OplogRead::read(&oplog, refs, &query)
        };
        let mut result = build_timeline_from(
            &read,
            &query,
            engine.as_ref(),
            &actors,
            (now, gitraptor_git::local_utc_offset_s()),
        );
        if let Some(engine) = &engine
            && result
                .entries
                .iter()
                .any(|e| matches!(e.origin, EntryOrigin::GitEvent { .. }))
        {
            let root = repo.main_root.as_deref().unwrap_or(&repo.repo.worktree);
            let reader = RepoReader::open(root, &ReaderOptions::default()).ok();
            fill_files(
                &mut result,
                &engine.events,
                reader.as_ref().map(|r| r as &dyn PathsSource),
                PathsCache::shared(),
                FILES_BUDGET,
            );
        }
        // The manual points exist only for a connection that declared it understands them.
        if !self.has(methods::CAP_TM_TIMELINE_MANUAL.name) {
            crate::timemachine::timeline::without_manual(&mut result);
        }
        serde_json::to_value(result).map_err(|_| ErrorObject::new(code::INTERNAL, "serialization"))
    }

    /// A Time Machine command declared ahead of its story: strict
    /// parameters, the requester and, when the repo layer is wired, the
    /// scope and the ids are checked; then "not implemented". Nothing is
    /// recorded and nothing is touched.
    fn tm_command(
        &self,
        spec: &MethodSpec,
        request: &Request,
        story: &'static str,
    ) -> Result<serde_json::Value, ErrorObject> {
        let (worktree, surface, operation_id, snapshot_id) = match spec.name {
            methods::TM_UNDO => {
                let p: UndoParams = request.params()?;
                p.validate().map_err(tm_invalid)?;
                (p.worktree, p.surface, p.operation_id, None)
            }
            methods::TM_REDO => {
                let p: RedoParams = request.params()?;
                p.validate().map_err(tm_invalid)?;
                (p.worktree, p.surface, p.operation_id, None)
            }
            methods::TM_RESTORE => {
                let p: RestoreParams = request.params()?;
                p.validate().map_err(tm_invalid)?;
                (p.worktree, p.surface, None, Some(p.snapshot_id))
            }
            methods::TM_SNAPSHOT => {
                let p: SnapshotParams = request.params()?;
                (p.worktree, p.surface, None, None)
            }
            _ => {
                let p: TimelineParams = request.params()?;
                p.validate().map_err(tm_invalid)?;
                (p.worktree, None, None, None)
            }
        };
        let channel = self.request_channel(surface)?;
        let named = self.named_worktree(worktree.as_deref())?;
        let r = self.resolve()?;
        if matches!(spec.name, methods::TM_UNDO | methods::TM_RESTORE) {
            require_attributed(channel, r.who.is_agent()).map_err(scope_refused)?;
        }
        // SECURITY: a command that names no id skips scope resolution only because every branch
        // below answers NOT_IMPLEMENTED and touches nothing. The story that implements the
        // selectors must restore the scope, the allowlist and the requester's identity first.
        // Only a request that names an id has a repo to look it up in; the selectors stay with
        // their stories and are not scoped here.
        if (snapshot_id.is_some() || operation_id.is_some())
            && let Some(repo) = self.repo_for(channel, named.as_deref())
        {
            let repo = repo?;
            if let Some(id) = &snapshot_id
                && snapshot_in(&repo, id).is_none()
            {
                return Err(not_found_id());
            }
            if let Some(id) = &operation_id
                && operation_in(&repo, id).is_none()
            {
                return Err(not_found_id());
            }
        }
        Err(
            ErrorObject::new(code::NOT_IMPLEMENTED, "not implemented yet")
                .with_data(serde_json::json!({ "implemented_by": story })),
        )
    }
}

fn invalid(why: validate::Invalid) -> ErrorObject {
    use validate::Invalid;
    let reason = match why {
        Invalid::Empty => InvalidReason::Empty,
        Invalid::TooLong => InvalidReason::TooLong,
        Invalid::NotAbsolute => InvalidReason::NotAbsolute,
        Invalid::ControlCharacter => InvalidReason::ControlCharacter,
        Invalid::UncOrDevice => InvalidReason::UncOrDevice,
        Invalid::DeviceName => InvalidReason::DeviceName,
        Invalid::AlternateStream => InvalidReason::AlternateStream,
        Invalid::OutsideObserved => InvalidReason::OutsideObserved,
        Invalid::InvalidRef => InvalidReason::InvalidRef,
        Invalid::ReservedName => InvalidReason::ReservedName,
    };
    ErrorObject::new(code::INVALID_PARAMS, why.as_str()).with_data(InvalidData { reason })
}

fn rejected(reason: RepoRejection) -> ErrorObject {
    ErrorObject::new(code::REPO_REJECTED, "repo rejected").with_data(RepoRejectedData { reason })
}

/// The agent a registration declares, with its name validated (M7).
fn declared_agent(agent: &DeclaredAgent) -> Result<Agent, ErrorObject> {
    Ok(match agent {
        DeclaredAgent::ClaudeCode => Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
        },
        DeclaredAgent::Other { name } => Agent {
            kind: AgentKind::Other,
            name: Some(validate::declared_agent_name(name).map_err(invalid)?),
        },
    })
}

fn registration_rejected(reason: RegistrationRejection) -> ErrorObject {
    ErrorObject::new(code::REGISTRATION_REJECTED, "registration rejected")
        .with_data(RegistrationRejectedData { reason })
}

fn registration_error(err: RegistrationError) -> ErrorObject {
    match err {
        RegistrationError::Rejected(reason) => registration_rejected(reason),
        RegistrationError::Internal => ErrorObject::new(code::INTERNAL, "profile unavailable"),
    }
}

fn repo_command_error(err: RepoCommandError) -> ErrorObject {
    match err {
        RepoCommandError::UnknownRepo => rejected(RepoRejection::UnknownRepo),
        RepoCommandError::Internal => ErrorObject::new(code::INTERNAL, "profile unavailable"),
    }
}

/// The kebab-case wire text of a contract enum.
fn enum_text(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn outcome_text(outcome: AuditOutcome) -> &'static str {
    match outcome {
        AuditOutcome::Accepted => "accepted",
        AuditOutcome::Rejected => "rejected",
        AuditOutcome::NotImplemented => "not-implemented",
    }
}

fn reason_text(reason: RefusalReason) -> &'static str {
    match reason {
        RefusalReason::AgentAncestry => "agent-ancestry",
        RefusalReason::SessionLeaderAgent => "session-leader-agent",
        RefusalReason::DaemonDescendant => "daemon-descendant",
        RefusalReason::NoControllingTerminal => "no-controlling-terminal",
        RefusalReason::IdentityUnverified => "identity-unverified",
        RefusalReason::NotAvailableToMcp => "not-available-to-mcp",
        RefusalReason::Unsupported => "unsupported",
        RefusalReason::WorktreeMismatch => "worktree-mismatch",
        RefusalReason::AgentMismatch => "agent-mismatch",
    }
}

/// An audit row as the contract exposes it.
fn audit_entry(id: i64, row: &AuditRow) -> Option<AuditEntry> {
    let text = |s: &str| serde_json::Value::String(s.to_owned());
    Some(AuditEntry {
        id,
        at_ms: row.at_ms,
        operation: row.operation.clone(),
        repo_id: row.repo_id.clone(),
        outcome: serde_json::from_value(text(&row.outcome)).ok()?,
        reason: match &row.reason {
            Some(r) => Some(serde_json::from_value(text(r)).ok()?),
            None => None,
        },
        client: serde_json::from_str(&row.client).ok()?,
    })
}

/// A repo id as the profile makes them: short, hexadecimal and dashes.
fn valid_repo_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_texts_match_the_wire_form() {
        for outcome in [
            AuditOutcome::Accepted,
            AuditOutcome::Rejected,
            AuditOutcome::NotImplemented,
        ] {
            assert_eq!(enum_text(&outcome), outcome_text(outcome));
        }
        for reason in [
            RefusalReason::AgentAncestry,
            RefusalReason::SessionLeaderAgent,
            RefusalReason::DaemonDescendant,
            RefusalReason::NoControllingTerminal,
            RefusalReason::IdentityUnverified,
            RefusalReason::NotAvailableToMcp,
            RefusalReason::Unsupported,
        ] {
            assert_eq!(enum_text(&reason), reason_text(reason));
        }
    }

    /// N7: path and scope refusals carry a typed reason in `data`, one per
    /// cause, besides the log text in `message`.
    #[test]
    fn invalid_path_and_scope_have_typed_reasons() {
        use validate::Invalid;
        let invalids = [
            Invalid::Empty,
            Invalid::TooLong,
            Invalid::NotAbsolute,
            Invalid::ControlCharacter,
            Invalid::UncOrDevice,
            Invalid::DeviceName,
            Invalid::AlternateStream,
            Invalid::OutsideObserved,
            Invalid::InvalidRef,
            Invalid::ReservedName,
        ];
        let reasons: Vec<InvalidReason> = invalids
            .iter()
            .map(|why| {
                let e = invalid(*why);
                assert_eq!(e.code, code::INVALID_PARAMS);
                serde_json::from_value::<InvalidData>(e.data.unwrap())
                    .unwrap()
                    .reason
            })
            .collect();
        assert_eq!(reasons, InvalidReason::ALL);
        let scopes = [
            ScopeError::NoWorkingFolder,
            ScopeError::NotObserved,
            ScopeError::NotAllowlisted,
            ScopeError::UnattributedOverMcp,
            ScopeError::ForeignWorktree,
        ];
        let reasons: Vec<ScopeRefusal> = scopes
            .iter()
            .map(|why| {
                let e = scope_refused(*why);
                assert_eq!(e.code, code::SCOPE_REFUSED);
                serde_json::from_value::<ScopeRefusedData>(e.data.unwrap())
                    .unwrap()
                    .reason
            })
            .collect();
        assert_eq!(reasons, ScopeRefusal::ALL);
    }
}
