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
    /// `--exec-path`: where this Git keeps its programs (resolution of the Git for Windows
    /// launcher, ADR-GRP-009 § 4).
    ExecPath,
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
            Self::ExecPath => &["--exec-path"],
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
    /// Consolidation of the store (ADR-TMC-007 § 4, Enmienda E10).
    Repack,
    /// Loose objects of the store without refs, past the grace period (ADR-TMC-007 § 4).
    Prune,
}

impl WriteSubcommand {
    pub(crate) fn words(self) -> &'static [&'static str] {
        match self {
            Self::UpdateRef => &["update-ref", "--stdin", "-z"],
            Self::IndexInfo => &["update-index", "-z", "--index-info"],
            Self::SkipWorktree => &["update-index", "--skip-worktree", "-z", "--stdin"],
            Self::PackObjects => &["pack-objects", "--revs", "--stdout", "--quiet"],
            Self::IndexPack => &["index-pack", "--stdin", "--strict", "--keep=gitraptor-tm"],
            Self::Repack => &["repack", "-d", "-q", "--geometric=2"],
            // Grace period of ADR-TMC-007 § 4 (⚠️ ASSUMPTION: 1 hour) for captures in flight.
            Self::Prune => &["prune", "--expire=1.hour.ago"],
        }
    }
}

impl WriteSubcommand {
    /// Every variant, for [`daemon_git_argvs`]; `every_write_subcommand_is_listed` keeps it whole.
    pub(crate) const ALL: [Self; 7] = [
        Self::UpdateRef,
        Self::IndexInfo,
        Self::SkipWorktree,
        Self::PackObjects,
        Self::IndexPack,
        Self::Repack,
        Self::Prune,
    ];
}

impl Subcommand {
    /// Every variant, for [`daemon_git_argvs`]; `every_read_subcommand_is_listed` keeps it whole.
    pub(crate) const ALL: [Self; 9] = [
        Self::Version,
        Self::ExecPath,
        Self::RevParse,
        Self::ForEachRef,
        Self::WorktreeList,
        Self::RevList,
        Self::MergeBase,
        Self::Log,
        Self::Config,
    ];
}

/// The argv (program excluded) of every `git` the daemon launches through the read layer and the
/// Time Machine write layer, each with the first word of its subcommand: the options are the
/// real ones, the paths placeholders. For the engine's fitness check of the daemon's inert
/// `git`s (ADR-GRP-012, amendment 2026-11-09), so a new subcommand cannot skip its
/// classification. The read layer's arguments after the subcommand are not included.
pub fn daemon_git_argvs() -> Vec<(&'static str, Vec<OsString>)> {
    let root = if cfg!(windows) { "C:\\r" } else { "/r" };
    let git_dir = Path::new(root).join(".git");
    let work_tree = Path::new(root).to_path_buf();
    let hooks = Path::new(root).join("hooks-empty");
    let global = Path::new(root).join("global-empty");
    let index = Path::new(root).join("index-tmp");
    let target = WriteTarget {
        git_dir: &git_dir,
        work_tree: Some(&work_tree),
        hooks_dir: &hooks,
        global_config: &global,
        index_file: Some(&index),
    };
    let mut out: Vec<(&'static str, Vec<OsString>)> = WriteSubcommand::ALL
        .iter()
        .map(|s| (s.words()[0], write_argv(&target, *s)))
        .collect();
    out.extend(Subcommand::ALL.iter().map(|s| {
        let mut argv: Vec<OsString> = FIXED_OPTIONS.iter().map(OsString::from).collect();
        argv.extend(s.words().iter().map(OsString::from));
        (s.words()[0], argv)
    }));
    out
}

/// Whether `args` (program excluded) is exactly the argv the daemon gives one of its `git`s that
/// cannot move a ref, `HEAD` or the list of worktrees (ADR-GRP-012, amendment 2026-11-09): a
/// template of the Time Machine write layer other than `update-ref`, or of the read layer's
/// `version`, `rev-parse`, `for-each-ref`, `rev-list`, `merge-base` or `log`. Fails closed.
pub fn is_daemon_inert_argv(args: &[OsString]) -> bool {
    inert_write_argv(args) || inert_read_argv(args)
}

/// `args` with `options` stripped from its front, word by word, or `None` if they differ.
fn strip_options<'a>(args: &'a [OsString], options: &[&str]) -> Option<&'a [OsString]> {
    let rest = args.get(options.len()..)?;
    args.iter()
        .zip(options)
        .all(|(a, o)| a == o)
        .then_some(rest)
}

/// The absolute path a single word carries after `prefix`, or `None`.
fn absolute_after<'a>(word: &'a OsString, prefix: &str) -> Option<&'a Path> {
    let path = os_strip_prefix(word, prefix)?;
    (!path.as_os_str().is_empty() && path.is_absolute()).then_some(path)
}

#[cfg(unix)]
fn os_strip_prefix<'a>(word: &'a OsStr, prefix: &str) -> Option<&'a Path> {
    use std::os::unix::ffi::OsStrExt;
    let rest = word.as_bytes().strip_prefix(prefix.as_bytes())?;
    Some(Path::new(OsStr::from_bytes(rest)))
}

/// Without a byte view of an `OsStr`, a word that is not Unicode fails closed.
#[cfg(not(unix))]
fn os_strip_prefix<'a>(word: &'a OsStr, prefix: &str) -> Option<&'a Path> {
    word.to_str()?.strip_prefix(prefix).map(Path::new)
}

/// The exact layout of [`write_argv`] for a subcommand other than `update-ref`, nothing after.
fn inert_write_argv(args: &[OsString]) -> bool {
    let Some(rest) = strip_options(args, WRITE_OPTIONS) else {
        return false;
    };
    let [c1, hooks, c2, safe, git_dir, rest @ ..] = rest else {
        return false;
    };
    if c1 != "-c"
        || c2 != "-c"
        || absolute_after(hooks, "core.hooksPath=").is_none()
        || absolute_after(safe, "safe.directory=").is_none()
        || absolute_after(git_dir, "--git-dir=").is_none()
    {
        return false;
    }
    let words = match rest.first() {
        Some(w) if os_strip_prefix(w, "--work-tree=").is_some() => {
            if absolute_after(w, "--work-tree=").is_none() {
                return false;
            }
            &rest[1..]
        }
        _ => rest,
    };
    WriteSubcommand::ALL
        .iter()
        .filter(|s| **s != WriteSubcommand::UpdateRef)
        .any(|s| words.len() == s.words().len() && words.iter().zip(s.words()).all(|(a, w)| a == w))
}

/// [`FIXED_OPTIONS`], the words of an inert read subcommand, then arguments none of which makes
/// Git write a file (`--output`).
fn inert_read_argv(args: &[OsString]) -> bool {
    const INERT_READS: [Subcommand; 6] = [
        Subcommand::Version,
        Subcommand::RevParse,
        Subcommand::ForEachRef,
        Subcommand::RevList,
        Subcommand::MergeBase,
        Subcommand::Log,
    ];
    let Some(rest) = strip_options(args, FIXED_OPTIONS) else {
        return false;
    };
    INERT_READS.iter().any(|s| {
        let words = s.words();
        let Some(tail) = rest.get(words.len()..) else {
            return false;
        };
        rest.iter().zip(words).all(|(a, w)| a == w)
            && tail
                .iter()
                .all(|a| a != "--output" && os_strip_prefix(a, "--output=").is_none())
    })
}

/// The closed list of the Guardrails write layer (ADR-GRD-001 § 7): only the activation key
/// `core.hooksPath`, only in the common `config` named with `--file`. Adding one requires
/// revising ADR-GRD-001 (checked by `tests/static_check.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardSubcommand {
    /// Writes the absolute path of the dispatchers folder (the commit point of the install).
    Set,
    /// Writes back the value the repo had at local level before the install (uninstall): the
    /// recorded value as is, which may be relative (`.husky/_`).
    Restore,
    /// Removes the key (a revert, or an uninstall with no previous local value).
    Unset,
    /// Reads the value in that one file, without includes.
    Get,
}

impl GuardSubcommand {
    pub(crate) fn words(self) -> &'static [&'static str] {
        match self {
            Self::Set | Self::Restore => &["config", "--no-includes"],
            Self::Unset => &["config", "--no-includes", "--unset"],
            Self::Get => &["config", "--no-includes", "--get"],
        }
    }
}

/// The reads of the Guardrails layer (ADR-GRD-001 § 5 and § 7): `core.hooksPath` as each
/// worktree sees it, with every level and file, and the `includeIf "onbranch:"` that could
/// change it. Apart from the read layer, which never lists configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardRead {
    HooksPathAll,
    OnbranchIncludes,
}

impl GuardRead {
    pub(crate) fn words(self) -> &'static [&'static str] {
        match self {
            Self::HooksPathAll => &[
                "config",
                "--show-scope",
                "--show-origin",
                "-z",
                "--get-all",
                "core.hooksPath",
            ],
            Self::OnbranchIncludes => &[
                "config",
                "--show-scope",
                "--name-only",
                "-z",
                "--get-regexp",
                "^includeif\\.onbranch:",
            ],
        }
    }
}

/// The null device: the global configuration of a Guardrails write-layer child. Also on Windows:
/// Git for Windows maps `/dev/null` itself, while `NUL` in `GIT_CONFIG_GLOBAL` makes Git fail
/// with "unable to access 'NUL'" (measured with Git 2.56.0.windows.1).
const NULL_DEVICE: &str = "/dev/null";

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

    /// Run `git config --file <config> … core.hooksPath [<value>]` for the Guardrails write layer
    /// (ADR-GRD-001 § 4 paso 3 and § 7): argv fixed here, no shell, no system or global
    /// configuration, no hooks (`git config` runs none) and the folder of `config` as cwd.
    pub(crate) fn run_guard(
        &self,
        git: &Path,
        config: &Path,
        subcommand: GuardSubcommand,
        value: Option<&Path>,
    ) -> Result<Output, ReadError> {
        let relative_ok = subcommand == GuardSubcommand::Restore;
        if !git.is_absolute()
            || !config.is_absolute()
            || value.is_some_and(|v| !relative_ok && !v.is_absolute())
            || value.is_some_and(|v| v.as_os_str().to_string_lossy().starts_with('-'))
        {
            return Err(ReadError::InvalidInput(
                "guard write layer paths must be absolute".into(),
            ));
        }
        let takes_value = matches!(subcommand, GuardSubcommand::Set | GuardSubcommand::Restore);
        if takes_value != value.is_some() {
            return Err(ReadError::InvalidInput("hooksPath value mismatch".into()));
        }
        let cwd = config
            .parent()
            .ok_or_else(|| ReadError::InvalidInput("config has no folder".into()))?;
        let mut argv: Vec<OsString> = subcommand.words().iter().map(OsString::from).collect();
        let mut file = OsString::from("--file=");
        file.push(config);
        argv.push(file);
        argv.push("core.hooksPath".into());
        if let Some(value) = value {
            argv.push(value.into());
        }
        if let Some(sink) = &self.argv_sink {
            let mut logged = vec![git.to_string_lossy().into_owned()];
            logged.extend(argv.iter().map(|a| a.to_string_lossy().into_owned()));
            sink.record(&logged);
        }
        let mut env: Vec<(OsString, OsString)> = self
            .child_env()
            .into_iter()
            .filter(|(k, _)| !key_matches(k, "HOME"))
            .collect();
        env.push(("GIT_CONFIG_NOSYSTEM".into(), "1".into()));
        env.push(("GIT_CONFIG_GLOBAL".into(), NULL_DEVICE.into()));
        let mut command = Command::new(git);
        command.args(&argv).env_clear().envs(env).current_dir(cwd);
        self.spawn_and_wait(command, Input::None, None, &subcommand.words().join(" "))
    }

    /// Run a read of the Guardrails layer in `worktree`: argv fixed here, the read allowlist of
    /// the environment (so the global and system levels count, `GIT_CONFIG_*` never does).
    pub(crate) fn run_guard_read(
        &self,
        git: &Path,
        worktree: &Path,
        read: GuardRead,
    ) -> Result<Output, ReadError> {
        if !git.is_absolute() || !worktree.is_absolute() {
            return Err(ReadError::InvalidInput(
                "guard layer paths must be absolute".into(),
            ));
        }
        let argv: Vec<OsString> = read.words().iter().map(OsString::from).collect();
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
            .current_dir(worktree);
        self.spawn_and_wait(command, Input::None, None, "config")
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

        #[cfg(target_os = "macos")]
        lower_background_child(&mut command);

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

/// A `git` started by background work inherits its thread's class (ADR-GRP-015 § 1, D9): when
/// the calling thread runs below `Default`, the child lowers its own nice and disk I/O policy
/// before `exec`. Any other thread installs nothing, so the child keeps `posix_spawn`.
/// XP-45: macOS only on purpose; elsewhere the child inherits the priority as is (D4).
#[cfg(target_os = "macos")]
fn lower_background_child(command: &mut Command) {
    use gitraptor_macsys::qos::{self, QosClass};
    if let Some(class @ (QosClass::Utility | QosClass::Background)) = qos::current() {
        qos::lower_child(command, class);
    }
}

#[cfg(test)]
#[path = "invoke_inert_tests.rs"]
mod inert_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A new variant fails to compile here until it is placed in `ALL`.
    #[test]
    fn every_write_subcommand_is_listed() {
        for s in WriteSubcommand::ALL {
            let at = match s {
                WriteSubcommand::UpdateRef => 0,
                WriteSubcommand::IndexInfo => 1,
                WriteSubcommand::SkipWorktree => 2,
                WriteSubcommand::PackObjects => 3,
                WriteSubcommand::IndexPack => 4,
                WriteSubcommand::Repack => 5,
                WriteSubcommand::Prune => 6,
            };
            assert_eq!(WriteSubcommand::ALL[at], s);
        }
    }

    /// A new variant fails to compile here until it is placed in `ALL`.
    #[test]
    fn every_read_subcommand_is_listed() {
        for s in Subcommand::ALL {
            let at = match s {
                Subcommand::Version => 0,
                Subcommand::ExecPath => 1,
                Subcommand::RevParse => 2,
                Subcommand::ForEachRef => 3,
                Subcommand::WorktreeList => 4,
                Subcommand::RevList => 5,
                Subcommand::MergeBase => 6,
                Subcommand::Log => 7,
                Subcommand::Config => 8,
            };
            assert_eq!(Subcommand::ALL[at], s);
        }
    }

    #[test]
    fn daemon_git_argvs_lists_every_subcommand_once() {
        let all = daemon_git_argvs();
        assert_eq!(
            all.len(),
            WriteSubcommand::ALL.len() + Subcommand::ALL.len()
        );
        assert!(all.iter().any(|(w, _)| *w == "update-ref"));
    }

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

    /// The guard key accepts only an absolute value, except the restore of the value the repo
    /// had (US-GRD-003), which may be relative; never one Git would read as an option.
    #[test]
    fn the_guard_key_takes_only_the_values_it_may() {
        let inv = Invoker::default();
        let git = Path::new("/usr/bin/git");
        let config = Path::new("/r/.git/config");
        let rejected = |sub, value: &str| {
            matches!(
                inv.run_guard(git, config, sub, Some(Path::new(value))),
                Err(ReadError::InvalidInput(_))
            )
        };
        assert!(rejected(GuardSubcommand::Set, ".husky/_"));
        assert!(rejected(GuardSubcommand::Set, "-x"));
        assert!(rejected(GuardSubcommand::Restore, "-x"));
        assert!(rejected(GuardSubcommand::Restore, "--file=/etc/x"));
        assert!(matches!(
            inv.run_guard(git, config, GuardSubcommand::Restore, None),
            Err(ReadError::InvalidInput(_))
        ));
        assert!(matches!(
            inv.run_guard(git, config, GuardSubcommand::Unset, Some(Path::new("/a"))),
            Err(ReadError::InvalidInput(_))
        ));
    }
}
