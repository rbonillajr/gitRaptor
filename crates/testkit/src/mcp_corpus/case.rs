//! The case model: one JSON file per attack, with a closed schema.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Names of the planted canaries a case may reference as `{canary:<name>}`.
pub const CANARY_NAMES: [&str; 8] = [
    "env-github-token",
    "env-aws-secret",
    "env-anthropic-key",
    "remote-userinfo",
    "remote-query",
    "dotenv-file",
    "commit-message",
    "git-extraheader",
];

/// Where a case is judged: before the engine, or against a real daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Server,
    Engine,
}

/// An operating system a case can run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    Macos,
    Linux,
    Windows,
}

impl Platform {
    /// Every platform, in report order.
    pub const ALL: [Self; 3] = [Self::Macos, Self::Linux, Self::Windows];

    /// The platform this build runs on, or `None` on any other OS.
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::Macos)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(windows) {
            Some(Self::Windows)
        } else {
            None
        }
    }

    /// The name used in case files and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }
}

/// How far the daemon knows a repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepoState {
    #[default]
    None,
    Observed,
    Enabled,
}

/// The root a [`Location`] hangs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    Root,
    Repo,
    OtherRepo,
    Home,
    Worktree(String),
}

/// An anchor plus normal path components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub anchor: Anchor,
    pub rel: Vec<String>,
}

/// The concrete folders of one case's temporary machine.
pub struct Roots {
    pub root: PathBuf,
    pub repo: PathBuf,
    pub other_repo: PathBuf,
    pub home: PathBuf,
}

impl Location {
    /// The absolute path of this location. `Worktree(n)` is `root/wt-<n>`.
    pub fn resolve(&self, roots: &Roots) -> PathBuf {
        roots.root.clone()
    }
}

/// A linked worktree of `repo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub name: String,
    pub branch: String,
}

/// A symlink to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub at: Location,
    pub to: Location,
}

/// The machine a case needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Setup {
    pub repo: RepoState,
    pub other_repo: RepoState,
    pub worktrees: Vec<Worktree>,
    pub symlinks: Vec<Link>,
    pub dirs: Vec<Location>,
    pub path_trap: bool,
}

/// Who launches `raptor-mcp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parent {
    #[default]
    Unattributed,
    Agent,
}

/// How the MCP server is launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub cwd: Location,
    pub env: BTreeMap<String, String>,
    pub parent: Parent,
}

/// One thing a case sends.
#[derive(Debug, Clone, PartialEq)]
pub enum Send {
    Call {
        tool: String,
        arguments: Option<Value>,
        repeat: u32,
    },
    Raw(String),
    Message(serde_json::Map<String, Value>),
}

/// What the case must get back.
#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    Refusal {
        code: String,
        params: Vec<String>,
    },
    ProtocolError {
        code: i64,
        message: Option<String>,
        field: Option<String>,
    },
    InvalidRequest,
    Ignored,
}

/// One attack.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub title: String,
    pub threats: Vec<String>,
    pub tier: Tier,
    pub platforms: Vec<Platform>,
    pub pending: Option<String>,
    /// A reference to a known server gap (optional `known_gap` key). Such a case is expected to
    /// get through today: the report counts it apart, and once it is rejected the gate asks to
    /// drop the mark.
    pub known_gap: Option<String>,
    pub setup: Setup,
    pub session: Session,
    pub send: Vec<Send>,
    pub expect: Expect,
    pub forbidden: Vec<String>,
}

impl Case {
    /// Answers the case's messages must get: one per `call` repetition and one per `message`.
    pub fn expected_answers(&self) -> usize {
        0
    }

    /// Whether the case runs on `platform`.
    pub fn runs_on(&self, platform: Platform) -> bool {
        let _ = platform;
        false
    }
}

/// Why a case file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseError {
    Json(String),
    UnknownField(String),
    Missing(String),
    Invalid { field: String, why: String },
    Location { field: String, why: String },
    Placeholder { field: String, name: String },
    Tier(String),
    Platform(String),
    IdMismatch { id: String, file: String },
    Duplicate(String),
    Io(String),
}

impl std::fmt::Display for CaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CaseError {}

/// Parses one case file.
///
/// # Errors
/// Any [`CaseError`] except `IdMismatch`, `Duplicate` and `Io`, which belong to [`load_dir`].
pub fn parse(text: &str) -> Result<Case, CaseError> {
    let _ = text;
    Err(CaseError::Json("stub".into()))
}

/// Every `*.json` directly under `dir`, sorted by id. Reports every bad file, not only the first.
///
/// # Errors
/// One entry per file that does not load.
pub fn load_dir(dir: &Path) -> Result<Vec<Case>, Vec<(PathBuf, CaseError)>> {
    let _ = dir;
    Ok(Vec::new())
}

/// Replaces the `{...}` placeholders of `text` with `vars`.
///
/// # Errors
/// `CaseError::Placeholder` for a name that is not in `vars`.
pub fn expand(text: &str, vars: &BTreeMap<String, String>) -> Result<String, CaseError> {
    let _ = vars;
    Ok(text.to_owned())
}
