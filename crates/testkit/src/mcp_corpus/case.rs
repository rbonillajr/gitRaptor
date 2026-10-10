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
        let mut path = match &self.anchor {
            Anchor::Root => roots.root.clone(),
            Anchor::Repo => roots.repo.clone(),
            Anchor::OtherRepo => roots.other_repo.clone(),
            Anchor::Home => roots.home.clone(),
            Anchor::Worktree(name) => roots.root.join(format!("wt-{name}")),
        };
        path.extend(&self.rel);
        path
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
    /// Answers the case's messages must get: one per `call` repetition and one per `message`,
    /// and one for a `raw` line unless the case expects it to be ignored.
    pub fn expected_answers(&self) -> usize {
        let raw = usize::from(self.expect != Expect::Ignored);
        self.send
            .iter()
            .map(|send| match send {
                Send::Call { repeat, .. } => usize::try_from(*repeat).unwrap_or(usize::MAX),
                Send::Message(_) => 1,
                Send::Raw(_) => raw,
            })
            .sum()
    }

    /// Whether the case runs on `platform`.
    pub fn runs_on(&self, platform: Platform) -> bool {
        self.platforms.contains(&platform)
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
        match self {
            Self::Json(why) => write!(f, "not valid JSON: {why}"),
            Self::UnknownField(name) => write!(f, "unknown field {name}"),
            Self::Missing(name) => write!(f, "missing field {name}"),
            Self::Invalid { field, why } => write!(f, "invalid {field}: {why}"),
            Self::Location { field, why } => write!(f, "invalid location in {field}: {why}"),
            Self::Placeholder { field, name } => {
                write!(
                    f,
                    "unknown or unavailable placeholder {{{name}}} in {field}"
                )
            }
            Self::Tier(why) => write!(f, "tier mismatch: {why}"),
            Self::Platform(why) => write!(f, "platform mismatch: {why}"),
            Self::IdMismatch { id, file } => {
                write!(f, "id {id} does not match the file name {file}")
            }
            Self::Duplicate(id) => write!(f, "duplicate id {id}"),
            Self::Io(why) => write!(f, "i/o error: {why}"),
        }
    }
}

impl std::error::Error for CaseError {}

type Object = serde_json::Map<String, Value>;

const ROOT_KEYS: [&str; 12] = [
    "id",
    "title",
    "threats",
    "tier",
    "platforms",
    "pending",
    "known_gap",
    "setup",
    "session",
    "send",
    "expect",
    "forbidden",
];

/// Variables the runner owns: a case file may not set them (it would reach the real profile).
const FORBIDDEN_ENV: [&str; 2] = ["GITRAPTOR_PROFILE_DIR", "GITRAPTOR_AGENT_EXECUTABLES"];

fn invalid(field: &str, why: &str) -> CaseError {
    CaseError::Invalid {
        field: field.to_owned(),
        why: why.to_owned(),
    }
}

fn object<'a>(value: &'a Value, field: &str) -> Result<&'a Object, CaseError> {
    value
        .as_object()
        .ok_or_else(|| invalid(field, "must be an object"))
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, CaseError> {
    value
        .as_str()
        .ok_or_else(|| invalid(field, "must be a string"))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, CaseError> {
    value
        .as_array()
        .ok_or_else(|| invalid(field, "must be an array"))
}

fn check_keys(object: &Object, allowed: &[&str], context: &str) -> Result<(), CaseError> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        None => Ok(()),
        Some(key) if context.is_empty() => Err(CaseError::UnknownField(key.clone())),
        Some(key) => Err(CaseError::UnknownField(format!("{context}.{key}"))),
    }
}

fn required<'a>(object: &'a Object, key: &str) -> Result<&'a Value, CaseError> {
    object
        .get(key)
        .ok_or_else(|| CaseError::Missing(key.to_owned()))
}

fn string_list(value: &Value, field: &str, non_empty: bool) -> Result<Vec<String>, CaseError> {
    let items = array(value, field)?;
    if non_empty && items.is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    items
        .iter()
        .map(|item| text(item, field).map(str::to_owned))
        .collect()
}

fn is_kebab(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn parse_state(value: Option<&Value>, field: &str) -> Result<RepoState, CaseError> {
    match value {
        None => Ok(RepoState::None),
        Some(value) => match text(value, field)? {
            "none" => Ok(RepoState::None),
            "observed" => Ok(RepoState::Observed),
            "enabled" => Ok(RepoState::Enabled),
            _ => Err(invalid(field, "must be none, observed or enabled")),
        },
    }
}

fn parse_location(raw: &str, worktrees: &[Worktree], field: &str) -> Result<Location, CaseError> {
    let location = |why: &str| CaseError::Location {
        field: field.to_owned(),
        why: why.to_owned(),
    };
    let mut parts = raw.split('/');
    let head = parts.next().unwrap_or_default();
    let anchor = match head {
        "root" => Anchor::Root,
        "repo" => Anchor::Repo,
        "other_repo" => Anchor::OtherRepo,
        "home" => Anchor::Home,
        other => match other.strip_prefix("wt-") {
            Some(name) if worktrees.iter().any(|w| w.name == name) => {
                Anchor::Worktree(name.to_owned())
            }
            Some(_) => return Err(location("worktree is not declared in setup.worktrees")),
            None => return Err(location("unknown anchor")),
        },
    };
    let mut rel = Vec::new();
    for part in parts {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains(['\\', ':'])
            || part.chars().any(char::is_control)
        {
            return Err(location("components must be normal names"));
        }
        rel.push(part.to_owned());
    }
    Ok(Location { anchor, rel })
}

fn parse_setup(value: &Value) -> Result<Setup, CaseError> {
    let setup = object(value, "setup")?;
    check_keys(
        setup,
        &[
            "repo",
            "other_repo",
            "worktrees",
            "symlinks",
            "dirs",
            "path_trap",
        ],
        "setup",
    )?;
    let mut out = Setup {
        repo: parse_state(setup.get("repo"), "setup.repo")?,
        other_repo: parse_state(setup.get("other_repo"), "setup.other_repo")?,
        ..Setup::default()
    };
    if let Some(list) = setup.get("worktrees") {
        for item in array(list, "setup.worktrees")? {
            let entry = object(item, "setup.worktrees")?;
            check_keys(entry, &["name", "branch"], "setup.worktrees")?;
            let name = text(required(entry, "name")?, "setup.worktrees.name")?;
            let branch = text(required(entry, "branch")?, "setup.worktrees.branch")?;
            let name_ok = (1..=32).contains(&name.len())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !name_ok {
                return Err(invalid(
                    "setup.worktrees.name",
                    "must match [a-z0-9-]{1,32}",
                ));
            }
            let branch_ok = !branch.is_empty()
                && !branch.starts_with('-')
                && branch
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
            if !branch_ok {
                return Err(invalid(
                    "setup.worktrees.branch",
                    "must be a plain branch name",
                ));
            }
            if out.worktrees.iter().any(|w| w.name == name) {
                return Err(invalid("setup.worktrees.name", "declared twice"));
            }
            out.worktrees.push(Worktree {
                name: name.to_owned(),
                branch: branch.to_owned(),
            });
        }
    }
    if let Some(list) = setup.get("symlinks") {
        for item in array(list, "setup.symlinks")? {
            let entry = object(item, "setup.symlinks")?;
            check_keys(entry, &["at", "to"], "setup.symlinks")?;
            let at = text(required(entry, "at")?, "setup.symlinks.at")?;
            let to = text(required(entry, "to")?, "setup.symlinks.to")?;
            let at = parse_location(at, &out.worktrees, "setup.symlinks.at")?;
            if at.rel.is_empty() {
                return Err(CaseError::Location {
                    field: "setup.symlinks.at".to_owned(),
                    why: "must name something inside an anchor".to_owned(),
                });
            }
            let to = parse_location(to, &out.worktrees, "setup.symlinks.to")?;
            out.symlinks.push(Link { at, to });
        }
    }
    if let Some(list) = setup.get("dirs") {
        for item in array(list, "setup.dirs")? {
            let dir = parse_location(text(item, "setup.dirs")?, &out.worktrees, "setup.dirs")?;
            if dir.rel.is_empty() {
                return Err(CaseError::Location {
                    field: "setup.dirs".to_owned(),
                    why: "must name something inside an anchor".to_owned(),
                });
            }
            out.dirs.push(dir);
        }
    }
    if let Some(flag) = setup.get("path_trap") {
        out.path_trap = flag
            .as_bool()
            .ok_or_else(|| invalid("setup.path_trap", "must be a boolean"))?;
    }
    Ok(out)
}

fn parse_session(value: Option<&Value>, worktrees: &[Worktree]) -> Result<Session, CaseError> {
    let mut session = Session {
        cwd: Location {
            anchor: Anchor::Repo,
            rel: Vec::new(),
        },
        env: BTreeMap::new(),
        parent: Parent::Unattributed,
    };
    let Some(value) = value else {
        return Ok(session);
    };
    let map = object(value, "session")?;
    check_keys(map, &["cwd", "env", "parent"], "session")?;
    if let Some(cwd) = map.get("cwd") {
        session.cwd = parse_location(text(cwd, "session.cwd")?, worktrees, "session.cwd")?;
    }
    if let Some(env) = map.get("env") {
        for (key, value) in object(env, "session.env")? {
            let mut chars = key.chars();
            let key_ok = chars
                .next()
                .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
                && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            if !key_ok {
                return Err(invalid("session.env", "keys must match [A-Z_][A-Z0-9_]*"));
            }
            if FORBIDDEN_ENV.contains(&key.as_str()) {
                return Err(invalid("session.env", "the runner owns this variable"));
            }
            session
                .env
                .insert(key.clone(), text(value, "session.env")?.to_owned());
        }
    }
    if let Some(parent) = map.get("parent") {
        session.parent = match text(parent, "session.parent")? {
            "unattributed" => Parent::Unattributed,
            "agent" => Parent::Agent,
            _ => return Err(invalid("session.parent", "must be unattributed or agent")),
        };
    }
    Ok(session)
}

fn parse_platforms(value: &Value) -> Result<Vec<Platform>, CaseError> {
    let mut platforms = Vec::new();
    for name in string_list(value, "platforms", true)? {
        let platform = Platform::ALL
            .into_iter()
            .find(|p| p.as_str() == name)
            .ok_or_else(|| invalid("platforms", "must be macos, linux or windows"))?;
        if platforms.contains(&platform) {
            return Err(invalid("platforms", "lists a platform twice"));
        }
        platforms.push(platform);
    }
    Ok(platforms)
}

fn parse_send(value: &Value) -> Result<Vec<Send>, CaseError> {
    let list = array(value, "send")?;
    if list.is_empty() {
        return Err(invalid("send", "must not be empty"));
    }
    let mut sends = Vec::new();
    for item in list {
        let entry = object(item, "send")?;
        let forms = ["call", "raw", "message"]
            .iter()
            .filter(|key| entry.contains_key(**key))
            .count();
        if forms != 1 {
            return Err(invalid(
                "send",
                "each item needs exactly one of call, raw, message",
            ));
        }
        if let Some(call) = entry.get("call") {
            check_keys(entry, &["call", "arguments", "repeat"], "send")?;
            let repeat = match entry.get("repeat") {
                None => 1,
                Some(n) => n
                    .as_u64()
                    .filter(|n| (1..=200).contains(n))
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| invalid("send.repeat", "must be an integer from 1 to 200"))?,
            };
            sends.push(Send::Call {
                tool: text(call, "send.call")?.to_owned(),
                arguments: entry.get("arguments").cloned(),
                repeat,
            });
        } else if let Some(raw) = entry.get("raw") {
            check_keys(entry, &["raw"], "send")?;
            sends.push(Send::Raw(text(raw, "send.raw")?.to_owned()));
        } else if let Some(message) = entry.get("message") {
            check_keys(entry, &["message"], "send")?;
            let message = object(message, "send.message")?;
            if message.contains_key("id") || message.contains_key("jsonrpc") {
                return Err(invalid("send.message", "the runner sets id and jsonrpc"));
            }
            sends.push(Send::Message(message.clone()));
        }
    }
    Ok(sends)
}

fn parse_expect(value: &Value) -> Result<Expect, CaseError> {
    let map = object(value, "expect")?;
    check_keys(
        map,
        &["refusal", "protocol_error", "invalid_request", "ignored"],
        "expect",
    )?;
    let mut entries = map.iter();
    let (Some((kind, body)), None) = (entries.next(), entries.next()) else {
        return Err(invalid("expect", "needs exactly one key"));
    };
    let body = object(body, "expect")?;
    match kind.as_str() {
        "refusal" => {
            check_keys(body, &["code", "params"], "expect.refusal")?;
            let code = text(required(body, "code")?, "expect.refusal.code")?;
            if !is_kebab(code) {
                return Err(invalid("expect.refusal.code", "must be kebab-case"));
            }
            let params = match body.get("params") {
                None => Vec::new(),
                Some(list) => string_list(list, "expect.refusal.params", false)?,
            };
            Ok(Expect::Refusal {
                code: code.to_owned(),
                params,
            })
        }
        "protocol_error" => {
            check_keys(body, &["code", "message", "field"], "expect.protocol_error")?;
            let code = required(body, "code")?
                .as_i64()
                .ok_or_else(|| invalid("expect.protocol_error.code", "must be an integer"))?;
            let optional = |key: &str| -> Result<Option<String>, CaseError> {
                body.get(key)
                    .map(|v| text(v, "expect.protocol_error").map(str::to_owned))
                    .transpose()
            };
            Ok(Expect::ProtocolError {
                code,
                message: optional("message")?,
                field: optional("field")?,
            })
        }
        "invalid_request" | "ignored" => {
            check_keys(body, &[], "expect")?;
            Ok(if kind == "ignored" {
                Expect::Ignored
            } else {
                Expect::InvalidRequest
            })
        }
        _ => Err(CaseError::UnknownField(format!("expect.{kind}"))),
    }
}

/// Finds a `{name}` marker starting at byte `start` (which holds `{`). Anything that does not
/// look like a name (`{"`, `{1}`) is plain text, so JSON and `HEAD@{1}` pass through.
fn marker_at(text: &str, start: usize) -> Option<(usize, &str)> {
    let rest = &text[start + 1..];
    let close = rest.find('}')?;
    let name = &rest[..close];
    let mut chars = name.chars();
    let shaped = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | ':' | '-'));
    shaped.then_some((start + 1 + close + 1, name))
}

fn check_markers(value: &str, field: &str, setup: &Setup) -> Result<(), CaseError> {
    let mut at = 0;
    while let Some(offset) = value[at..].find('{') {
        let start = at + offset;
        at = start + 1;
        let Some((end, name)) = marker_at(value, start) else {
            continue;
        };
        let known = match name {
            "root" | "repo" | "other_repo" | "home" => true,
            "repo_id:repo" => setup.repo != RepoState::None,
            "repo_id:other_repo" => setup.other_repo != RepoState::None,
            _ => name
                .strip_prefix("canary:")
                .is_some_and(|canary| CANARY_NAMES.contains(&canary)),
        };
        if !known {
            return Err(CaseError::Placeholder {
                field: field.to_owned(),
                name: name.to_owned(),
            });
        }
        at = end;
    }
    Ok(())
}

fn check_markers_in(value: &Value, field: &str, setup: &Setup) -> Result<(), CaseError> {
    match value {
        Value::String(s) => check_markers(s, field, setup),
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| check_markers_in(item, field, setup)),
        Value::Object(map) => map.iter().try_for_each(|(key, item)| {
            check_markers(key, field, setup)?;
            check_markers_in(item, field, setup)
        }),
        _ => Ok(()),
    }
}

/// Parses one case file.
///
/// # Errors
/// Any [`CaseError`] except `IdMismatch`, `Duplicate` and `Io`, which belong to [`load_dir`].
pub fn parse(source: &str) -> Result<Case, CaseError> {
    let value: Value =
        serde_json::from_str(source).map_err(|why| CaseError::Json(why.to_string()))?;
    let root = object(&value, "case")?;
    check_keys(root, &ROOT_KEYS, "")?;

    let id = text(required(root, "id")?, "id")?;
    if !is_kebab(id) || id.len() > 64 {
        return Err(invalid(
            "id",
            "must be kebab-case and at most 64 characters",
        ));
    }
    let title = text(required(root, "title")?, "title")?;
    if !(1..=160).contains(&title.chars().count()) {
        return Err(invalid("title", "must be 1 to 160 characters"));
    }
    let threats = string_list(required(root, "threats")?, "threats", true)?;
    let tier = match text(required(root, "tier")?, "tier")? {
        "server" => Tier::Server,
        "engine" => Tier::Engine,
        _ => return Err(invalid("tier", "must be server or engine")),
    };
    let platforms = parse_platforms(required(root, "platforms")?)?;
    let pending = root
        .get("pending")
        .map(|v| text(v, "pending").map(str::to_owned))
        .transpose()?;
    let known_gap = root
        .get("known_gap")
        .map(|v| text(v, "known_gap").map(str::to_owned))
        .transpose()?;
    if pending.as_deref() == Some("") || known_gap.as_deref() == Some("") {
        return Err(invalid(
            "pending",
            "pending and known_gap must not be empty",
        ));
    }
    let setup = match root.get("setup") {
        Some(value) => parse_setup(value)?,
        None => Setup::default(),
    };
    let session = parse_session(root.get("session"), &setup.worktrees)?;

    if tier == Tier::Server && (setup != Setup::default() || session.parent == Parent::Agent) {
        return Err(CaseError::Tier(
            "a server case needs no setup and no agent parent".to_owned(),
        ));
    }
    let needs_unix = tier == Tier::Engine
        || !setup.symlinks.is_empty()
        || setup.path_trap
        || session.parent == Parent::Agent;
    if needs_unix && platforms.contains(&Platform::Windows) {
        return Err(CaseError::Platform(
            "engine, symlinks, path_trap and an agent parent exclude windows".to_owned(),
        ));
    }
    let everywhere = platforms.len() == Platform::ALL.len();
    if everywhere && pending.is_some() {
        return Err(invalid(
            "pending",
            "forbidden when the case runs on every platform",
        ));
    }
    if !everywhere && pending.is_none() {
        return Err(invalid(
            "pending",
            "required when the case skips a platform",
        ));
    }

    let send = parse_send(required(root, "send")?)?;
    let expect = parse_expect(required(root, "expect")?)?;
    if expect == Expect::Ignored && send.iter().any(|s| !matches!(s, Send::Raw(_))) {
        return Err(invalid(
            "expect",
            "ignored needs a send made only of raw lines",
        ));
    }
    let forbidden = match root.get("forbidden") {
        Some(list) => string_list(list, "forbidden", false)?,
        None => Vec::new(),
    };

    for item in &forbidden {
        check_markers(item, "forbidden", &setup)?;
    }
    for value in session.env.values() {
        check_markers(value, "session.env", &setup)?;
    }
    for item in &send {
        match item {
            Send::Call {
                tool, arguments, ..
            } => {
                check_markers(tool, "send.call", &setup)?;
                if let Some(arguments) = arguments {
                    check_markers_in(arguments, "send.arguments", &setup)?;
                }
            }
            Send::Raw(line) => check_markers(line, "send.raw", &setup)?,
            Send::Message(map) => {
                for (key, item) in map {
                    check_markers(key, "send.message", &setup)?;
                    check_markers_in(item, "send.message", &setup)?;
                }
            }
        }
    }

    Ok(Case {
        id: id.to_owned(),
        title: title.to_owned(),
        threats,
        tier,
        platforms,
        pending,
        known_gap,
        setup,
        session,
        send,
        expect,
        forbidden,
    })
}

/// Every `*.json` directly under `dir`, sorted by id. Reports every bad file, not only the first.
///
/// # Errors
/// One entry per file that does not load.
pub fn load_dir(dir: &Path) -> Result<Vec<Case>, Vec<(PathBuf, CaseError)>> {
    let entries = std::fs::read_dir(dir)
        .map_err(|why| vec![(dir.to_path_buf(), CaseError::Io(why.to_string()))])?;
    let mut paths = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "json") && path.is_file() {
                    paths.push(path);
                }
            }
            Err(why) => errors.push((dir.to_path_buf(), CaseError::Io(why.to_string()))),
        }
    }
    paths.sort();

    let mut cases: Vec<Case> = Vec::new();
    for path in paths {
        let loaded = std::fs::read_to_string(&path)
            .map_err(|why| CaseError::Io(why.to_string()))
            .and_then(|source| parse(&source));
        match loaded {
            Ok(case) => {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if stem != case.id {
                    errors.push((
                        path,
                        CaseError::IdMismatch {
                            id: case.id,
                            file: stem,
                        },
                    ));
                } else if cases.iter().any(|c| c.id == case.id) {
                    errors.push((path, CaseError::Duplicate(case.id)));
                } else {
                    cases.push(case);
                }
            }
            Err(why) => errors.push((path, why)),
        }
    }
    if errors.is_empty() {
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    } else {
        Err(errors)
    }
}

/// Replaces the `{...}` placeholders of `text` with `vars`, keyed by the name between the braces
/// (`repo`, `repo_id:other_repo`, `canary:dotenv-file`). Braces that do not form a name stay.
///
/// # Errors
/// `CaseError::Placeholder` for a name that is not in `vars`.
pub fn expand(text: &str, vars: &BTreeMap<String, String>) -> Result<String, CaseError> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(offset) = text[at..].find('{') {
        let start = at + offset;
        out.push_str(&text[at..start]);
        match marker_at(text, start) {
            Some((end, name)) => {
                let value = vars.get(name).ok_or_else(|| CaseError::Placeholder {
                    field: "text".to_owned(),
                    name: name.to_owned(),
                })?;
                out.push_str(value);
                at = end;
            }
            None => {
                out.push('{');
                at = start + 1;
            }
        }
    }
    out.push_str(&text[at..]);
    Ok(out)
}
