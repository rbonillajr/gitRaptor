//! Client library shared by `raptor` and `raptor-mcp` (TS-GRP-004,
//! ADR-GRP-005 § 3 and § 5): the engine's side of it.
//!
//! The connection, the L-06 checks and the handshake live in
//! [`gitraptor_api::client`] (INF-CKP-001 Entrega 2b). This module resolves
//! the profile and implements how a daemon is started: if none runs, it starts
//! the installed binary with a clean environment (SEC-10), or asks the service
//! manager when the login autostart is registered (US-GRP-004,
//! [`crate::autostart`]), and waits for an old daemon to release the instance
//! lock (SEC-13). The TUI gets these as a [`Launch`] built by the binary
//! (ADR-CKP-003, Enmienda 2026-10-08).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::client as api;
pub use gitraptor_api::client::{ClientError, Connect, Incoming, Launch, NeverLaunch};
use gitraptor_api::messages::ClientKind;

use crate::profile::ProfileDirs;

#[cfg(any(unix, windows))]
use {
    crate::daemon::wait_until_released,
    std::ffi::OsString,
    std::process::{Command, Stdio},
};

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
    /// The capabilities this binary understands (ADR-GRP-016 § 1): a daemon
    /// of the same protocol that lacks one is replaced. Every one of the
    /// contract; tests play a newer binary with more.
    pub capabilities: Vec<&'static str>,
}

impl ClientOptions {
    pub fn new(dirs: ProfileDirs, kind: ClientKind) -> Self {
        Self {
            dirs,
            kind,
            protocol: PROTOCOL_VERSION,
            launcher: Launcher::installed(),
            start_timeout: Duration::from_secs(5),
            capabilities: gitraptor_api::capability::all().map(|c| c.name).collect(),
        }
    }
}

impl ClientOptions {
    /// Where and how to connect, for [`gitraptor_api::client::ensure_daemon_with`].
    pub fn connect(&self) -> Result<Connect, ClientError> {
        Ok(Connect {
            runtime: runtime(&self.dirs)?.to_path_buf(),
            kind: self.kind,
            protocol: self.protocol,
            start_timeout: self.start_timeout,
            capabilities: self.capabilities.clone(),
        })
    }

    /// How this client starts a daemon that is not running.
    pub fn launcher(&self) -> InstalledLauncher {
        InstalledLauncher {
            launcher: self.launcher.clone(),
            dirs: self.dirs.clone(),
        }
    }
}

/// The runtime folder of the profile: where the channel is.
fn runtime(dirs: &ProfileDirs) -> Result<&Path, ClientError> {
    dirs.runtime
        .as_deref()
        .ok_or(ClientError::Unsupported("no runtime folder"))
}

/// A greeted connection to the daemon: [`gitraptor_api::client::Client`],
/// opened from the profile.
pub struct Client(api::Client);

impl std::ops::Deref for Client {
    type Target = api::Client;

    fn deref(&self) -> &api::Client {
        &self.0
    }
}

impl std::ops::DerefMut for Client {
    fn deref_mut(&mut self) -> &mut api::Client {
        &mut self.0
    }
}

impl Client {
    /// Connects and greets. Does not start a daemon.
    pub fn connect(
        dirs: &ProfileDirs,
        kind: ClientKind,
        protocol: u32,
    ) -> Result<Self, ClientError> {
        api::Client::connect(runtime(dirs)?, kind, protocol).map(Self)
    }

    /// Connects to the channel in a runtime folder fixed beforehand (the constant of a
    /// Guardrails dispatcher, ADR-GRD-003 § 4). Before sending anything, `verify` gets the
    /// server's pid as the kernel reports it; if it refuses, nothing is sent
    /// ([`ClientError::NotAuthentic`]). Does not start a daemon.
    pub fn connect_runtime(
        runtime: &Path,
        kind: ClientKind,
        protocol: u32,
        verify: impl FnOnce(u32) -> bool,
    ) -> Result<Self, ClientError> {
        api::Client::connect_runtime(runtime, kind, protocol, verify).map(Self)
    }

    /// The connection of the client library.
    pub fn into_inner(self) -> api::Client {
        self.0
    }
}

/// Connects to the daemon, starting it if needed (ADR-GRP-005 § 3) and
/// replacing an older one (SEC-13).
pub fn ensure_daemon(options: &ClientOptions) -> Result<Client, ClientError> {
    ensure_daemon_with(options, &mut || {})
}

/// [`ensure_daemon`], calling `on_launch` right before a daemon is started,
/// so a client can say "starting the engine" apart from "connecting"
/// (ADR-CKP-003 § 4).
pub fn ensure_daemon_with(
    options: &ClientOptions,
    on_launch: &mut dyn FnMut(),
) -> Result<Client, ClientError> {
    let connect = options.connect()?;
    let mut launcher = options.launcher();
    api::ensure_daemon_with(&connect, &mut launcher, on_launch).map(Client)
}

/// How `raptor` starts a daemon: the installed binary, through the service
/// manager when the login autostart is registered; and how it waits for an
/// old one to release the instance lock.
#[derive(Debug, Clone)]
pub struct InstalledLauncher {
    launcher: Launcher,
    dirs: ProfileDirs,
}

impl Launch for InstalledLauncher {
    #[cfg(any(unix, windows))]
    fn launch(&mut self) -> Result<(), ClientError> {
        launch(&self.launcher, &self.dirs)
    }

    #[cfg(not(any(unix, windows)))]
    fn launch(&mut self) -> Result<(), ClientError> {
        Err(ClientError::TransportUnsupported)
    }

    #[cfg(any(unix, windows))]
    fn wait_released(&mut self, timeout: Duration) -> bool {
        wait_until_released(&self.dirs.state, timeout).unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    fn wait_released(&mut self, _timeout: Duration) -> bool {
        false
    }
}

/// Starts `<raptor> daemon` detached, with a clean environment built by
/// allowlist and the working folder in the profile (SEC-10).
#[cfg(any(unix, windows))]
fn launch(launcher: &Launcher, dirs: &ProfileDirs) -> Result<(), ClientError> {
    let Launcher::Installed(exe) = launcher else {
        return Err(ClientError::NotRunning);
    };
    // Registered autostart: the service manager starts it, with its own
    // environment (ADR-GRP-005 § 3, US-GRP-004).
    if crate::autostart::Autostart::for_current_user().is_some_and(|a| a.start(exe)) {
        return Ok(());
    }
    let cwd = if dirs.state.is_dir() {
        dirs.state.clone()
    } else if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/")
    };
    let mut command = Command::new(exe);
    // Its own process group and no console: the client's Ctrl-C or closed
    // window does not reach the daemon.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        // Nothing of the client reaches the daemon: an inherited pipe of the
        // caller's (`raptor status | more`, the MCP's stdio) would stay open
        // for the daemon's whole life.
        gitraptor_winsys::process::keep_std_handles_private();
    }
    let mut child = command
        .arg("daemon")
        .env_clear()
        .envs(clean_env())
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(ClientError::Launch)?;
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
/// the test overrides ([`debug_overrides`]).
#[cfg(unix)]
pub fn clean_env() -> Vec<(OsString, OsString)> {
    let mut env = vec![(OsString::from("PATH"), OsString::from(DAEMON_PATH))];
    if let Ok(Some(user)) = nix::unistd::User::from_uid(nix::unistd::getuid()) {
        env.push(("HOME".into(), user.dir.into_os_string()));
        env.push(("USER".into(), user.name.clone().into()));
        env.push(("LOGNAME".into(), user.name.into()));
    }
    env.extend(debug_overrides());
    env
}

/// Environment of an on-demand daemon on Windows, never taken from the
/// client's (SEC-10): the Windows folder read from the kernel, a `PATH` of
/// only its system folders, the user and local-data folders from the
/// known-folder API (as the profile resolves them) and the Program Files
/// folder of the Windows drive, where the daemon looks for Git. Debug builds
/// also pass the test overrides.
#[cfg(windows)]
pub fn clean_env() -> Vec<(OsString, OsString)> {
    let mut env = Vec::new();
    if let Some(windows) = gitraptor_winsys::system::windows_dir() {
        let path =
            std::env::join_paths([windows.join("System32"), windows.clone()]).unwrap_or_default();
        env.push((
            OsString::from("SystemRoot"),
            windows.clone().into_os_string(),
        ));
        env.push((OsString::from("PATH"), path));
        if let Some(drive) = windows.parent() {
            env.push((
                OsString::from("ProgramFiles"),
                drive.join("Program Files").into_os_string(),
            ));
        }
    }
    if let Some(base) = directories::BaseDirs::new() {
        env.push(("USERPROFILE".into(), base.home_dir().as_os_str().to_owned()));
        env.push((
            "LOCALAPPDATA".into(),
            base.data_local_dir().as_os_str().to_owned(),
        ));
    }
    env.extend(debug_overrides());
    env
}

/// The test overrides a daemon started for this client keeps: the
/// profile, the agent classifier, the sessions' clock, the resource
/// targets and the autostart folder and service tool. Empty in release builds (SEC-06).
#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
pub(crate) fn debug_overrides() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env = Vec::new();
    if cfg!(debug_assertions) {
        for name in [
            crate::profile::PROFILE_DIR_ENV,
            crate::daemon::AGENT_EXECUTABLES_ENV,
            crate::daemon::CLOCK_SKEW_FILE_ENV,
            crate::daemon::TEST_GIT_ENV,
            crate::daemon::DISCOVERY_HOME_ENV,
            crate::daemon::DISCOVERY_POLL_ENV,
            crate::resources::RESOURCE_TARGETS_ENV,
            crate::autostart::AUTOSTART_DIR_ENV,
            crate::autostart::SERVICE_TOOL_ENV,
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
    dirs.runtime.as_deref().map(api::transport::socket_path)
}
