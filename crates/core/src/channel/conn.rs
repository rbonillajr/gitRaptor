//! One client connection: accept checks, handshake, dispatch and output.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::framing::{FrameError, MAX_MESSAGE_BYTES, decode_request, read_frame};
use gitraptor_api::messages::{
    AuditEntry, AuditListParams, AuditListResult, AuditOutcome, ClientIdentity, ClientKind,
    ConnectionProfile, Hello, HelloResult, IncompatibleData, McpRepoView, McpSnapshot, NoParams,
    RefusalReason, RefusedData, ReplaceParams, RepoAddParams, RepoRetireParams, Snapshot,
    StopResult, SubscribeParams, SubscribeResult, UnsubscribeParams,
};
use gitraptor_api::methods::{self, METHODS, MethodSpec};
use gitraptor_api::rpc::{ErrorObject, Id, Request, Response, code};

use super::authz::{AcceptedPeer, ChainLink, Verdict, check_reserved};
use super::bus::{Outbox, Subscribed};
use super::peer::{ProcInfo, peer_cred, process_cwd};
use super::validate;
use super::{ServerCtx, file_id};
use crate::daemon::{Field, StopCause, now_ms};
use crate::profile::AuditRow;

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
        match spec.name {
            methods::PING => self.reply(&request.id, request.params::<NoParams>().map(|_| "pong")),
            methods::ENGINE_SNAPSHOT => {
                let result = request.params::<NoParams>().map(|_| self.snapshot());
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
            methods::DAEMON_STOP => {
                let result = request
                    .params::<NoParams>()
                    .and_then(|_| self.reserved(spec, None));
                return self.stop_after(request, result, |requested_by| StopCause::StopCommand {
                    requested_by,
                });
            }
            methods::DAEMON_REPLACE => return self.replace(request),
            _ => {
                // Declared ahead of their stories: validated, authorized and
                // audited, then "not implemented".
                let result = self
                    .pending_params(spec, request)
                    .and_then(|repo| self.reserved(spec, repo));
                self.reply(&request.id, result);
            }
        }
        After::Continue
    }

    /// Strict parameters of a method declared ahead of its story. Returns
    /// the repo it targets, for the audit.
    fn pending_params(
        &self,
        spec: &MethodSpec,
        request: &Request,
    ) -> Result<Option<String>, ErrorObject> {
        match spec.name {
            methods::REPO_ADD => {
                let p: RepoAddParams = request.params()?;
                validate::client_path(&p.path).map_err(invalid)?;
                Ok(None)
            }
            methods::REPO_RETIRE => {
                let p: RepoRetireParams = request.params()?;
                let ok = !p.repo_id.is_empty()
                    && p.repo_id.len() <= 64
                    && p.repo_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
                if !ok {
                    return Err(ErrorObject::new(code::INVALID_PARAMS, "invalid repo_id"));
                }
                Ok(Some(p.repo_id))
            }
            _ => request.params::<NoParams>().map(|_| None),
        }
    }

    fn snapshot(&self) -> serde_json::Value {
        let (seq, shared) = self.ctx.bus.snapshot();
        let run_id = self.ctx.bus.run_id().to_owned();
        match self.profile {
            ConnectionProfile::Full => serde_json::to_value(Snapshot {
                run_id,
                seq,
                engine: shared.engine,
                daemon: self.ctx.daemon.clone(),
                repos: shared.repos,
            }),
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

fn invalid(why: validate::Invalid) -> ErrorObject {
    ErrorObject::new(code::INVALID_PARAMS, why.as_str())
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
