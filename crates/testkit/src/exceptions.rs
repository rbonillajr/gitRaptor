//! Per-scenario exceptions (INF-GRP-001, Alcance): the only differences a scenario may show.
//! Outside the scenarios that declare them, no exception applies and the criterion is binary:
//! zero differences (ADR-GRP-009).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::fingerprint::{Change, ChangeKind, Field, Key, Snapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exception {
    /// Anything at or below `prefix` in `scope` (engine data in the profile).
    Subtree { scope: String, prefix: PathBuf },
    /// Exactly this path may be created, removed or modified (an autostart artifact).
    Exact { scope: String, path: PathBuf },
    /// This directory may change its mtime and ctime only: an allowed entry was created
    /// or removed inside it.
    DirTimes { scope: String, path: PathBuf },
    /// This Git config file may change only in the listed keys (`section.key` or
    /// `section.subsection.key`, case-insensitive for section and key).
    ConfigKeys {
        scope: String,
        path: PathBuf,
        keys: Vec<String>,
    },
}

/// The exceptions of one scenario.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exceptions(pub Vec<Exception>);

impl Exceptions {
    /// No exception: zero differences.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn with(mut self, e: Exception) -> Self {
        self.0.push(e);
        self
    }

    /// Both sets of exceptions.
    pub fn and(mut self, other: Self) -> Self {
        self.0.extend(other.0);
        self
    }

    /// The engine's own data and state folders of the profile (ADR-GRP-006 § 1). The profile's
    /// configuration folder is not included: the engine only reads it.
    pub fn engine_profile(scope: &str) -> Self {
        Self::none()
            .with(Exception::Subtree {
                scope: scope.into(),
                prefix: "data".into(),
            })
            .with(Exception::Subtree {
                scope: scope.into(),
                prefix: "state".into(),
            })
    }

    /// Autostart artifacts of `raptor daemon enable` (PQ-1, ADR-GRP-005 § 3): exactly these
    /// paths, plus the times of their parent directories. Used only by the US-GRP-004 suite.
    pub fn autostart(artifacts: &[(&str, &Path)]) -> Self {
        let mut out = Self::none();
        for (scope, path) in artifacts {
            out = out.with(Exception::Exact {
                scope: (*scope).into(),
                path: path.to_path_buf(),
            });
            if let Some(parent) = path.parent() {
                out = out.with(Exception::DirTimes {
                    scope: (*scope).into(),
                    path: parent.to_owned(),
                });
            }
        }
        out
    }

    /// Guardrails explicit install (ADR-GRD-001 § 7, Enmienda 2026-10-04): only `core.hooksPath`
    /// in the common dir `config` and the `gitraptor/` folder of the common dir. `common_dir` is
    /// relative to `scope` (e.g. `.git`). Used only by the INF-GRD-001 install scenarios; after
    /// uninstall the scenario goes back to [`Exceptions::none`].
    pub fn guardrails_install(scope: &str, common_dir: &Path) -> Self {
        Self::none()
            .with(Exception::ConfigKeys {
                scope: scope.into(),
                path: common_dir.join("config"),
                keys: vec!["core.hookspath".into()],
            })
            .with(Exception::Subtree {
                scope: scope.into(),
                prefix: common_dir.join("gitraptor"),
            })
            .with(Exception::DirTimes {
                scope: scope.into(),
                path: common_dir.to_owned(),
            })
    }

    /// After a Guardrails uninstall (compared with the snapshot before install): the common dir
    /// `config` may have been rewritten (new inode and times) but must say exactly the same, and
    /// the common dir may only change its times. Anything else is a difference.
    pub fn guardrails_uninstalled(scope: &str, common_dir: &Path) -> Self {
        Self::none()
            .with(Exception::ConfigKeys {
                scope: scope.into(),
                path: common_dir.join("config"),
                keys: Vec::new(),
            })
            .with(Exception::DirTimes {
                scope: scope.into(),
                path: common_dir.to_owned(),
            })
    }

    /// Paths whose content must be kept in the snapshot to evaluate the exceptions.
    pub fn keep_content(&self) -> BTreeSet<Key> {
        self.0
            .iter()
            .filter_map(|e| match e {
                Exception::ConfigKeys { scope, path, .. } => Some((scope.clone(), path.clone())),
                _ => None,
            })
            .collect()
    }

    /// The changes that no exception allows.
    pub fn filter(&self, changes: &[Change], before: &Snapshot, after: &Snapshot) -> Vec<Change> {
        changes
            .iter()
            .filter(|c| !self.0.iter().any(|e| allows(e, c, before, after)))
            .cloned()
            .collect()
    }
}

fn allows(e: &Exception, c: &Change, before: &Snapshot, after: &Snapshot) -> bool {
    match e {
        Exception::Subtree { scope, prefix } => *scope == c.scope && c.path.starts_with(prefix),
        Exception::Exact { scope, path } => *scope == c.scope && c.path == *path,
        Exception::DirTimes { scope, path } => {
            *scope == c.scope
                && c.path == *path
                && matches!(&c.kind, ChangeKind::Modified(fields)
                    if fields.iter().all(|f| matches!(f, Field::Mtime | Field::Ctime)))
        }
        Exception::ConfigKeys { scope, path, keys } => {
            if *scope != c.scope || c.path != *path || !matches!(c.kind, ChangeKind::Modified(_)) {
                return false;
            }
            let content = |s: &Snapshot| s.get(scope, path).and_then(|e| e.content.clone());
            match (content(before), content(after)) {
                (Some(b), Some(a)) => match (without_keys(&b, keys), without_keys(&a, keys)) {
                    (Some(b), Some(a)) => b == a,
                    _ => false,
                },
                _ => false,
            }
        }
    }
}

/// A Git config file as Git itself parses it (`git config --file <tmp> --list -z`): the
/// `(key, value)` entries in file order, without the allowed keys. Formatting, comments, quoting,
/// escapes, continuations and empty sections do not count; the order does, because a later
/// entry overrides an `include.path` read before it (SPIKE-GRD-001 § 5.1, Q-GRD-29). A value-less
/// key (`[core] bare`) has no value. `None` when Git cannot parse the file.
pub fn without_keys(content: &[u8], keys: &[String]) -> Option<Vec<(String, Option<String>)>> {
    let tmp = tempfile::NamedTempFile::new().ok()?;
    std::fs::write(tmp.path(), content).ok()?;
    let out = config_command(tmp.path()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let keys: Vec<String> = keys.iter().map(|k| k.to_lowercase()).collect();
    let mut entries = Vec::new();
    for raw in out.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let raw = String::from_utf8_lossy(raw);
        let (key, value) = match raw.split_once('\n') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (raw.into_owned(), None),
        };
        if !keys.contains(&key.to_lowercase()) {
            entries.push((key, value));
        }
    }
    Some(entries)
}

/// `git config --file <path> --list -z` with an isolated environment: no system, global or
/// command-line configuration, and includes not followed (the default with `--file`).
fn config_command(path: &Path) -> std::process::Command {
    let mut c = std::process::Command::new(crate::fixture::git_from_path());
    c.arg("config")
        .arg("--file")
        .arg(path)
        .args(["--list", "-z"])
        .current_dir(path.parent().unwrap_or(Path::new(".")))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_DIR");
    c
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}
