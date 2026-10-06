//! Login autostart (US-GRP-004; ADR-GRP-005 § 3, PQ-1, SEC-14).
//!
//! The only write outside the profile: one artifact per OS, created by
//! `raptor daemon enable` and removed by `raptor daemon disable`, both run
//! by the developer in the CLI process (never through the channel, never by
//! the engine on its own):
//!
//! | OS | Artifact |
//! |----|----------|
//! | macOS | `~/Library/LaunchAgents/dev.gitraptor.plist` |
//! | Linux | `~/.config/systemd/user/gitraptor.service` and its link in `default.target.wants` |
//! | Windows | Value `GitRaptor` of `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
//!
//! `~` is the home of the user database, not `$HOME` nor `$XDG_CONFIG_HOME`
//! (SEC-10). The artifact runs `<raptor> daemon --autostart`. `disable`
//! never stops the running engine (no `bootout`, no `systemctl stop`): an
//! agent cannot use it to stop the capture (ADR-GRP-005 § 6). The engine
//! only checks whether the artifact exists, for `scope.snapshot`; clients
//! ask the service manager to start it when the artifact runs their own
//! binary (macOS and Linux), and never with `kickstart -k`.
//!
//! Debug builds only (SEC-06), fail-closed: [`AUTOSTART_DIR_ENV`] moves the
//! artifact to a test folder and requires [`SERVICE_TOOL_ENV`], which
//! replaces `launchctl` or `systemctl`; [`AUTOSTART_REGKEY_ENV`] moves the
//! Windows value to a test subkey. With a test profile and no test folder
//! there is no autostart at all. The artifact carries the test overrides of
//! the profile, so the engine it starts uses the test profile.

use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

/// launchd label and base name of the artifacts.
pub const LABEL: &str = "dev.gitraptor";
/// systemd unit of the autostart.
pub const UNIT: &str = "gitraptor.service";
/// Name of the value in HKCU `Run`.
pub const RUN_VALUE: &str = "GitRaptor";
/// Argument of `raptor daemon` started by the service manager (see
/// [`start_failure_exit`]).
pub const AUTOSTART_ARG: &str = "--autostart";

/// Debug-build test hook: the folder of the artifact (macOS and Linux).
/// Release builds do not even read it (SEC-06).
pub const AUTOSTART_DIR_ENV: &str = "GITRAPTOR_AUTOSTART_DIR";
/// Debug-build test hook: the HKCU subkey of the value (Windows).
pub const AUTOSTART_REGKEY_ENV: &str = "GITRAPTOR_AUTOSTART_REGKEY";
/// Debug-build test hook: the executable used instead of `launchctl` or
/// `systemctl`. Release builds do not even read it (SEC-06).
pub const SERVICE_TOOL_ENV: &str = "GITRAPTOR_TEST_SERVICE_TOOL";

/// Longest wait for one call of the service tool.
#[cfg(unix)]
const TOOL_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg_attr(not(windows), allow(dead_code))]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Path components of the npm channel and of the npx cache: a binary there
/// is not an installation (SEC-14, ADR-GRP-014, INF-GRP-004).
const TRANSIENT_COMPONENTS: [&str; 2] = ["node_modules", "_npx"];

/// Exit code of `raptor daemon` whose start failed. Started by the service
/// manager (`--autostart`) it is always 0, whatever failed (another
/// instance, insecure profile, I/O), so `KeepAlive.SuccessfulExit = false`
/// and `Restart=on-failure` do not relaunch it every few seconds
/// (ADR-GRP-015); the error is on its (null) stderr and, once the profile
/// opened, in the engine's log. A panic still exits non-zero and is
/// relaunched. Otherwise: 3 for another instance (TS-GRP-003), 1 for the
/// rest.
pub fn start_failure_exit(autostart: bool, already_running: bool) -> u8 {
    match (autostart, already_running) {
        (true, _) => 0,
        (false, true) => crate::daemon::EXIT_ALREADY_RUNNING as u8,
        (false, false) => 1,
    }
}

/// Why the autostart could not be changed.
#[derive(Debug)]
pub enum AutostartError {
    /// This OS has no supported mechanism, the user has no home folder, or
    /// (debug builds) the test hooks are incomplete.
    Unsupported,
    /// The binary is not an installation: the npm channel, a package
    /// runner's cache, a temporary folder, or writable by others (SEC-14).
    TransientBinary(PathBuf),
    /// Linux: the user's systemd does not load the unit (no user manager,
    /// or one that reads another folder). Nothing was left written.
    ManagerUnavailable,
    Io(io::Error),
}

impl std::fmt::Display for AutostartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "no supported login autostart on this system"),
            Self::TransientBinary(path) => {
                write!(f, "{} is not an installed raptor", path.display())
            }
            Self::ManagerUnavailable => write!(f, "the user's systemd does not load the unit"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for AutostartError {}

impl From<io::Error> for AutostartError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Result of `enable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enabled {
    /// Where the artifact is, to show the developer.
    pub artifact: String,
    /// `false` if the same artifact was already there.
    pub changed: bool,
    /// Whether the service manager took it for this session too. Always
    /// `false` on Windows (it acts at the next login) and on macOS without
    /// a graphical session (SSH).
    pub active_now: bool,
}

/// The login autostart of the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autostart {
    /// macOS and Linux: the folder of the artifact. Windows: the HKCU subkey.
    location: PathBuf,
    #[cfg_attr(not(unix), allow(dead_code))]
    tool: PathBuf,
    #[cfg(unix)]
    uid: u32,
}

impl Autostart {
    /// The autostart of the current user, or `None` if this OS has no
    /// supported mechanism (Linux without `systemctl`, other Unixes) or,
    /// in debug builds, the test hooks are incomplete.
    pub fn for_current_user() -> Option<Self> {
        let profile_overridden = debug_var(crate::profile::PROFILE_DIR_ENV).is_some();
        let location = if cfg!(windows) {
            debug_var(AUTOSTART_REGKEY_ENV)
        } else {
            debug_var(AUTOSTART_DIR_ENV)
        };
        let tool = debug_var(SERVICE_TOOL_ENV).map(PathBuf::from);
        match (&location, &tool) {
            // A test never uses the real autostart, nor the real manager.
            (None, _) if profile_overridden => return None,
            (Some(_), None) if !cfg!(windows) => return None,
            _ => {}
        }
        Self::platform(location.map(PathBuf::from), tool)
    }

    #[cfg(target_os = "macos")]
    fn platform(location: Option<PathBuf>, tool: Option<PathBuf>) -> Option<Self> {
        let location = match location {
            Some(dir) => dir,
            None => user_home()?.join("Library/LaunchAgents"),
        };
        Some(Self {
            location,
            tool: tool.unwrap_or_else(|| PathBuf::from("/bin/launchctl")),
            uid: nix::unistd::getuid().as_raw(),
        })
    }

    #[cfg(target_os = "linux")]
    fn platform(location: Option<PathBuf>, tool: Option<PathBuf>) -> Option<Self> {
        let tool = tool.or_else(|| {
            ["/usr/bin/systemctl", "/bin/systemctl"]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
        })?;
        let location = match location {
            Some(dir) => dir,
            None => user_home()?.join(".config/systemd/user"),
        };
        Some(Self {
            location,
            tool,
            uid: nix::unistd::getuid().as_raw(),
        })
    }

    #[cfg(windows)]
    fn platform(location: Option<PathBuf>, _tool: Option<PathBuf>) -> Option<Self> {
        Some(Self {
            location: location.unwrap_or_else(|| PathBuf::from(RUN_KEY)),
            tool: PathBuf::new(),
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn platform(_location: Option<PathBuf>, _tool: Option<PathBuf>) -> Option<Self> {
        None
    }

    /// The files `enable` creates, in creation order (macOS and Linux).
    /// Their folders are created if missing and never removed.
    pub fn artifacts(&self) -> Vec<PathBuf> {
        if cfg!(target_os = "macos") {
            vec![self.location.join(format!("{LABEL}.plist"))]
        } else if cfg!(target_os = "linux") {
            vec![
                self.location.join(UNIT),
                self.location.join("default.target.wants").join(UNIT),
            ]
        } else {
            Vec::new()
        }
    }

    /// Whether the artifact is in place. Never fails: unreadable counts as
    /// absent.
    pub fn is_registered(&self) -> bool {
        #[cfg(windows)]
        {
            gitraptor_winsys::registry::get_string(&self.location, RUN_VALUE)
                .ok()
                .flatten()
                .is_some()
        }
        #[cfg(not(windows))]
        {
            let artifacts = self.artifacts();
            !artifacts.is_empty() && artifacts.iter().all(|a| a.symlink_metadata().is_ok())
        }
    }

    /// The binary the artifact runs, if it is one `enable` wrote.
    pub fn registered_exe(&self) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            let value = gitraptor_winsys::registry::get_string(&self.location, RUN_VALUE)
                .ok()
                .flatten()?;
            parse_run_value(&value)
        }
        #[cfg(not(windows))]
        {
            let text = std::fs::read_to_string(self.artifacts().first()?).ok()?;
            if cfg!(target_os = "macos") {
                parse_plist_exe(&text)
            } else {
                parse_unit_exe(&text)
            }
        }
    }

    /// Registers the autostart for `exe`, the running `raptor` as it was
    /// launched (`current_exe()`): its stable path, not the canonical one,
    /// so a package manager's symlink survives upgrades. On macOS and Linux
    /// it also hands it to the service manager for this session.
    /// Idempotent.
    pub fn enable(&self, exe: &Path) -> Result<Enabled, AutostartError> {
        if !exe.is_absolute() {
            return Err(AutostartError::TransientBinary(exe.to_owned()));
        }
        let canonical = exe.canonicalize()?;
        let temp = temp_dirs();
        refuse_transient(exe, &temp)?;
        refuse_transient(&canonical, &temp)?;
        refuse_writable_by_others(&canonical)?;
        self.enable_platform(exe)
    }

    #[cfg(target_os = "macos")]
    fn enable_platform(&self, exe: &Path) -> Result<Enabled, AutostartError> {
        let plist = &self.artifacts()[0];
        let changed = write_if_changed(plist, &launch_agent_plist(exe, &artifact_env()))?;
        // Fails without a graphical session (SSH) or if already loaded
        // (enabled before in this session): then ask whether it is loaded.
        let active_now = self.run_tool(&["bootstrap", &self.domain(), path_str(plist)?], &[])
            || self.run_tool(&["print", &self.service()], &[]);
        Ok(Enabled {
            artifact: plist.display().to_string(),
            changed,
            active_now,
        })
    }

    #[cfg(target_os = "linux")]
    fn enable_platform(&self, exe: &Path) -> Result<Enabled, AutostartError> {
        let artifacts = self.artifacts();
        let (unit, link) = (&artifacts[0], &artifacts[1]);
        let mut changed = write_if_changed(unit, &systemd_unit(exe, &artifact_env()))?;
        if std::fs::read_link(link).ok().as_deref() != Some(unit.as_path()) {
            std::fs::create_dir_all(link.parent().unwrap_or(&self.location))?;
            if link.symlink_metadata().is_ok() {
                std::fs::remove_file(link)?;
            }
            std::os::unix::fs::symlink(unit, link)?;
            changed = true;
        }
        // The user manager must load this very unit; if not (no user
        // session, another `XDG_CONFIG_HOME`), nothing is left written.
        if !(self.systemctl(&["daemon-reload"]) && self.systemctl(&["cat", UNIT])) {
            self.disable()?;
            return Err(AutostartError::ManagerUnavailable);
        }
        let active_now = self.systemctl(&["start", UNIT]);
        Ok(Enabled {
            artifact: unit.display().to_string(),
            changed,
            active_now,
        })
    }

    #[cfg(windows)]
    fn enable_platform(&self, exe: &Path) -> Result<Enabled, AutostartError> {
        use gitraptor_winsys::registry;
        let value = run_value(exe);
        let changed =
            registry::get_string(&self.location, RUN_VALUE)?.as_deref() != Some(value.as_str());
        if changed {
            registry::set_string(&self.location, RUN_VALUE, &value)?;
        }
        Ok(Enabled {
            artifact: format!(r"HKCU\{}\{RUN_VALUE}", self.location.display()),
            changed,
            active_now: false,
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn enable_platform(&self, _exe: &Path) -> Result<Enabled, AutostartError> {
        Err(AutostartError::Unsupported)
    }

    /// Removes exactly what `enable` created. Never stops the running
    /// engine; on macOS the loaded job stays until logout. Returns whether
    /// there was anything to remove.
    pub fn disable(&self) -> Result<bool, AutostartError> {
        #[cfg(windows)]
        {
            Ok(gitraptor_winsys::registry::delete_value(
                &self.location,
                RUN_VALUE,
            )?)
        }
        #[cfg(not(windows))]
        {
            let mut removed = false;
            // The link before the unit it points to.
            for artifact in self.artifacts().iter().rev() {
                match std::fs::remove_file(artifact) {
                    Ok(()) => removed = true,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
            }
            #[cfg(target_os = "linux")]
            if removed {
                self.systemctl(&["daemon-reload"]);
            }
            Ok(removed)
        }
    }

    /// Asks the service manager to start the engine (ADR-GRP-005 § 3), so
    /// it gets the manager's environment, never the client's. Only when the
    /// artifact runs `installed` (the client's own `raptor`): a foreign or
    /// stale artifact is not trusted. `false` on Windows, when not
    /// registered or when the manager refused: the client then starts the
    /// engine itself with a clean environment.
    pub fn start(&self, installed: &Path) -> bool {
        let Some(registered) = self.registered_exe() else {
            return false;
        };
        if !same_file(&registered, installed) {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            // Never `-k`: it would kill a running engine.
            self.run_tool(&["kickstart", &self.service()], &[])
                || self.run_tool(
                    &[
                        "bootstrap",
                        &self.domain(),
                        &self.artifacts()[0].to_string_lossy(),
                    ],
                    &[],
                )
        }
        #[cfg(target_os = "linux")]
        {
            self.systemctl(&["start", UNIT])
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            false
        }
    }

    #[cfg(target_os = "macos")]
    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }

    #[cfg(target_os = "macos")]
    fn service(&self) -> String {
        format!("gui/{}/{LABEL}", self.uid)
    }

    #[cfg(target_os = "linux")]
    fn systemctl(&self, args: &[&str]) -> bool {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // The user manager is reached through the runtime folder derived
        // from the uid, checked to be the user's own and private; the
        // caller's environment is never passed.
        let runtime = format!("/run/user/{}", self.uid);
        let private = std::fs::metadata(&runtime)
            .is_ok_and(|m| m.uid() == self.uid && m.permissions().mode() & 0o077 == 0);
        if !private {
            return false;
        }
        let mut full = vec!["--user"];
        full.extend_from_slice(args);
        self.run_tool(&full, &[("XDG_RUNTIME_DIR", runtime.as_str())])
    }

    /// Runs the service tool by absolute path, with a fixed argv and
    /// environment (SEC-10) and a time limit.
    #[cfg(unix)]
    fn run_tool(&self, args: &[&str], env: &[(&str, &str)]) -> bool {
        let child = std::process::Command::new(&self.tool)
            .args(args)
            .env_clear()
            .env("PATH", crate::client::DAEMON_PATH)
            .envs(env.iter().copied())
            .current_dir("/")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            return false;
        };
        let deadline = std::time::Instant::now() + TOOL_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }
}

/// `Err` if `exe` is not an installation (SEC-14): the npm channel, the
/// npx, `pnpm dlx` or `bunx` cache, or under a temporary folder (the same
/// caches `raptor mcp install` refuses).
pub fn refuse_transient(exe: &Path, temp: &[PathBuf]) -> Result<(), AutostartError> {
    let names: Vec<String> = exe
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let cache = names.iter().enumerate().any(|(i, name)| {
        TRANSIENT_COMPONENTS.contains(&name.as_str())
            || name.starts_with("bunx-")
            || (name == "dlx" && i > 0 && names[i - 1].contains("pnpm"))
    });
    let in_temp = temp
        .iter()
        .filter(|t| t.parent().is_some())
        .any(|t| exe.starts_with(t));
    if cache || in_temp {
        return Err(AutostartError::TransientBinary(exe.to_owned()));
    }
    Ok(())
}

/// `Err` if another user could replace the binary the artifact runs, the
/// same rule SEC-10 applies to Git.
#[cfg(unix)]
fn refuse_writable_by_others(exe: &Path) -> Result<(), AutostartError> {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::metadata(exe)?.permissions().mode() & 0o022 != 0 {
        return Err(AutostartError::TransientBinary(exe.to_owned()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn refuse_writable_by_others(_exe: &Path) -> Result<(), AutostartError> {
    Ok(())
}

/// The temporary folders of this machine, as given and canonical.
fn temp_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir()];
    if cfg!(unix) {
        dirs.extend(
            ["/tmp", "/var/tmp", "/var/folders", "/dev/shm", "/run/user"]
                .into_iter()
                .map(PathBuf::from),
        );
    }
    if cfg!(windows)
        && let Some(root) = std::env::var_os("SystemRoot")
    {
        dirs.push(PathBuf::from(root).join("Temp"));
    }
    let mut out = Vec::new();
    for dir in dirs {
        if let Ok(canonical) = dir.canonicalize() {
            out.push(canonical);
        }
        out.push(dir);
    }
    out
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The test overrides an engine started by the artifact needs (debug
/// builds only; empty in release).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn artifact_env() -> Vec<(OsString, OsString)> {
    crate::client::debug_overrides()
}

/// The launchd agent: started at load (login) and relaunched only if it
/// exits with an error; a normal process, not a background one
/// (ADR-GRP-015). No output paths: the engine logs in its profile (SEC-05).
pub fn launch_agent_plist(exe: &Path, env: &[(OsString, OsString)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    out.push_str(&format!("  <key>Label</key>\n  <string>{LABEL}</string>\n"));
    out.push_str("  <key>ProgramArguments</key>\n  <array>\n");
    for arg in [exe.to_string_lossy().as_ref(), "daemon", AUTOSTART_ARG] {
        out.push_str(&format!("    <string>{}</string>\n", xml_escape(arg)));
    }
    out.push_str("  </array>\n");
    if !env.is_empty() {
        out.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
        for (key, value) in env {
            out.push_str(&format!(
                "    <key>{}</key>\n    <string>{}</string>\n",
                xml_escape(&key.to_string_lossy()),
                xml_escape(&value.to_string_lossy())
            ));
        }
        out.push_str("  </dict>\n");
    }
    out.push_str(
        "  <key>RunAtLoad</key>\n  <true/>\n\
         \x20 <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
         \x20 <key>ProcessType</key>\n  <string>Standard</string>\n\
         </dict>\n</plist>\n",
    );
    out
}

/// The systemd user unit: started with the session and restarted only on
/// failure; no `Nice=` (ADR-GRP-015); no output to the journal (SEC-05).
pub fn systemd_unit(exe: &Path, env: &[(OsString, OsString)]) -> String {
    let mut out = String::from("[Unit]\nDescription=GitRaptor engine\n\n[Service]\nType=simple\n");
    out.push_str(&format!(
        "ExecStart={} daemon {AUTOSTART_ARG}\n",
        systemd_quote(&exe.to_string_lossy())
    ));
    for (key, value) in env {
        let pair = format!("{}={}", key.to_string_lossy(), value.to_string_lossy());
        out.push_str(&format!("Environment={}\n", systemd_quote(&pair)));
    }
    out.push_str(
        "StandardOutput=null\nStandardError=null\nRestart=on-failure\n\n\
         [Install]\nWantedBy=default.target\n",
    );
    out
}

/// The command of the HKCU `Run` value: the absolute path between quotes,
/// so a path with spaces cannot be read as another program (SEC-14).
pub fn run_value(exe: &Path) -> String {
    format!("\"{}\" daemon {AUTOSTART_ARG}", exe.display())
}

/// The binary of a plist written by [`launch_agent_plist`].
pub fn parse_plist_exe(text: &str) -> Option<PathBuf> {
    let after = text.split_once("<key>ProgramArguments</key>")?.1;
    let first = after.split_once("<string>")?.1.split_once("</string>")?.0;
    Some(PathBuf::from(xml_unescape(first)))
}

/// The binary of a unit written by [`systemd_unit`].
pub fn parse_unit_exe(text: &str) -> Option<PathBuf> {
    let line = text.lines().find_map(|l| l.strip_prefix("ExecStart=\""))?;
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(PathBuf::from(out)),
            '\\' => out.push(chars.next()?),
            '%' => {
                chars.next()?;
                out.push('%');
            }
            c => out.push(c),
        }
    }
    None
}

/// The binary of a value written by [`run_value`].
pub fn parse_run_value(text: &str) -> Option<PathBuf> {
    let rest = text.strip_prefix('"')?;
    Some(PathBuf::from(rest.split_once('"')?.0))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// One systemd word between double quotes, with `\`, `"` and the `%`
/// specifiers escaped.
fn systemd_quote(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// Writes `content` to `path` (mode 0644, through a new temporary file in
/// the same folder renamed over it, so a link at `path` is replaced, never
/// followed) unless it already holds exactly that. Creates the folder.
#[cfg(unix)]
fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let current = std::fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .and_then(|_| std::fs::read(path).ok());
    if current.as_deref() == Some(content.as_bytes()) {
        return Ok(false);
    }
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "artifact without folder"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{LABEL}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| true)
}

#[cfg(target_os = "macos")]
fn path_str(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))
}

/// Home folder from the user database, never from `$HOME` (SEC-10).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn user_home() -> Option<PathBuf> {
    nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.dir)
        .filter(|d| d.is_absolute())
}

fn debug_var(name: &str) -> Option<OsString> {
    if cfg!(debug_assertions) {
        std::env::var_os(name).filter(|v| !v.is_empty())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_runs_the_absolute_binary_with_autostart() {
        let plist = launch_agent_plist(Path::new("/Applications/Git Raptor/raptor"), &[]);
        assert!(plist.contains("<string>dev.gitraptor</string>"));
        assert!(plist.contains(
            "<string>/Applications/Git Raptor/raptor</string>\n    <string>daemon</string>\n    \
             <string>--autostart</string>"
        ));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(plist.contains("<string>Standard</string>"));
        assert!(!plist.contains("EnvironmentVariables"));
        assert!(!plist.contains("StandardOutPath"));
        assert_eq!(
            parse_plist_exe(&plist).unwrap(),
            Path::new("/Applications/Git Raptor/raptor")
        );
    }

    #[test]
    fn the_plist_escapes_xml_and_carries_overrides() {
        let env = [("GITRAPTOR_PROFILE_DIR".into(), "/t/a&b".into())];
        let plist = launch_agent_plist(Path::new("/opt/<r>&/raptor"), &env);
        assert!(plist.contains("<string>/opt/&lt;r&gt;&amp;/raptor</string>"));
        assert!(
            plist.contains("<key>GITRAPTOR_PROFILE_DIR</key>\n    <string>/t/a&amp;b</string>")
        );
        assert_eq!(
            parse_plist_exe(&plist).unwrap(),
            Path::new("/opt/<r>&/raptor")
        );
    }

    #[test]
    fn the_unit_quotes_the_binary_and_restarts_on_failure() {
        let env = [("GITRAPTOR_PROFILE_DIR".into(), "/t/p%1".into())];
        let unit = systemd_unit(Path::new("/home/u/my \"apps\"%/raptor"), &env);
        assert!(
            unit.contains("ExecStart=\"/home/u/my \\\"apps\\\"%%/raptor\" daemon --autostart\n")
        );
        assert!(unit.contains("Environment=\"GITRAPTOR_PROFILE_DIR=/t/p%%1\"\n"));
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("StandardOutput=null\nStandardError=null\n"));
        assert!(unit.contains("WantedBy=default.target\n"));
        assert!(!unit.contains("Nice="));
        assert_eq!(
            parse_unit_exe(&unit).unwrap(),
            Path::new("/home/u/my \"apps\"%/raptor")
        );
    }

    #[test]
    fn the_run_value_quotes_a_path_with_spaces() {
        let exe = Path::new(r"C:\Program Files\GitRaptor\raptor.exe");
        let value = run_value(exe);
        assert_eq!(
            value,
            r#""C:\Program Files\GitRaptor\raptor.exe" daemon --autostart"#
        );
        assert_eq!(parse_run_value(&value).unwrap(), exe);
    }

    #[test]
    fn an_npm_npx_or_temporary_binary_is_refused() {
        let temp = [PathBuf::from("/private/var/folders/x")];
        for exe in [
            "/Users/u/.npm/_npx/1a2b/node_modules/gitraptor/bin/raptor",
            "/usr/local/lib/node_modules/gitraptor/bin/raptor",
            "/Users/u/Library/pnpm/dlx/raptor",
            "/var/u/bunx-501-gitraptor/raptor",
            "/private/var/folders/x/T/raptor",
        ] {
            assert!(
                matches!(
                    refuse_transient(Path::new(exe), &temp),
                    Err(AutostartError::TransientBinary(_))
                ),
                "{exe}"
            );
        }
        assert!(refuse_transient(Path::new("/opt/homebrew/bin/raptor"), &temp).is_ok());
        // Only whole components.
        assert!(refuse_transient(Path::new("/opt/my_npx/raptor"), &temp).is_ok());
        assert!(refuse_transient(Path::new("/opt/dlx/raptor"), &temp).is_ok());
        // A temporary root of "/" would refuse everything: ignored.
        assert!(refuse_transient(Path::new("/opt/raptor"), &[PathBuf::from("/")]).is_ok());
    }

    #[test]
    fn a_start_that_fails_under_the_service_manager_is_not_relaunched() {
        for already_running in [true, false] {
            assert_eq!(start_failure_exit(true, already_running), 0);
        }
        assert_eq!(start_failure_exit(false, true), 3);
        assert_eq!(start_failure_exit(false, false), 1);
    }

    #[cfg(unix)]
    fn fake_tool(dir: &Path, exit: u8) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("tool.log");
        let tool = dir.join("tool.sh");
        std::fs::write(
            &tool,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\nexit {exit}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        tool
    }

    #[cfg(unix)]
    fn binary(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.join("bin/raptor");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        exe.canonicalize().unwrap()
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn enable_and_disable_create_and_remove_exactly_the_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let tool = fake_tool(dir.path(), if cfg!(target_os = "macos") { 1 } else { 0 });
        let autostart = Autostart::platform(Some(dir.path().join("agents")), Some(tool)).unwrap();
        assert!(!autostart.is_registered());
        let exe = binary(dir.path());
        if cfg!(target_os = "linux") && !Path::new(&format!("/run/user/{}", autostart.uid)).is_dir()
        {
            // Without a user runtime folder the manager is unavailable and
            // nothing stays written.
            assert!(matches!(
                autostart.enable_platform(&exe),
                Err(AutostartError::ManagerUnavailable)
            ));
            assert!(!autostart.is_registered());
            return;
        }
        let enabled = autostart.enable_platform(&exe).unwrap();
        assert!(enabled.changed);
        assert!(autostart.is_registered());
        assert_eq!(autostart.registered_exe().unwrap(), exe);
        assert!(!autostart.enable_platform(&exe).unwrap().changed);
        let mut found = walk(&dir.path().join("agents"));
        let mut expected = autostart.artifacts();
        expected.sort();
        found.sort();
        assert_eq!(found, expected);
        assert!(autostart.disable().unwrap());
        assert!(!autostart.is_registered());
        assert!(walk(&dir.path().join("agents")).is_empty());
        assert!(!autostart.disable().unwrap());
    }

    /// The service manager is asked only for an artifact that runs the
    /// client's own binary; otherwise the client starts the engine itself.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_manager_starts_only_an_artifact_of_the_same_binary() {
        let dir = tempfile::tempdir().unwrap();
        let tool = fake_tool(dir.path(), 0);
        let autostart = Autostart::platform(Some(dir.path().join("agents")), Some(tool)).unwrap();
        let exe = binary(dir.path());
        let other = dir.path().join("other-raptor");
        std::fs::write(&other, "").unwrap();
        assert!(!autostart.start(&exe), "not registered");
        autostart.enable_platform(&exe).unwrap();
        assert!(!autostart.start(&other), "another binary");
        assert!(autostart.start(&exe));
        let log = std::fs::read_to_string(dir.path().join("tool.log")).unwrap();
        let uid = nix::unistd::getuid().as_raw();
        assert_eq!(
            log.lines().collect::<Vec<_>>(),
            [
                format!("bootstrap gui/{uid} {}", autostart.artifacts()[0].display()),
                format!("kickstart gui/{uid}/dev.gitraptor"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_writable_by_others_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = binary(dir.path());
        assert!(refuse_writable_by_others(&exe).is_ok());
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(matches!(
            refuse_writable_by_others(&exe),
            Err(AutostartError::TransientBinary(_))
        ));
    }

    #[cfg(unix)]
    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.symlink_metadata().unwrap().is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }
}
