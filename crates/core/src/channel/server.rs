//! The server of the channel (TS-GRP-004): a Unix socket, or a named pipe on
//! Windows (DS-TS-GRP-004 § 8).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::messages::DaemonView;

use super::ProtectedWiring;
use super::marks::ExecutorMarks;
use super::{ChannelConfig, EventBus, authz, conn, peer, transport};
use crate::daemon::{Logger, ShutdownHandle};

/// The file the daemon was launched from: a `daemon.replace` is only
/// accepted from the file that now sits at that path, if it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchIdentity {
    pub path: PathBuf,
    pub file: Option<(u64, u64)>,
}

impl LaunchIdentity {
    pub fn capture(path: PathBuf) -> Self {
        let file = file_id(&path);
        Self { path, file }
    }
}

/// `(device, inode)` of the file at `path` itself (not followed).
fn socket_id(path: &std::path::Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path)
            .ok()
            .map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// `(device, inode)` of the file at `path`, following symlinks.
pub(crate) fn file_id(path: &std::path::Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Everything connection threads share.
pub(crate) struct ServerCtx {
    pub config: ChannelConfig,
    pub uid: u32,
    pub bus: Arc<EventBus>,
    pub control: ShutdownHandle,
    pub logger: Logger,
    pub instance_id: String,
    pub daemon: DaemonView,
    pub launch: LaunchIdentity,
    /// `(pid, start)` of this daemon: its descendants never pass the
    /// reserved checks (DEP-MCP-3).
    pub daemon_id: Option<(u32, u64)>,
    pub procs: Box<dyn peer::ProcSource + Send + Sync>,
    pub conns: Mutex<conn::ConnTable>,
    pub stopping: AtomicBool,
    pub runtime: PathBuf,
    /// Processes started by running protected operations (DEP-MCP-3).
    pub marks: Arc<ExecutorMarks>,
    /// Repos, executor and allowlist for protected operations; `None` until
    /// the executor of F-001-02 is wired.
    pub protected: Option<ProtectedWiring>,
    /// The Time Machine's own commands; `None` without a repo layer.
    pub time_machine: Option<super::TimeMachineWiring>,
    /// The engine as the Time Machine reads it, and the anchor capture
    /// (US-TMC-004); `None` without the daemon's repo layer.
    pub tm_engine: Option<crate::timemachine::continuous::CaptureDeps>,
    /// Ahead/behind counts the snapshots already walked (US-GRP-012).
    pub divergence: crate::observe::DivergenceCache,
    /// The engine's own consumption (`engine.resources`, US-GRP-017).
    pub resources: Arc<crate::resources::ResourceMonitor>,
    /// Protected repos for `guard.evaluate` (US-GRD-001).
    pub guard: Arc<crate::guardrails::GuardRegistry>,
    /// The MCP allowlist, written by the daemon's loop (US-MCP-002).
    pub mcp_repos: Arc<crate::timemachine::protected::McpRepos>,
    /// `git` processes whose commit already had its authorship decision (DS-US-GRD-018 D6).
    pub commit_decisions: crate::guardrails::second_line::Decided,
}

impl ServerCtx {
    pub(crate) fn checks(&self) -> authz::Checks<'_> {
        authz::Checks {
            uid: self.uid,
            procs: self.procs.as_ref(),
            matcher: &self.config.agents,
            daemon: self.daemon_id,
            marks: Some(&self.marks),
            terminal_proof: authz::TERMINAL_PROOF,
        }
    }
}

/// The channel of a running daemon.
pub struct Server {
    ctx: Arc<ServerCtx>,
    accept: Option<JoinHandle<()>>,
    /// `(dev, inode)` of the socket file this server bound.
    socket: Option<(u64, u64)>,
    #[cfg(unix)]
    wake: Option<std::os::fd::OwnedFd>,
    #[cfg(windows)]
    wake: Option<gitraptor_winsys::pipe::PipeWaker>,
}

/// Instances of the channel pipe at most: the connection limits
/// (`ChannelLimits`) refuse clients above theirs, and this caps the pipe
/// handles a local flood can make the daemon hold.
#[cfg(windows)]
const PIPE_INSTANCES: u32 = 64;

/// A bound but not yet serving channel. Binding happens in
/// `Daemon::start`, before the daemon creates any thread (the long-path
/// bind changes the process working folder).
pub struct BoundChannel {
    #[cfg(unix)]
    listener: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    listener: gitraptor_winsys::pipe::PipeListener,
    runtime: PathBuf,
}

impl BoundChannel {
    /// Binds the socket in `runtime` (SEC-01).
    #[cfg(unix)]
    pub fn bind(runtime: &std::path::Path) -> Result<Self, crate::profile::ProfileError> {
        Ok(Self {
            listener: transport::bind(runtime)?,
            runtime: runtime.to_path_buf(),
        })
    }

    /// Creates the channel pipe of `runtime` (SEC-01, DS-TS-GRP-004 § 8).
    #[cfg(windows)]
    pub fn bind(runtime: &std::path::Path) -> Result<Self, crate::profile::ProfileError> {
        Ok(Self {
            listener: transport::bind(runtime, PIPE_INSTANCES)?,
            runtime: runtime.to_path_buf(),
        })
    }

    pub fn runtime(&self) -> &std::path::Path {
        &self.runtime
    }
}

/// What the daemon hands the channel when it starts serving.
pub(crate) struct ServeArgs {
    pub config: ChannelConfig,
    pub bus: Arc<EventBus>,
    pub control: ShutdownHandle,
    pub logger: Logger,
    pub instance_id: String,
    pub daemon: DaemonView,
    pub protected: Option<ProtectedWiring>,
    pub time_machine: Option<super::TimeMachineWiring>,
    pub tm_engine: Option<crate::timemachine::continuous::CaptureDeps>,
    pub resources: Arc<crate::resources::ResourceMonitor>,
    pub guard: Arc<crate::guardrails::GuardRegistry>,
    pub mcp_repos: Arc<crate::timemachine::protected::McpRepos>,
}

impl Server {
    pub(crate) fn serve(bound: BoundChannel, args: ServeArgs) -> std::io::Result<Self> {
        let launch = LaunchIdentity::capture(
            args.config
                .launch_exe
                .clone()
                .or_else(|| std::env::current_exe().ok())
                .unwrap_or_default(),
        );
        if args.config.agents.is_override() {
            args.logger.warn("agent_matcher_override", &[]);
        }
        let procs = peer::SystemProcs;
        let daemon_id = peer::ProcSource::read(&procs, std::process::id())
            .ok()
            .map(|me| (me.pid, me.start_us));
        let ctx = Arc::new(ServerCtx {
            uid: args.config.expected_uid.unwrap_or_else(peer::current_uid),
            config: args.config,
            bus: args.bus,
            control: args.control,
            logger: args.logger,
            instance_id: args.instance_id,
            daemon: args.daemon,
            launch,
            daemon_id,
            procs: Box::new(procs),
            conns: Mutex::new(conn::ConnTable::default()),
            stopping: AtomicBool::new(false),
            runtime: bound.runtime,
            marks: Arc::new(ExecutorMarks::default()),
            protected: args.protected,
            time_machine: args.time_machine,
            tm_engine: args.tm_engine,
            divergence: crate::observe::DivergenceCache::default(),
            resources: args.resources,
            guard: args.guard,
            mcp_repos: args.mcp_repos,
            commit_decisions: Default::default(),
        });
        Self::listen(ctx, bound.listener)
    }

    #[cfg(unix)]
    fn listen(
        ctx: Arc<ServerCtx>,
        listener: std::os::unix::net::UnixListener,
    ) -> std::io::Result<Self> {
        let socket = socket_id(&transport::socket_path(&ctx.runtime));
        // The accept loop waits on the listener and on a wake-up pipe: the
        // socket path may belong to an impostor by the time we stop, so we
        // cannot wake it by connecting to it.
        let (wake_rx, wake_tx) = rustix::pipe::pipe()?;
        listener.set_nonblocking(true)?;
        let accept_ctx = Arc::clone(&ctx);
        let accept = std::thread::Builder::new()
            .name("raptor-accept".into())
            .spawn(move || accept_loop(&accept_ctx, &listener, &wake_rx))?;
        Ok(Self {
            ctx,
            accept: Some(accept),
            socket,
            wake: Some(wake_tx),
        })
    }

    #[cfg(windows)]
    fn listen(
        ctx: Arc<ServerCtx>,
        listener: gitraptor_winsys::pipe::PipeListener,
    ) -> std::io::Result<Self> {
        let wake = listener.waker();
        let accept_ctx = Arc::clone(&ctx);
        let accept = std::thread::Builder::new()
            .name("raptor-accept".into())
            .spawn(move || pipe_accept_loop(&accept_ctx, &listener))?;
        Ok(Self {
            ctx,
            accept: Some(accept),
            socket: None,
            wake: Some(wake),
        })
    }

    /// Whether the socket file is still the one this server bound. Another
    /// process of the user can remove it and bind its own, cutting clients
    /// off from the real daemon without any crash being recorded.
    ///
    /// Windows: there is no file to replace and the pipe name is never free
    /// while the daemon serves (DS-TS-GRP-004 § 8, P4 and P8); the channel
    /// is intact while its accept thread runs.
    pub fn socket_intact(&self) -> bool {
        if cfg!(windows) {
            return self
                .accept
                .as_ref()
                .is_some_and(|accept| !accept.is_finished());
        }
        self.socket.is_some()
            && socket_id(&transport::socket_path(&self.ctx.runtime)) == self.socket
    }

    pub fn runtime(&self) -> &std::path::Path {
        &self.ctx.runtime
    }

    /// Stops accepting, lets every connection write what it has queued
    /// (the answer to a stop command included) and closes them.
    pub fn shutdown(&mut self) {
        self.ctx.stopping.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        #[cfg(unix)]
        if let Some(wake) = self.wake.take() {
            let _ = rustix::io::write(&wake, &[1]);
        }
        #[cfg(windows)]
        if let Some(wake) = self.wake.take() {
            wake.wake();
        }
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        conn::close_all(&self.ctx, Duration::from_secs(1));
        // Only our own socket: a replaced one is not ours to remove.
        #[cfg(unix)]
        if self.socket_intact() {
            let _ = std::fs::remove_file(transport::socket_path(&self.ctx.runtime));
        }
    }
}

#[cfg(unix)]
fn accept_loop(
    ctx: &Arc<ServerCtx>,
    listener: &std::os::unix::net::UnixListener,
    wake: &std::os::fd::OwnedFd,
) {
    use rustix::event::{PollFd, PollFlags, poll};
    loop {
        let mut fds = [
            PollFd::new(listener, PollFlags::IN),
            PollFd::new(wake, PollFlags::IN),
        ];
        match poll(&mut fds, None) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return,
        }
        if ctx.stopping.load(Ordering::SeqCst) || !fds[1].revents().is_empty() {
            return;
        }
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_ok() {
                        conn::accept(ctx, stream);
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    }
}

/// Consecutive accept failures after which the pipe is given up: the
/// accept thread ends, `socket_intact` turns false and the daemon binds
/// again (as on Unix when the socket file is replaced).
#[cfg(windows)]
const MAX_ACCEPT_ERRORS: u32 = 20;

#[cfg(windows)]
fn pipe_accept_loop(ctx: &Arc<ServerCtx>, listener: &gitraptor_winsys::pipe::PipeListener) {
    let mut errors = 0;
    loop {
        if ctx.stopping.load(Ordering::SeqCst) {
            return;
        }
        match listener.accept() {
            Ok(Some(stream)) => {
                errors = 0;
                conn::accept(ctx, stream);
            }
            Ok(None) => return,
            Err(_) => {
                errors += 1;
                ctx.logger.warn("channel_accept_failed", &[]);
                // Transient (out of resources, say): back off and retry,
                // waking at once if the server stops.
                if errors >= MAX_ACCEPT_ERRORS || listener.wait_woken(Duration::from_millis(100)) {
                    return;
                }
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.accept.is_some() {
            self.shutdown();
        }
    }
}
