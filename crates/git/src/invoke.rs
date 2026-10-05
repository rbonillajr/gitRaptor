//! The only module of `crates/git` that spawns processes (ADR-GRP-009 § 3, Validación 5): the
//! read layer and the Time Machine write layer (ADR-TMC-002 § 2) each with their own profile.
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

/// Options placed before every subcommand of the Time Machine write layer (SEC-TMC-02). The
/// hooks folder, `--git-dir` and `--work-tree` are added per invocation by [`write_argv`].
pub(crate) const WRITE_OPTIONS: &[&str] = &[
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=false",
    "-c",
    "core.splitIndex=false",
    "-c",
    "index.sparse=false",
    "-c",
    "index.skipHash=false",
    "-c",
    "gc.auto=0",
    "-c",
    "maintenance.auto=false",
    "-c",
    "gpg.program=",
    "-c",
    "commit.gpgSign=false",
    "-c",
    "protocol.allow=never",
    "-c",
    "credential.helper=",
    "-c",
    "core.sshCommand=",
    "-c",
    "core.askPass=",
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

/// Variables with fixed values in every write-layer child, on top of [`FIXED_ENV`].
/// `GIT_CONFIG_GLOBAL` (an empty file of the profile) is added per invocation.
const WRITE_ENV: &[(&str, &str)] = &[
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
    ("GIT_ALLOW_PROTOCOL", ""),
    ("GIT_PROTOCOL_FROM_USER", "0"),
];

/// The closed list of the Time Machine write layer (ADR-TMC-002 § 2, SEC-TMC-02), each with its
/// options fixed here: data only travels through standard input, so no revision, ref or path is
/// ever parsed as an option (SEC-TMC-14). No porcelain and no remote operation: adding one
/// requires revising ADR-TMC-002 (checked by `tests/static_check.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteSubcommand {
    /// One ref transaction with expected old values.
    UpdateRef,
    /// Entries of a temporary index (`GIT_INDEX_FILE`), conflict stages included.
    IndexInfo,
    /// The skip-worktree mark on paths of a temporary index.
    SkipWorktree,
    /// A pack of the objects the revisions on stdin reach, on the store.
    PackObjects,
    /// A pack from stdin into the user's repository, checked and kept until refs reach it.
    IndexPack,
}

impl WriteSubcommand {
    pub(crate) fn words(self) -> &'static [&'static str] {
        match self {
            Self::UpdateRef => &["update-ref", "--stdin", "-z"],
            Self::IndexInfo => &["update-index", "-z", "--index-info"],
            Self::SkipWorktree => &["update-index", "--skip-worktree", "-z", "--stdin"],
            Self::PackObjects => &["pack-objects", "--revs", "--stdout", "--quiet"],
            Self::IndexPack => &["index-pack", "--stdin", "--strict", "--keep=gitraptor-tm"],
        }
    }
}

/// Where a write-layer invocation acts. Every path is absolute and comes from the validated
/// state of the daemon, never from repository configuration (ADR-TMC-002 § 2).
#[derive(Debug, Clone, Copy)]
pub(crate) struct WriteTarget<'a> {
    pub git_dir: &'a Path,
    pub work_tree: Option<&'a Path>,
    /// An empty folder of the profile for `core.hooksPath`.
    pub hooks_dir: &'a Path,
    /// An empty file of the profile for `GIT_CONFIG_GLOBAL`.
    pub global_config: &'a Path,
    /// A temporary index (`GIT_INDEX_FILE`), never the worktree's own.
    pub index_file: Option<&'a Path>,
}

/// What a child reads on its standard input.
pub(crate) enum Input<'a> {
    None,
    Bytes(&'a [u8]),
    File(std::fs::File),
}

/// The argv of a write-layer invocation, without the executable.
pub(crate) fn write_argv(target: &WriteTarget<'_>, subcommand: WriteSubcommand) -> Vec<OsString> {
    let mut argv: Vec<OsString> = WRITE_OPTIONS.iter().map(OsString::from).collect();
    let mut hooks = OsString::from("core.hooksPath=");
    hooks.push(target.hooks_dir);
    argv.extend(["-c".into(), hooks]);
    // The read layer already decided the repository is trusted with the user's own
    // `safe.directory`; the global configuration is neutralized here, so the decision is passed
    // for that exact path only.
    let mut safe = OsString::from("safe.directory=");
    safe.push(target.work_tree.unwrap_or(target.git_dir));
    argv.extend(["-c".into(), safe]);
    let mut git_dir = OsString::from("--git-dir=");
    git_dir.push(target.git_dir);
    argv.push(git_dir);
    if let Some(work_tree) = target.work_tree {
        let mut w = OsString::from("--work-tree=");
        w.push(work_tree);
        argv.push(w);
    }
    argv.extend(subcommand.words().iter().map(OsString::from));
    argv
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
        command.args(&argv).env_clear().envs(self.child_env());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        self.spawn_and_wait(command, Input::None, None, &subcommand.words().join(" "))
    }

    /// Run `git <WRITE_OPTIONS> <subcommand> <args>` for the Time Machine write layer
    /// (ADR-TMC-002 § 2, SEC-TMC-02): explicit `--git-dir` (and `--work-tree` when given), no
    /// system or global configuration, an empty hooks folder, no signing and no protocol.
    pub(crate) fn run_write(
        &self,
        git: &Path,
        target: &WriteTarget<'_>,
        subcommand: WriteSubcommand,
        input: Input<'_>,
        stdout_to: Option<std::fs::File>,
    ) -> Result<Output, ReadError> {
        if !git.is_absolute() {
            return Err(ReadError::InvalidInput(
                "git executable must be an absolute path".into(),
            ));
        }
        for path in [
            Some(target.git_dir),
            target.work_tree,
            Some(target.hooks_dir),
        ]
        .into_iter()
        .flatten()
        .chain([target.global_config])
        .chain(target.index_file)
        {
            if !path.is_absolute() {
                return Err(ReadError::InvalidInput(
                    "write layer paths must be absolute".into(),
                ));
            }
        }
        let argv = write_argv(target, subcommand);

        if let Some(sink) = &self.argv_sink {
            let mut logged = vec![git.to_string_lossy().into_owned()];
            logged.extend(argv.iter().map(|a| a.to_string_lossy().into_owned()));
            sink.record(&logged);
        }

        let mut command = Command::new(git);
        command
            .args(&argv)
            .env_clear()
            .envs(self.write_env(target))
            .current_dir(target.git_dir);
        self.spawn_and_wait(command, input, stdout_to, &subcommand.words().join(" "))
    }

    /// The environment of a write-layer child: the read allowlist without `HOME`, plus the
    /// variables of SEC-TMC-02.
    pub(crate) fn write_env(&self, target: &WriteTarget<'_>) -> Vec<(OsString, OsString)> {
        let mut env: Vec<(OsString, OsString)> = self
            .child_env()
            .into_iter()
            .filter(|(k, _)| !key_matches(k, "HOME"))
            .collect();
        env.extend(WRITE_ENV.iter().map(|(k, v)| (k.into(), v.into())));
        env.push(("GIT_CONFIG_GLOBAL".into(), target.global_config.into()));
        if let Some(index) = target.index_file {
            env.push(("GIT_INDEX_FILE".into(), index.into()));
        }
        env
    }

    fn spawn_and_wait(
        &self,
        mut command: Command,
        input: Input<'_>,
        stdout_to: Option<std::fs::File>,
        what: &str,
    ) -> Result<Output, ReadError> {
        let to_file = stdout_to.is_some();
        command
            .stdout(match stdout_to {
                Some(file) => Stdio::from(file),
                None => Stdio::piped(),
            })
            .stderr(Stdio::piped());
        let mut bytes_in = None;
        match input {
            Input::None => {
                command.stdin(Stdio::null());
            }
            Input::Bytes(b) => {
                command.stdin(Stdio::piped());
                bytes_in = Some(b.to_vec());
            }
            Input::File(f) => {
                command.stdin(Stdio::from(f));
            }
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
        let writer = match (bytes_in, child.stdin.take()) {
            (Some(bytes), Some(mut stdin)) => Some(std::thread::spawn(move || {
                use std::io::Write;
                let _ = stdin.write_all(&bytes);
            })),
            _ => None,
        };
        let stdout = if to_file {
            None
        } else {
            child.stdout.take().map(spawn_reader)
        };
        let stderr = child.stderr.take().map(spawn_reader);

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ReadError::TemporarilyUnavailable(format!(
                        "git {what} exceeded {:?}",
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
        if let Some(w) = writer {
            let _ = w.join();
        }
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
