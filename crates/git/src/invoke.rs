//! The only module of the read layer that spawns processes (ADR-GRP-009 § 3, Validación 5).
//!
//! Every child is the resolved `git` by absolute path, without a shell, with a fixed set of
//! options that neutralize locks, fsmonitor, pagers, credentials and trace2, and with an
//! environment built from scratch out of an allowlist (SEC-10).

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ReadError;

/// Options placed before every subcommand.
pub(crate) const FIXED_OPTIONS: &[&str] = &[
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=keep",
    "-c",
    "core.splitIndex=false",
    "-c",
    "gc.auto=0",
    "-c",
    "maintenance.auto=false",
    "-c",
    "log.showSignature=false",
    "-c",
    "credential.helper=",
    "-c",
    "color.ui=false",
    "-c",
    "core.pager=cat",
    "-c",
    "trace2.normalTarget=",
    "-c",
    "trace2.eventTarget=",
    "-c",
    "trace2.perfTarget=",
];

/// Variables with fixed values in every child.
const FIXED_ENV: &[(&str, &str)] = &[
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_PAGER", "cat"),
    ("LC_ALL", "C"),
];

/// Variables copied from the parent environment when present. `PATH` is handled apart.
#[cfg(not(windows))]
const INHERITED_ENV: &[&str] = &["HOME"];
#[cfg(windows)]
const INHERITED_ENV: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
];

/// Upper bound for the captured output of one invocation.
const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

/// The subcommands the read layer may run. Adding one requires revising ADR-GRP-009.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subcommand {
    Version,
    RevParse,
    ForEachRef,
    WorktreeList,
    RevList,
    MergeBase,
    Log,
    Config,
}

impl Subcommand {
    fn words(self) -> &'static [&'static str] {
        match self {
            Self::Version => &["version"],
            Self::RevParse => &["rev-parse"],
            Self::ForEachRef => &["for-each-ref"],
            Self::WorktreeList => &["worktree", "list"],
            Self::RevList => &["rev-list"],
            Self::MergeBase => &["merge-base"],
            Self::Log => &["log"],
            Self::Config => &["config"],
        }
    }
}

/// Receives every argv executed, for the diagnostic mode (ADR-GRP-009 § 3, Auditoría).
pub trait ArgvSink: Send + Sync {
    /// Record one argv, the executable first.
    fn record(&self, argv: &[String]);
}

/// An in-memory [`ArgvSink`].
#[derive(Debug, Default, Clone)]
pub struct MemoryArgvLog(Arc<Mutex<Vec<Vec<String>>>>);

impl MemoryArgvLog {
    /// All recorded argvs, oldest first.
    pub fn entries(&self) -> Vec<Vec<String>> {
        self.0.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl ArgvSink for MemoryArgvLog {
    fn record(&self, argv: &[String]) {
        if let Ok(mut entries) = self.0.lock() {
            entries.push(argv.to_vec());
        }
    }
}

/// How children are launched: timeout, parent environment and argv log.
#[derive(Clone)]
pub struct Invoker {
    timeout: Duration,
    parent_env: Vec<(OsString, OsString)>,
    argv_sink: Option<Arc<dyn ArgvSink>>,
}

impl Default for Invoker {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            parent_env: std::env::vars_os().collect(),
            argv_sink: None,
        }
    }
}

impl std::fmt::Debug for Invoker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The parent environment may hold secrets: never print it (SEC-05).
        f.debug_struct("Invoker")
            .field("timeout", &self.timeout)
            .field("argv_sink", &self.argv_sink.is_some())
            .finish_non_exhaustive()
    }
}

impl Invoker {
    /// Maximum duration of one invocation; past it the child is killed.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The environment the child environment is built from. Defaults to the process environment.
    pub fn with_parent_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>,
    ) -> Self {
        self.parent_env = env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        self
    }

    /// Record every argv into `sink` (diagnostic mode).
    pub fn with_argv_sink(mut self, sink: Arc<dyn ArgvSink>) -> Self {
        self.argv_sink = Some(sink);
        self
    }

    /// The environment a child receives, built from the allowlist.
    pub fn child_env(&self) -> Vec<(OsString, OsString)> {
        build_child_env(&self.parent_env)
    }

    /// Run `git <FIXED_OPTIONS> <subcommand> <args>` in `cwd` and return its stdout.
    pub(crate) fn run(
        &self,
        git: &Path,
        cwd: Option<&Path>,
        subcommand: Subcommand,
        args: &[&OsStr],
    ) -> Result<Output, ReadError> {
        if !git.is_absolute() {
            return Err(ReadError::InvalidInput(
                "git executable must be an absolute path".into(),
            ));
        }
        let mut argv: Vec<OsString> = FIXED_OPTIONS.iter().map(OsString::from).collect();
        argv.extend(subcommand.words().iter().map(OsString::from));
        argv.extend(args.iter().map(|a| a.to_os_string()));

        if let Some(sink) = &self.argv_sink {
            let mut logged = vec![git.to_string_lossy().into_owned()];
            logged.extend(argv.iter().map(|a| a.to_string_lossy().into_owned()));
            sink.record(&logged);
        }

        let mut command = Command::new(git);
        command
            .args(&argv)
            .env_clear()
            .envs(self.child_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|e| ReadError::Unavailable(format!("could not start git: {e}")))?;
        let stdout = child.stdout.take().map(spawn_reader);
        let stderr = child.stderr.take().map(spawn_reader);

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ReadError::TemporarilyUnavailable(format!(
                        "git {} exceeded {:?}",
                        subcommand.words().join(" "),
                        self.timeout
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => {
                    let _ = child.kill();
                    return Err(ReadError::Unavailable(format!("waiting for git: {e}")));
                }
            }
        };
        let collect = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
            h.and_then(|h| h.join().ok()).unwrap_or_default()
        };
        Ok(Output {
            success: status.success(),
            code: status.code(),
            stdout: collect(stdout),
            stderr: collect(stderr),
        })
    }
}

fn spawn_reader(pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.take(MAX_OUTPUT_BYTES).read_to_end(&mut buf);
        buf
    })
}

/// Build the child environment from `parent`: only allowlisted variables survive, `PATH` loses
/// its relative entries and the fixed variables are added.
fn build_child_env(parent: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let lookup = |name: &str| {
        parent
            .iter()
            .find(|(k, _)| key_matches(k, name))
            .map(|(_, v)| v.clone())
    };
    let mut env: Vec<(OsString, OsString)> = INHERITED_ENV
        .iter()
        .filter_map(|name| lookup(name).map(|v| (OsString::from(name), v)))
        .collect();
    if let Some(path) = lookup("PATH") {
        let absolute: Vec<PathBuf> = std::env::split_paths(&path)
            .filter(|p| p.is_absolute())
            .collect();
        if let Ok(joined) = std::env::join_paths(absolute) {
            env.push(("PATH".into(), joined));
        }
    }
    env.extend(FIXED_ENV.iter().map(|(k, v)| (k.into(), v.into())));
    env
}

/// Environment names are case-insensitive on Windows.
fn key_matches(key: &OsStr, name: &str) -> bool {
    if cfg!(windows) {
        key.to_str().is_some_and(|k| k.eq_ignore_ascii_case(name))
    } else {
        key == name
    }
}

/// Result of one invocation.
#[derive(Debug)]
pub(crate) struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    /// Turn a failed run into an error, mapping `safe.directory` rejections to `Untrusted`.
    pub fn into_success(self, what: &str) -> Result<Vec<u8>, ReadError> {
        if self.success {
            return Ok(self.stdout);
        }
        let stderr = String::from_utf8_lossy(&self.stderr);
        if is_dubious_ownership(&stderr) {
            return Err(ReadError::Untrusted(format!("{what}: dubious ownership")));
        }
        Err(ReadError::Unavailable(format!(
            "{what} failed with code {:?}",
            self.code
        )))
    }
}

/// Git's message when `safe.directory` rejects a repository.
pub(crate) fn is_dubious_ownership(stderr: &str) -> bool {
    stderr.contains("detected dubious ownership")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(env: &[(OsString, OsString)]) -> Vec<String> {
        env.iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn child_env_is_an_allowlist() {
        let parent: Vec<(OsString, OsString)> = [
            ("HOME", "/home/dev"),
            ("PATH", ".:/usr/bin:bin:/opt/homebrew/bin"),
            ("GIT_EXEC_PATH", "/tmp/evil"),
            ("GIT_SSH_COMMAND", "evil"),
            ("GIT_DIR", "/tmp/other"),
            ("GIT_CONFIG_PARAMETERS", "'core.fsmonitor'='evil'"),
            ("LD_PRELOAD", "/tmp/evil.so"),
            ("DYLD_INSERT_LIBRARIES", "/tmp/evil.dylib"),
            ("XDG_CONFIG_HOME", "/tmp/evil"),
            ("GIT_TRACE2_EVENT", "/tmp/trace"),
            ("GIT_PAGER", "evil"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let env = build_child_env(&parent);
        let names = names(&env);
        for forbidden in [
            "GIT_EXEC_PATH",
            "GIT_SSH_COMMAND",
            "GIT_DIR",
            "GIT_CONFIG_PARAMETERS",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "XDG_CONFIG_HOME",
            "GIT_TRACE2_EVENT",
        ] {
            assert!(!names.iter().any(|n| n == forbidden), "{forbidden} leaked");
        }
        let get = |k: &str| {
            env.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        #[cfg(not(windows))]
        {
            assert_eq!(get("HOME").as_deref(), Some("/home/dev"));
            assert_eq!(get("PATH").as_deref(), Some("/usr/bin:/opt/homebrew/bin"));
        }
        assert_eq!(get("GIT_PAGER").as_deref(), Some("cat"));
        assert_eq!(get("GIT_OPTIONAL_LOCKS").as_deref(), Some("0"));
        assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
        assert_eq!(get("LC_ALL").as_deref(), Some("C"));
    }

    #[test]
    fn dubious_ownership_maps_to_untrusted() {
        let out = Output {
            success: false,
            code: Some(128),
            stdout: vec![],
            stderr: b"fatal: detected dubious ownership in repository at '/r'\n".to_vec(),
        };
        assert!(matches!(
            out.into_success("rev-parse"),
            Err(ReadError::Untrusted(_))
        ));
    }

    #[test]
    fn relative_git_is_rejected_before_spawning() {
        let err = Invoker::default()
            .run(Path::new("git"), None, Subcommand::Version, &[])
            .unwrap_err();
        assert!(matches!(err, ReadError::InvalidInput(_)));
    }
}
