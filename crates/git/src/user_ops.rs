//! Invocation of user operations (ADR-CKP-002 § 6 and § 11; ADR-GRP-009, Enmienda Cockpit): the
//! only way the user-operation executor of `crates/core` launches `git`. Only `executor` may
//! import it (static check in `crates/core/tests/executor_boundary.rs`).
//!
//! A closed, typed list of operations. Each one becomes an opaque [`UserGitCommand`] that can only
//! be launched: the resolved `git` by absolute path, no shell, the repo always explicit
//! (`--git-dir`, `--work-tree`), a fixed set of `-c` options that neutralize editors, pagers,
//! the network, replacement objects, background maintenance and rebase extras, stdin to null
//! (or the commit message, then closed), its own process group, captured and capped output, and
//! an environment built from scratch out of an allowlist plus the session variables the daemon
//! validated (M-01). There is no variant with `--amend`, `--no-verify`, `--allow-empty`,
//! `--exec` or a double `--force`.
//!
//! What the user configured still runs (NFR-07): hooks, filters, merge drivers, `rerere`,
//! identity and signing.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

use crate::{Oid, RefName, SystemGit};

/// Most bytes of captured stdout or stderr per stream.
pub const MAX_CAPTURED_BYTES: u64 = 256 * 1024;
/// Most entries of a declared `PATH` (⚠️ ASSUMPTION of ADR-CKP-002 § 6).
pub const MAX_PATH_ENTRIES: usize = 64;
/// Longest declared `PATH`, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Longest locale value.
const MAX_LOCALE_BYTES: usize = 64;
/// Editor used when the installed `raptor-no-editor` cannot be used (never `:`).
pub const FALLBACK_NO_EDITOR: &str = "/usr/bin/false";
/// Minimal `PATH` when the client declared none that passed.
const DEFAULT_PATH: &str = "/usr/bin:/bin";

/// Session variables a client may declare (ADR-CKP-002 § 6).
pub const SESSION_VARS: &[&str] = &[
    "PATH",
    "SSH_AUTH_SOCK",
    "GNUPGHOME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_COLLATE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
];

/// Options placed before every user operation.
const NEUTRALIZING: &[&str] = &[
    "core.fsmonitor=false",
    "core.pager=cat",
    "diff.external=",
    "protocol.allow=never",
    "credential.helper=",
    "submodule.recurse=false",
    "core.useReplaceRefs=false",
    "gc.auto=0",
    "maintenance.auto=false",
    "rebase.updateRefs=false",
    "rebase.autoStash=false",
    "rebase.autoSquash=false",
    "color.ui=false",
    "trace2.normalTarget=",
    "trace2.eventTarget=",
    "trace2.perfTarget=",
];

/// The closed list of user operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserOp {
    /// `merge --no-edit -m <message> <oid>`: an oid of the plan, never a branch name. The message
    /// comes from a fixed template with a validated branch name.
    MergeOid {
        oid: Oid,
        message: String,
    },
    MergeAbort,
    /// `rebase --no-update-refs --no-autosquash --no-autostash <onto>`.
    RebaseOnto {
        onto: Oid,
    },
    RebaseAbort,
    /// `commit -F -` of what is staged; the message goes through stdin.
    CommitStaged,
    /// `commit -F - -- <paths>`, paths taken literally (`GIT_LITERAL_PATHSPECS=1`).
    CommitPaths {
        paths: Vec<String>,
    },
    /// `worktree add -b <branch> <path> <start>`.
    WorktreeAdd {
        branch: RefName,
        path: PathBuf,
        start: Oid,
    },
    /// `worktree remove --force <path>` (one `--force`: a locked worktree is refused).
    WorktreeRemove {
        path: PathBuf,
    },
    /// `update-ref <ref> <new> <old>`: fails whole if the ref moved.
    UpdateRef {
        name: RefName,
        new: Oid,
        old: Oid,
    },
    /// `update-ref -d <ref> <old>`.
    DeleteRef {
        name: RefName,
        old: Oid,
    },
}

impl UserOp {
    fn words(&self) -> Result<Vec<OsString>, String> {
        let s = |t: &str| OsString::from(t);
        let full = |r: &RefName| -> Result<OsString, String> {
            if r.as_str().starts_with("refs/heads/") {
                Ok(s(r.as_str()))
            } else {
                Err("a ref update needs the full refs/heads/ name".into())
            }
        };
        let absolute = |p: &Path| -> Result<OsString, String> {
            if p.is_absolute() {
                Ok(p.as_os_str().to_owned())
            } else {
                Err("worktree path must be absolute".into())
            }
        };
        Ok(match self {
            Self::MergeOid { oid, message } => {
                if message.is_empty() || message.contains('\0') {
                    return Err("invalid merge message".into());
                }
                vec![
                    s("merge"),
                    s("--no-edit"),
                    s("-m"),
                    s(message),
                    s(&oid.to_hex()),
                ]
            }
            Self::MergeAbort => vec![s("merge"), s("--abort")],
            Self::RebaseOnto { onto } => vec![
                s("rebase"),
                s("--no-update-refs"),
                s("--no-autosquash"),
                s("--no-autostash"),
                s(&onto.to_hex()),
            ],
            Self::RebaseAbort => vec![s("rebase"), s("--abort")],
            Self::CommitStaged => vec![s("commit"), s("-F"), s("-")],
            Self::CommitPaths { paths } => {
                if paths.is_empty() {
                    return Err("no paths".into());
                }
                let mut w = vec![s("commit"), s("-F"), s("-"), s("--")];
                w.extend(paths.iter().map(|p| s(p)));
                w
            }
            Self::WorktreeAdd {
                branch,
                path,
                start,
            } => vec![
                s("worktree"),
                s("add"),
                s("-b"),
                s(branch.as_str()),
                absolute(path)?,
                s(&start.to_hex()),
            ],
            Self::WorktreeRemove { path } => {
                vec![s("worktree"), s("remove"), s("--force"), absolute(path)?]
            }
            Self::UpdateRef { name, new, old } => vec![
                s("update-ref"),
                full(name)?,
                s(&new.to_hex()),
                s(&old.to_hex()),
            ],
            Self::DeleteRef { name, old } => {
                vec![s("update-ref"), s("-d"), full(name)?, s(&old.to_hex())]
            }
        })
    }

    fn takes_message(&self) -> bool {
        matches!(self, Self::CommitStaged | Self::CommitPaths { .. })
    }
}

/// The repo, explicit: never discovered from the working directory (M-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoTarget {
    pub git_dir: PathBuf,
    /// Root of the worktree, for operations with a working tree; also the working directory.
    pub work_tree: Option<PathBuf>,
}

/// Session variables that passed validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEnv {
    vars: Vec<(String, String)>,
}

impl SessionEnv {
    pub fn names(&self) -> Vec<&str> {
        self.vars.iter().map(|(k, _)| k.as_str()).collect()
    }
}

/// Why a declared variable, or one `PATH` entry, was dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDiagnostic {
    pub var: String,
    pub code: &'static str,
}

fn inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

#[cfg(unix)]
fn writable_by_others(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o022 != 0
}

#[cfg(unix)]
fn owned_by_me(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.uid() == rustix::process::geteuid().as_raw()
}

fn path_entry_problem(entry: &Path, excluded: &[PathBuf]) -> Option<&'static str> {
    if !entry.is_absolute() {
        return Some("path-entry-relative");
    }
    let Ok(meta) = std::fs::metadata(entry) else {
        return Some("path-entry-missing");
    };
    if !meta.is_dir() {
        return Some("path-entry-missing");
    }
    #[cfg(unix)]
    if writable_by_others(&meta) {
        return Some("path-entry-writable-by-others");
    }
    let canonical = std::fs::canonicalize(entry).unwrap_or_else(|_| entry.to_owned());
    if inside_any(&canonical, excluded) || inside_any(entry, excluded) {
        return Some("path-entry-in-repo");
    }
    None
}

fn var_problem(name: &str, value: &str) -> Option<&'static str> {
    match name {
        "SSH_AUTH_SOCK" => {
            let p = Path::new(value);
            if !p.is_absolute() {
                return Some("not-absolute");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                let Ok(meta) = std::fs::symlink_metadata(p) else {
                    return Some("missing");
                };
                if !meta.file_type().is_socket() {
                    return Some("not-a-socket");
                }
                if !owned_by_me(&meta) {
                    return Some("other-owner");
                }
            }
            None
        }
        "GNUPGHOME" => {
            let p = Path::new(value);
            if !p.is_absolute() {
                return Some("not-absolute");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let Ok(meta) = std::fs::symlink_metadata(p) else {
                    return Some("missing");
                };
                if !meta.is_dir() {
                    return Some("not-a-directory");
                }
                if !owned_by_me(&meta) {
                    return Some("other-owner");
                }
                if meta.permissions().mode() & 0o777 != 0o700 {
                    return Some("permissions-not-0700");
                }
            }
            None
        }
        _ => {
            let ok = !value.is_empty()
                && value.len() <= MAX_LOCALE_BYTES
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'@' | b'-'));
            (!ok).then_some("invalid-locale")
        }
    }
}

/// Validates the variables a client declared (M-01). `excluded` are the roots no `PATH` entry
/// may fall in: every observed worktree, the common `.git` and the profile. What fails is
/// dropped with a diagnostic; the declared `PATH` never resolves `git`, the rejecting editor or
/// `raptor`, which the daemon always uses by absolute path.
pub fn validate_session_env(
    declared: &[(String, String)],
    excluded: &[PathBuf],
) -> (SessionEnv, Vec<SessionDiagnostic>) {
    let mut env = SessionEnv::default();
    let mut diags = Vec::new();
    let mut diag = |var: &str, code| {
        diags.push(SessionDiagnostic {
            var: var.to_owned(),
            code,
        });
    };
    for (name, value) in declared {
        if !SESSION_VARS.contains(&name.as_str()) {
            diag(name, "not-allowed");
            continue;
        }
        if env.vars.iter().any(|(k, _)| k == name) {
            diag(name, "duplicate");
            continue;
        }
        if value.contains('\0') {
            diag(name, "contains-nul");
            continue;
        }
        if name == "PATH" {
            if value.len() > MAX_PATH_BYTES {
                diag(name, "too-long");
                continue;
            }
            let mut kept = Vec::new();
            for (i, entry) in std::env::split_paths(value).enumerate() {
                if i >= MAX_PATH_ENTRIES {
                    diag(name, "too-many-entries");
                    break;
                }
                match path_entry_problem(&entry, excluded) {
                    None => kept.push(entry),
                    Some(code) => diag(name, code),
                }
            }
            if let Ok(joined) = std::env::join_paths(&kept)
                && !kept.is_empty()
            {
                env.vars
                    .push((name.clone(), joined.to_string_lossy().into_owned()));
            }
            continue;
        }
        match var_problem(name, value) {
            None => env.vars.push((name.clone(), value.clone())),
            Some(code) => diag(name, code),
        }
    }
    (env, diags)
}

/// The rejecting editor (L-01): the installed `raptor-no-editor` when its path is absolute and
/// clean (Git then runs it without a shell), `/usr/bin/false` otherwise. Never `:`.
pub fn rejecting_editor(installed: Option<&Path>) -> PathBuf {
    let clean = |p: &Path| {
        p.is_absolute()
            && p.to_str().is_some_and(|t| {
                t.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
            })
            && p.is_file()
    };
    match installed {
        Some(p) if clean(p) => p.to_owned(),
        _ => PathBuf::from(FALLBACK_NO_EDITOR),
    }
}

/// A user operation ready to launch. Opaque: it can be inspected and launched, not changed.
pub struct UserGitCommand {
    git: PathBuf,
    argv: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    cwd: PathBuf,
    message: Option<Vec<u8>>,
}

impl std::fmt::Debug for UserGitCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The environment and the message may hold secrets or user content (SEC-05).
        f.debug_struct("UserGitCommand")
            .field("argv", &self.argv)
            .field("cwd", &self.cwd)
            .finish_non_exhaustive()
    }
}

/// Collects the capped output of a launched operation.
pub struct OutputCollector {
    stdout: JoinHandle<Vec<u8>>,
    stderr: JoinHandle<Vec<u8>>,
}

impl OutputCollector {
    /// Waits for both streams to close. Untrusted text, capped per stream.
    pub fn collect(self) -> (Vec<u8>, Vec<u8>) {
        (
            self.stdout.join().unwrap_or_default(),
            self.stderr.join().unwrap_or_default(),
        )
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = (&mut pipe).take(MAX_CAPTURED_BYTES).read_to_end(&mut buf);
            // Keep draining so the child never blocks on a full pipe.
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        }
        buf
    })
}

impl UserGitCommand {
    /// Builds the launch of `op` on `target`. `message` is required by a commit and refused by
    /// anything else. `home` is the daemon's `HOME`.
    pub fn new(
        git: &SystemGit,
        target: &RepoTarget,
        op: &UserOp,
        message: Option<&str>,
        session: &SessionEnv,
        no_editor: &Path,
        home: Option<&Path>,
    ) -> Result<Self, String> {
        if !git.path.is_absolute() || !no_editor.is_absolute() {
            return Err("git and the rejecting editor need absolute paths".into());
        }
        if !target.git_dir.is_absolute()
            || target
                .work_tree
                .as_deref()
                .is_some_and(|w| !w.is_absolute())
        {
            return Err("the repo must be explicit and absolute".into());
        }
        let message = match (op.takes_message(), message) {
            (true, Some(m)) if !m.is_empty() && !m.contains('\0') => Some(m.as_bytes().to_vec()),
            (true, _) => return Err("a commit needs a message".into()),
            (false, None) => None,
            (false, Some(_)) => return Err("only a commit takes a message".into()),
        };
        let editor = no_editor.to_string_lossy();
        let mut argv: Vec<OsString> = Vec::new();
        for opt in NEUTRALIZING {
            argv.push("-c".into());
            argv.push((*opt).into());
        }
        argv.push("-c".into());
        argv.push(format!("core.editor={editor}").into());
        argv.push("-c".into());
        argv.push(format!("sequence.editor={editor}").into());
        let mut git_dir = OsString::from("--git-dir=");
        git_dir.push(&target.git_dir);
        argv.push(git_dir);
        if let Some(wt) = &target.work_tree {
            let mut w = OsString::from("--work-tree=");
            w.push(wt);
            argv.push(w);
        }
        argv.extend(op.words()?);

        let mut env: Vec<(OsString, OsString)> = vec![
            ("GIT_EDITOR".into(), no_editor.into()),
            ("GIT_SEQUENCE_EDITOR".into(), no_editor.into()),
            ("GIT_MERGE_AUTOEDIT".into(), "no".into()),
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("GIT_PAGER".into(), "cat".into()),
            ("GIT_NO_REPLACE_OBJECTS".into(), "1".into()),
            ("GIT_LITERAL_PATHSPECS".into(), "1".into()),
        ];
        if let Some(home) = home {
            env.push(("HOME".into(), home.into()));
        }
        if !session.vars.iter().any(|(k, _)| k == "PATH") {
            env.push(("PATH".into(), DEFAULT_PATH.into()));
        }
        env.extend(
            session
                .vars
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        );
        let cwd = target
            .work_tree
            .clone()
            .unwrap_or_else(|| target.git_dir.clone());
        Ok(Self {
            git: git.path.clone(),
            argv,
            env,
            cwd,
            message,
        })
    }

    /// The argv after the executable, for the audit and the tests.
    pub fn argv(&self) -> Vec<String> {
        self.argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// Names of the variables the child receives (never their values).
    pub fn env_names(&self) -> Vec<String> {
        self.env
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    /// Launches `git`: direct child, its own process group, only fds 0 to 2 (the standard library
    /// opens everything else close-on-exec), stdin null or the message then closed.
    pub fn spawn(self) -> std::io::Result<(Child, OutputCollector)> {
        let mut cmd = Command::new(&self.git);
        cmd.args(&self.argv)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if self.message.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        // Pendiente: etapa de validación multiplataforma (CREATE_NO_WINDOW on Windows).
        let mut child = cmd.spawn()?;
        if let (Some(message), Some(mut stdin)) = (self.message, child.stdin.take()) {
            std::thread::spawn(move || {
                let _ = stdin.write_all(&message);
                // Dropping closes it: the hooks get EOF.
            });
        }
        let collector = OutputCollector {
            stdout: drain(child.stdout.take()),
            stderr: drain(child.stderr.take()),
        };
        Ok((child, collector))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GitVersion;

    fn git() -> SystemGit {
        SystemGit {
            path: PathBuf::from("/usr/bin/git"),
            version: GitVersion::parse("git version 2.50.0").unwrap(),
        }
    }

    fn target() -> RepoTarget {
        RepoTarget {
            git_dir: PathBuf::from("/w/repo/.git"),
            work_tree: Some(PathBuf::from("/w/repo")),
        }
    }

    fn oid() -> Oid {
        Oid::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap()
    }

    fn build(op: UserOp, message: Option<&str>) -> Result<UserGitCommand, String> {
        UserGitCommand::new(
            &git(),
            &target(),
            &op,
            message,
            &SessionEnv::default(),
            Path::new(FALLBACK_NO_EDITOR),
            Some(Path::new("/home/u")),
        )
    }

    /// ADR-CKP-002 § 6: explicit repo, neutralized programs, no network, no forbidden variants.
    #[test]
    fn every_operation_is_explicit_and_neutralized() {
        let ops = [
            UserOp::MergeOid {
                oid: oid(),
                message: "Merge branch 'feat-x'".into(),
            },
            UserOp::MergeAbort,
            UserOp::RebaseOnto { onto: oid() },
            UserOp::RebaseAbort,
            UserOp::WorktreeAdd {
                branch: RefName::new("feat/x").unwrap(),
                path: PathBuf::from("/w/repo-feat-x"),
                start: oid(),
            },
            UserOp::WorktreeRemove {
                path: PathBuf::from("/w/repo-feat-x"),
            },
            UserOp::UpdateRef {
                name: RefName::new("refs/heads/main").unwrap(),
                new: oid(),
                old: oid(),
            },
            UserOp::DeleteRef {
                name: RefName::new("refs/heads/feat/x").unwrap(),
                old: oid(),
            },
        ];
        for op in ops {
            let argv = build(op.clone(), None).unwrap().argv();
            for needle in [
                "--git-dir=/w/repo/.git",
                "--work-tree=/w/repo",
                "protocol.allow=never",
                "core.useReplaceRefs=false",
                "core.editor=/usr/bin/false",
                "sequence.editor=/usr/bin/false",
                "gc.auto=0",
                "rebase.updateRefs=false",
            ] {
                assert!(argv.iter().any(|a| a == needle), "{op:?} lacks {needle}");
            }
            for forbidden in [
                "--amend",
                "--no-verify",
                "--allow-empty",
                "--exec",
                "push",
                "fetch",
            ] {
                assert!(
                    !argv.iter().any(|a| a == forbidden),
                    "{op:?} has {forbidden}"
                );
            }
            assert!(argv.iter().filter(|a| *a == "--force").count() <= 1);
        }
    }

    #[test]
    fn the_commit_message_never_travels_in_argv() {
        let msg = "$(rm -rf ~) --amend";
        let cmd = build(
            UserOp::CommitPaths {
                paths: vec![":(glob)**".into()],
            },
            Some(msg),
        )
        .unwrap();
        let argv = cmd.argv();
        assert!(!argv.iter().any(|a| a.contains("rm -rf")));
        assert!(argv.ends_with(&["-F".into(), "-".into(), "--".into(), ":(glob)**".into()]));
        assert!(cmd.env_names().contains(&"GIT_LITERAL_PATHSPECS".into()));
        assert!(build(UserOp::CommitStaged, None).is_err());
        assert!(build(UserOp::RebaseAbort, Some("x")).is_err());
        // Short ref names never update a ref.
        let short = UserOp::DeleteRef {
            name: RefName::new("main").unwrap(),
            old: oid(),
        };
        assert!(build(short, None).is_err());
    }

    #[test]
    fn the_environment_is_built_from_scratch() {
        let names = build(UserOp::MergeAbort, None).unwrap().env_names();
        for name in &names {
            assert!(
                !name.starts_with("LD_") && !name.starts_with("DYLD_"),
                "{name}"
            );
            assert!(
                !matches!(
                    name.as_str(),
                    "GIT_ASKPASS" | "SSH_ASKPASS" | "GIT_SSH_COMMAND"
                ),
                "{name}"
            );
        }
        assert!(names.contains(&"PATH".into()));
    }

    /// L-01 (Validación 24): a path with spaces falls back to `/usr/bin/false`; never `:`.
    #[test]
    fn the_rejecting_editor_is_clean() {
        let dir = tempfile::tempdir().unwrap();
        let spaced = dir.path().join("with space");
        std::fs::create_dir(&spaced).unwrap();
        let exe = spaced.join("raptor-no-editor");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(
            rejecting_editor(Some(&exe)),
            PathBuf::from(FALLBACK_NO_EDITOR)
        );
        assert_eq!(rejecting_editor(None), PathBuf::from(FALLBACK_NO_EDITOR));
        let clean = dir.path().join("raptor-no-editor");
        std::fs::write(&clean, "").unwrap();
        let canonical = std::fs::canonicalize(&clean).unwrap();
        if canonical
            .to_str()
            .unwrap()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        {
            assert_eq!(rejecting_editor(Some(&canonical)), canonical);
        }
    }

    /// M-01 (Validación 20).
    #[cfg(unix)]
    #[test]
    fn session_variables_are_validated() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let good = root.join("bin");
        let open = root.join("open");
        let repo = root.join("repo");
        let gpg_loose = root.join("gpg-loose");
        let gpg_tight = root.join("gpg-tight");
        for d in [&good, &open, &repo, &gpg_loose, &gpg_tight] {
            std::fs::create_dir(d).unwrap();
        }
        std::fs::set_permissions(&good, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        std::fs::set_permissions(&gpg_loose, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&gpg_tight, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = std::env::join_paths([
            good.clone(),
            PathBuf::from("relative/bin"),
            open.clone(),
            repo.join("."),
        ])
        .unwrap();
        let declared = vec![
            ("PATH".to_owned(), path.to_string_lossy().into_owned()),
            (
                "SSH_AUTH_SOCK".to_owned(),
                good.to_string_lossy().into_owned(),
            ),
            (
                "GNUPGHOME".to_owned(),
                gpg_loose.to_string_lossy().into_owned(),
            ),
            ("LANG".to_owned(), "en_US.UTF-8;rm".to_owned()),
            ("LC_ALL".to_owned(), "es_ES.UTF-8".to_owned()),
            ("LD_PRELOAD".to_owned(), "/x.so".to_owned()),
        ];
        let (env, diags) = validate_session_env(&declared, std::slice::from_ref(&repo));
        let codes: Vec<_> = diags.iter().map(|d| (d.var.as_str(), d.code)).collect();
        assert!(
            codes.contains(&("PATH", "path-entry-relative")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("PATH", "path-entry-writable-by-others")),
            "{codes:?}"
        );
        assert!(codes.contains(&("PATH", "path-entry-in-repo")), "{codes:?}");
        assert!(
            codes.contains(&("SSH_AUTH_SOCK", "not-a-socket")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("GNUPGHOME", "permissions-not-0700")),
            "{codes:?}"
        );
        assert!(codes.contains(&("LANG", "invalid-locale")), "{codes:?}");
        assert!(codes.contains(&("LD_PRELOAD", "not-allowed")), "{codes:?}");
        assert_eq!(env.names(), ["PATH", "LC_ALL"]);
        assert_eq!(env.vars[0].1, good.to_string_lossy());
        let (env, _) = validate_session_env(
            &[("GNUPGHOME".into(), gpg_tight.to_string_lossy().into_owned())],
            &[],
        );
        assert_eq!(env.names(), ["GNUPGHOME"]);
    }
}
