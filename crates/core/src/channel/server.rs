//! The Unix-socket server of the channel (TS-GRP-004).

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
    /// Ahead/behind counts the snapshots already walked (US-GRP-012).
    pub divergence: crate::observe::DivergenceCache,
}

impl ServerCtx {
    pub(crate) fn checks(&self) -> authz::Checks<'_> {
        authz::Checks {
            uid: self.uid,
            procs: self.procs.as_ref(),
            matcher: &self.config.agents,
            daemon: self.daemon_id,
            marks: Some(&self.marks),
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
}

/// A bound but not yet serving channel. Binding happens in
/// `Daemon::start`, before the daemon creates any thread (the long-path
/// bind changes the process working folder).
pub struct BoundChannel {
    #[cfg(unix)]
    listener: std::os::unix::net::UnixListener,
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
}

impl Server {
    #[cfg(unix)]
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
            divergence: crate::observe::DivergenceCache::default(),
        });
        let listener = bound.listener;
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

    /// Whether the socket file is still the one this server bound. Another
    /// process of the user can remove it and bind its own, cutting clients
    /// off from the real daemon without any crash being recorded.
    pub fn socket_intact(&self) -> bool {
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
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        conn::close_all(&self.ctx, Duration::from_secs(1));
        // Only our own socket: a replaced one is not ours to remove.
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

impl Drop for Server {
    fn drop(&mut self) {
        if self.accept.is_some() {
            self.shutdown();
        }
    }
}
