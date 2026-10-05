//! One client connection: accept checks, handshake, dispatch and output.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::catalog::{
    self as catalog, CancelParams, CancelResult, Layer, PrepareParams, PrepareResult, RejectedData,
    RunParams,
};
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::framing::{FrameError, MAX_MESSAGE_BYTES, decode_request, read_frame};
use gitraptor_api::messages::{
    AuditEntry, AuditListParams, AuditListResult, AuditOutcome, ClientIdentity, ClientKind,
    ConnectionProfile, EventsHistoryParams, EventsHistoryResult, Hello, HelloResult,
    IncompatibleData, McpRepoView, McpSnapshot, NoParams, RefusalReason, RefusedData,
    ReplaceParams, RepoAddParams, RepoAddResult, RepoRejectedData, RepoRejection, RepoRetireParams,
    RepoRetireResult, Snapshot, StopResult, SubscribeParams, SubscribeResult, UnsubscribeParams,
};
use gitraptor_api::methods::{self, METHODS, MethodSpec};
use gitraptor_api::rpc::{ErrorObject, Id, Request, Response, code};
use gitraptor_api::timemachine::{
    Invalid as TmInvalid, MAX_REPORTED_REFS, McpRequesterView, OperationRunResult, PriorFailedData,
    RedoParams, RequestChannel, RequesterView, ResolveParams, RestoreParams, SnapshotParams,
    Surface, TimelineParams, UndoParams,
};

use super::authz::{AcceptedPeer, ChainLink, Verdict, check_reserved};
use super::bus::{Outbox, Subscribed};
use super::peer::{ProcInfo, peer_cred, process_cwd};
use super::requester::{self, Resolution};
use super::validate;
use super::{ServerCtx, file_id};
use crate::daemon::{
    CHANGE_LIST_BUDGET, Field, RepoAddRequest, RepoCommandError, StopCause, now_ms,
};
use crate::executor::{Caller, ExecError, PrepareInput, RunEnv, RunInput, layer_for};
use crate::profile::AuditRow;
use crate::timemachine::protected::scope::{
    operation_in, require_attributed, scope_for, snapshot_in,
};
use crate::timemachine::protected::{ProtectedError, RepoHandle, ScopeError, failure_text};

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
};

struct ConnEntry {
    id: u64,
    client: (u32, u64),
    stream: UnixStream,
    outbox: Arc<Outbox>,
    writer: Option<std::thread::JoinHandle<()>>,
}

/// Open connections.
#[derive(Default)]
pub(crate) struct ConnTable {
    next_id: u64,
    entries: Vec<ConnEntry>,
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// Checks a freshly accepted socket and, if it passes, serves it on its own
/// thread.
pub(crate) fn accept(ctx: &Arc<ServerCtx>, stream: UnixStream) {
    let accepted_us = now_us();
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
                phase: Phase::Handshake,
                subscriptions: Vec::new(),
                next_subscription: 1,
                bucket: Bucket::new(
                    thread_ctx.config.limits.rate_per_sec,
                    thread_ctx.config.limits.burst,
                ),
                reserved_bucket: Bucket::new(1, RESERVED_BURST),
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

fn refuse_connection(stream: &UnixStream, timeout: Duration) {
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
    stream: &UnixStream,
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
    phase: Phase,
    subscriptions: Vec<u32>,
    next_subscription: u32,
    bucket: Bucket,
    reserved_bucket: Bucket,
}

/// What a handled request leads to.
enum After {
    Continue,
    Close,
}

impl Connection<'_> {
    fn serve(&mut self, stream: UnixStream) {
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
        if hello.protocol != self.ctx.config.protocol {
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
        let result = HelloResult {
            protocol: self.ctx.config.protocol,
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
        };
        self.reply(&request.id, Ok(result));
        self.phase = Phase::Ready;
        After::Continue
    }

    fn offered(&self, m: &MethodSpec) -> bool {
        m.name != methods::HELLO && (self.profile == ConnectionProfile::Full || m.mcp)
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
        // other method outside the MCP allowlist does not exist for it.
        if !self.offered(spec) && !spec.reserved {
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
        // Every method that may modify a repo is served by one route, and
        // the only route that runs anything is the protected operation.
        if let Some(route) = write_route(spec.name) {
            let result = match route {
                WriteRoute::Protected => self.operation_run(request),
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
            methods::TM_SNAPSHOT | methods::TM_TIMELINE => {
                let story = spec.implemented_by.unwrap_or("US-TMC-006");
                let result = self.tm_command(spec, request, story);
                self.reply(&request.id, result);
            }
            methods::PING => self.reply(&request.id, request.params::<NoParams>().map(|_| "pong")),
            methods::ENGINE_SNAPSHOT => {
                let result = request.params::<NoParams>().map(|_| self.snapshot());
                self.reply(&request.id, result);
            }
            methods::ENGINE_RESOURCES => {
                let result = request
                    .params::<NoParams>()
                    .map(|_| self.ctx.resources.read());
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

    /// `repo.add` (US-GRP-001): parameters checked lexically, then the
    /// daemon authorizes and audits, and only then is the path read. An
    /// agent cannot make the daemon probe the file system.
    fn repo_add(&self, spec: &MethodSpec, request: &Request) -> Result<RepoAddResult, ErrorObject> {
        let params: RepoAddParams = request.params()?;
        let path = validate::client_path(&params.path).map_err(invalid)?;
        self.reserved(spec, None)?;
        let t_recv = gitraptor_api::clock::monotonic_ns();
        let common_dir = crate::observe::locate(&path).map_err(rejected)?;
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
            .map_err(repo_command_error)
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
                let cwd = process_cwd(self.peer.pid);
                let caller_repo = cwd.and_then(|cwd| {
                    shared.repos.iter().find_map(|r| {
                        let common = std::path::Path::new(r.path.raw());
                        let worktree = if common.file_name().is_some_and(|n| n == ".git") {
                            common.parent().unwrap_or(common)
                        } else {
                            common
                        };
                        cwd.starts_with(worktree).then(|| McpRepoView {
                            repo_id: r.repo_id.clone(),
                            state: r.state,
                        })
                    })
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
        self.ctx
            .control
            .event_history(params)
            .map(|events| EventsHistoryResult { events })
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
            if params.protocol <= self.ctx.config.protocol {
                return Err(ErrorObject::new(
                    code::INVALID_PARAMS,
                    "only a newer protocol replaces the daemon",
                ));
            }
            if self.is_installed_replacement() {
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
    }
}

fn tm_invalid(why: TmInvalid) -> ErrorObject {
    ErrorObject::new(code::INVALID_PARAMS, &why.message())
}

fn scope_refused(why: ScopeError) -> ErrorObject {
    ErrorObject::new(code::SCOPE_REFUSED, why.as_str())
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
        let cwd = if channel == RequestChannel::Mcp {
            process_cwd(self.peer.pid)
        } else {
            None
        };
        Some(
            scope_for(wiring.backend.as_ref(), channel, named, cwd.as_deref())
                .map_err(scope_refused),
        )
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

    /// `operation.prepare`: the plan, without effects (ADR-CKP-002 § 2).
    fn operation_prepare(&self, request: &Request) -> Result<serde_json::Value, ErrorObject> {
        let p: PrepareParams = request.params()?;
        p.validate().map_err(tm_invalid)?;
        let channel = self.request_channel(p.surface)?;
        let named = self.named_worktree(p.worktree.as_deref())?;
        let r = self.resolve()?;
        let wiring = self.wiring()?;
        let layer = self.layer(&r);
        let repo = self
            .repo_for(channel, named.as_deref())
            .expect("wiring checked above")?;
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
        let env = RunEnv {
            marks: &self.ctx.marks,
            procs: self.ctx.procs.as_ref(),
            stopping: &self.ctx.stopping,
            prior_deadline: wiring.prior_deadline,
            engine_mark: i64::try_from(self.ctx.bus.snapshot().0).unwrap_or(i64::MAX),
            publish: &publish,
        };
        let done = wiring
            .executor
            .run(
                wiring.backend.as_ref(),
                RunInput {
                    caller: self.caller(),
                    resolution: &r,
                    params: &p,
                    resolve_again: &checks_again,
                },
                &env,
            )
            .map_err(exec_error)?;
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
        if let Some(repo) = self.repo_for(channel, named.as_deref()) {
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
    ErrorObject::new(code::INVALID_PARAMS, why.as_str())
}

fn rejected(reason: RepoRejection) -> ErrorObject {
    ErrorObject::new(code::REPO_REJECTED, "repo rejected").with_data(RepoRejectedData { reason })
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
}
