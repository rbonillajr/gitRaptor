//! The closed, versioned catalog of user operations and its two-phase
//! contract (ADR-CKP-002 § 1, § 2 and § 11; TS-CKP-002).
//!
//! A client never writes to a repo in one call: `operation.prepare` returns a
//! plan with a `plan_id` and a fingerprint and touches nothing;
//! `operation.run` executes that plan, and only that plan, from the same
//! connection. The daemon fixes the [`Layer`] from the resolved requester,
//! never from the client, and the layer decides which operations exist for
//! the caller.
//!
//! Every parameter is typed and rejects unknown fields; oids never travel in
//! parameters: they come from the plan.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::timemachine::{Invalid, RequesterView, Surface};
use crate::{Untrusted, UntrustedName};

/// Version of the catalog. Adding an operation or an optional parameter
/// bumps it by a minor step (here: the next integer kept compatible by
/// clients that ignore unknown ids); changing a semantic, a class, the
/// governance or a surface mark requires revising ADR-CKP-002.
pub const CATALOG_VERSION: u32 = 1;
/// Lifetime of a prepared plan (ADR-CKP-002 § 2, step 6).
pub const PLAN_TTL_MS: u64 = 60_000;
/// Live plans per connection; the oldest is dropped past it.
pub const MAX_PLANS_PER_CONNECTION: usize = 4;
/// Requests waiting for one repo's write lock; one more is told "busy".
pub const MAX_QUEUED_PER_REPO: usize = 8;
/// Longest run of an operation with layer `mcp` (ADR-CKP-002 § 6,
/// ⚠️ ASSUMPTION until S-MCP-1).
pub const MCP_TIME_LIMIT_MS: u64 = 300_000;
/// Longest commit message, in bytes.
pub const MAX_COMMIT_MESSAGE_BYTES: usize = 16 * 1024;
/// Most literal paths in one commit.
pub const MAX_COMMIT_PATHS: usize = 256;
/// Longest literal path of a commit, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Longest snapshot label, in characters.
pub const MAX_LABEL_CHARS: usize = 64;
/// Longest snapshot label, in bytes: checked before the characters are walked, so
/// a huge string never costs a pass over it.
pub const MAX_LABEL_BYTES: usize = 256;
/// Most combining marks in a row inside a snapshot label.
const MAX_LABEL_MARK_RUN: usize = 4;
/// Longest new branch name, in bytes.
pub const MAX_BRANCH_BYTES: usize = 200;
/// Most warning codes in a plan or in a run request.
pub const MAX_WARNINGS: usize = 16;

/// The operations of the catalog, version 1.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum OperationId {
    MergeIntoBase,
    RebaseOntoBase,
    DiscardWorktree,
    CreateWorktree,
    Commit,
    Snapshot,
    AbortInProgress,
    OpenInEditor,
}

impl OperationId {
    pub fn as_str(self) -> &'static str {
        entry(self).name
    }
}

/// Class of an operation (ADR-TMC-007 § 2): when in doubt, destructive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OperationClass {
    Destructive,
    NonDestructive,
    /// Writes nothing in the repo (a Time Machine capture, or nothing).
    NoRepoWrite,
}

/// The normalized operation Guardrails evaluates (BR-VAL-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GovernedAs {
    Merge,
    Rebase,
    /// Remove a worktree and delete its branch.
    DeleteWorktree,
    CreateWorktree,
    Commit,
}

/// Who the daemon says is asking, for Guardrails and for what is offered
/// (ADR-CKP-002 § 4, M-03). Never declared by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// An unattributed caller that passes the reserved checks 1 to 3.
    Cockpit,
    /// An agent, or anyone who does not pass those checks.
    Mcp,
}

/// Static description of one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: OperationId,
    pub name: &'static str,
    pub class: OperationClass,
    pub governed: Option<GovernedAs>,
    /// Offered to layer `cockpit`.
    pub cockpit: bool,
    /// Offered to layer `mcp` (a tool of BR-14).
    pub mcp: bool,
    /// Runs inside the protected operation (intent, prior snapshot,
    /// execution, record). `open-in-editor` writes nothing and does not.
    pub protected: bool,
    /// Story that owns the operation's own logic (its step and its own
    /// preconditions). Until then `operation.prepare` answers
    /// `NOT_IMPLEMENTED` with `implemented_by` = this story.
    pub owner: &'static str,
}

impl CatalogEntry {
    pub fn offered_to(&self, layer: Layer) -> bool {
        match layer {
            Layer::Cockpit => self.cockpit,
            Layer::Mcp => self.mcp,
        }
    }
}

const fn op(
    id: OperationId,
    name: &'static str,
    class: OperationClass,
    governed: Option<GovernedAs>,
    (cockpit, mcp): (bool, bool),
    owner: &'static str,
) -> CatalogEntry {
    CatalogEntry {
        id,
        name,
        class,
        governed,
        cockpit,
        mcp,
        // The editor and the manual snapshot run outside the protected operation: the
        // snapshot is the recovery point itself, so it has no prior of its own.
        protected: !matches!(id, OperationId::OpenInEditor | OperationId::Snapshot),
        owner,
    }
}

/// The catalog (ADR-CKP-002 § 1).
pub const CATALOG: &[CatalogEntry] = &[
    op(
        OperationId::MergeIntoBase,
        "merge-into-base",
        OperationClass::Destructive,
        Some(GovernedAs::Merge),
        (true, false),
        "US-CKP-014",
    ),
    op(
        OperationId::RebaseOntoBase,
        "rebase-onto-base",
        OperationClass::Destructive,
        Some(GovernedAs::Rebase),
        (true, true),
        "US-CKP-015",
    ),
    op(
        OperationId::DiscardWorktree,
        "discard-worktree",
        OperationClass::Destructive,
        Some(GovernedAs::DeleteWorktree),
        (true, false),
        "US-CKP-017",
    ),
    op(
        OperationId::CreateWorktree,
        "create-worktree",
        OperationClass::NonDestructive,
        Some(GovernedAs::CreateWorktree),
        (true, true),
        "US-CKP-018",
    ),
    op(
        OperationId::Commit,
        "commit",
        OperationClass::NonDestructive,
        Some(GovernedAs::Commit),
        (false, true),
        "US-MCP-009",
    ),
    op(
        OperationId::Snapshot,
        "snapshot",
        OperationClass::NoRepoWrite,
        None,
        (false, true),
        "US-MCP-008",
    ),
    op(
        OperationId::AbortInProgress,
        "abort-in-progress",
        OperationClass::Destructive,
        None,
        (true, false),
        "US-CKP-016",
    ),
    op(
        OperationId::OpenInEditor,
        "open-in-editor",
        OperationClass::NoRepoWrite,
        None,
        (true, false),
        "US-CKP-013",
    ),
];

/// The entry of an operation. Every id has one (checked by a test).
pub fn entry(id: OperationId) -> &'static CatalogEntry {
    CATALOG.iter().find(|e| e.id == id).unwrap_or(&CATALOG[0])
}

// ----- Typed arguments -------------------------------------------------------

/// Arguments of an operation without its own parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

/// `create-worktree`. `path` exists only on a full connection: over MCP the
/// new path always comes from the template (H-02), so the schema refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWorktreeArgs {
    pub branch: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// `create-worktree` over MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpCreateWorktreeArgs {
    pub branch: String,
}

/// `commit`: literal paths, or everything staged when `paths` is absent.
/// The message never travels by argv and is never recorded or returned.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitArgs {
    pub message: String,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

impl std::fmt::Debug for CommitArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The message is user content (SEC-05): never in a log.
        f.debug_struct("CommitArgs")
            .field("message_bytes", &self.message.len())
            .field("paths", &self.paths)
            .finish()
    }
}

/// `snapshot`: a short label kept as untrusted text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotArgs {
    pub label: String,
}

/// `open-in-editor`: an absolute path inside an observed worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenInEditorArgs {
    pub path: String,
}

/// The validated arguments of one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationArgs {
    MergeIntoBase,
    RebaseOntoBase,
    DiscardWorktree,
    CreateWorktree(CreateWorktreeArgs),
    Commit(CommitArgs),
    Snapshot(SnapshotArgs),
    AbortInProgress,
    OpenInEditor(OpenInEditorArgs),
}

impl OperationArgs {
    /// Parses and checks the arguments of `op`. `mcp` is an MCP connection,
    /// whose schema for `create-worktree` has no `path`.
    pub fn parse(op: OperationId, args: &Map<String, Value>, mcp: bool) -> Result<Self, Invalid> {
        fn typed<T: serde::de::DeserializeOwned>(args: &Map<String, Value>) -> Result<T, Invalid> {
            serde_json::from_value(Value::Object(args.clone()))
                .map_err(|_| Invalid::new("args", "do not match the operation's parameters"))
        }
        let parsed = match op {
            OperationId::MergeIntoBase => typed::<NoArgs>(args).map(|_| Self::MergeIntoBase)?,
            OperationId::RebaseOntoBase => typed::<NoArgs>(args).map(|_| Self::RebaseOntoBase)?,
            OperationId::DiscardWorktree => typed::<NoArgs>(args).map(|_| Self::DiscardWorktree)?,
            OperationId::AbortInProgress => typed::<NoArgs>(args).map(|_| Self::AbortInProgress)?,
            OperationId::CreateWorktree if mcp => {
                let a: McpCreateWorktreeArgs = typed(args)?;
                Self::CreateWorktree(CreateWorktreeArgs {
                    branch: a.branch,
                    path: None,
                })
            }
            OperationId::CreateWorktree => Self::CreateWorktree(typed(args)?),
            OperationId::Commit => Self::Commit(typed(args)?),
            OperationId::Snapshot => Self::Snapshot(typed(args)?),
            OperationId::OpenInEditor => Self::OpenInEditor(typed(args)?),
        };
        parsed.check()?;
        Ok(parsed)
    }

    fn check(&self) -> Result<(), Invalid> {
        match self {
            Self::CreateWorktree(a) => {
                check_new_branch(&a.branch)?;
                if let Some(p) = &a.path {
                    check_absolute("path", p)?;
                }
                Ok(())
            }
            Self::Commit(a) => {
                if a.message.trim().is_empty() {
                    return Err(Invalid::new("message", "required"));
                }
                if a.message.len() > MAX_COMMIT_MESSAGE_BYTES {
                    return Err(Invalid::new("message", "too long"));
                }
                if a.message.contains('\0') {
                    return Err(Invalid::new("message", "contains NUL"));
                }
                if let Some(paths) = &a.paths {
                    if paths.is_empty() {
                        return Err(Invalid::new(
                            "paths",
                            "empty: omit it to commit what is staged",
                        ));
                    }
                    if paths.len() > MAX_COMMIT_PATHS {
                        return Err(Invalid::new("paths", "too many"));
                    }
                    for p in paths {
                        check_literal_relative(p)?;
                    }
                }
                Ok(())
            }
            Self::Snapshot(a) => check_snapshot_label(&a.label),
            Self::OpenInEditor(a) => check_absolute("path", &a.path),
            Self::MergeIntoBase
            | Self::RebaseOntoBase
            | Self::DiscardWorktree
            | Self::AbortInProgress => Ok(()),
        }
    }

    /// The arguments as they enter the plan's fingerprint: the commit
    /// message only by its hash input (the caller hashes it), never the
    /// text itself.
    pub fn fingerprint_view(&self) -> Value {
        match self {
            Self::CreateWorktree(a) => serde_json::json!({ "branch": a.branch, "path": a.path }),
            Self::Commit(a) => serde_json::json!({
                "message_len": a.message.len(),
                "message_sum": message_sum(&a.message),
                "paths": a.paths,
            }),
            Self::Snapshot(a) => serde_json::json!({ "label": a.label }),
            Self::OpenInEditor(a) => serde_json::json!({ "path": a.path }),
            _ => Value::Object(Map::new()),
        }
    }
}

/// A stable, non-cryptographic digest of the commit message (FNV-1a 64) for
/// the plan view; the daemon hashes the whole plan with SHA-256.
fn message_sum(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Validates a manual snapshot label: 1 to [`MAX_LABEL_CHARS`] characters, none of
/// [`is_forbidden_char`] nor of the hidden or odd characters of [`is_hidden_label_char`],
/// no whitespace at either end, and no combining mark first or more than four in a row.
/// Shared by `raptor-mcp` and the daemon; the daemon's check is the one that decides.
///
/// # Errors
/// An [`Invalid`] naming the `label` field. The byte length is checked first, so an
/// oversized string is refused without walking its characters.
pub fn check_snapshot_label(label: &str) -> Result<(), Invalid> {
    let bad = |why| Invalid::new("label", why);
    if label.len() > MAX_LABEL_BYTES {
        return Err(bad("must be 1 to 64 characters"));
    }
    let chars = label.chars().count();
    if chars == 0 || chars > MAX_LABEL_CHARS {
        return Err(bad("must be 1 to 64 characters"));
    }
    if label
        .chars()
        .any(|c| is_forbidden_char(c) || is_hidden_label_char(c))
    {
        return Err(bad("contains control, hidden or unusual characters"));
    }
    if label.trim() != label {
        return Err(bad("must not start or end with spaces"));
    }
    let mut run = 0usize;
    for c in label.chars() {
        if is_combining_mark(c) {
            run += 1;
            if run > MAX_LABEL_MARK_RUN {
                return Err(bad("has too many combining marks in a row"));
            }
        } else {
            run = 0;
        }
    }
    if label.chars().next().is_some_and(is_combining_mark) {
        return Err(bad("must not start with a combining mark"));
    }
    Ok(())
}

/// Characters that render as nothing or as something other than what they are, beyond
/// [`is_forbidden_char`]: fillers, soft hyphen, variation selectors, specials, private
/// use and non-characters. Only labels are held to this list.
fn is_hidden_label_char(c: char) -> bool {
    let u = u32::from(c);
    matches!(
        c,
        '\u{00ad}'
            | '\u{034f}'
            | '\u{115f}'
            | '\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180f}'
            | '\u{3164}'
            | '\u{ffa0}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{e0100}'..='\u{e01ef}'
            | '\u{fff0}'..='\u{fffb}'
            | '\u{e000}'..='\u{f8ff}'
            | '\u{f0000}'..='\u{10ffff}'
            | '\u{fdd0}'..='\u{fdef}'
    ) || u & 0xfffe == 0xfffe
}

/// Nonspacing and enclosing marks (Unicode Mn, Me) of the blocks in common use. The
/// standard library has no general-category lookup and the workspace adds no crate for
/// it, so this is a table of ranges: a script missing from it is simply not counted,
/// which only loosens the run limit, never the hidden-character list above.
fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036f}'
            | '\u{0483}'..='\u{0489}'
            | '\u{0591}'..='\u{05bd}'
            | '\u{05bf}'
            | '\u{05c1}'..='\u{05c2}'
            | '\u{05c4}'..='\u{05c5}'
            | '\u{05c7}'
            | '\u{0610}'..='\u{061a}'
            | '\u{064b}'..='\u{065f}'
            | '\u{0670}'
            | '\u{06d6}'..='\u{06dc}'
            | '\u{06df}'..='\u{06e4}'
            | '\u{06e7}'..='\u{06e8}'
            | '\u{06ea}'..='\u{06ed}'
            | '\u{0711}'
            | '\u{0730}'..='\u{074a}'
            | '\u{07a6}'..='\u{07b0}'
            | '\u{07eb}'..='\u{07f3}'
            | '\u{0900}'..='\u{0902}'
            | '\u{093a}'
            | '\u{093c}'
            | '\u{0941}'..='\u{0948}'
            | '\u{094d}'
            | '\u{0951}'..='\u{0957}'
            | '\u{0962}'..='\u{0963}'
            | '\u{0e31}'
            | '\u{0e34}'..='\u{0e3a}'
            | '\u{0e47}'..='\u{0e4e}'
            | '\u{0eb1}'
            | '\u{0eb4}'..='\u{0ebc}'
            | '\u{0ec8}'..='\u{0ece}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20f0}'
            | '\u{2cef}'..='\u{2cf1}'
            | '\u{2de0}'..='\u{2dff}'
            | '\u{302a}'..='\u{302d}'
            | '\u{3099}'..='\u{309a}'
            | '\u{a66f}'..='\u{a672}'
            | '\u{a674}'..='\u{a67d}'
            | '\u{fe20}'..='\u{fe2f}'
    )
}

/// C0, DEL, C1, bidi, line and paragraph separators, zero-width and the
/// Tags block (SEC-12, L-03).
pub fn is_forbidden_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{2069}'
                | '\u{feff}'
                | '\u{e0000}'..='\u{e007f}'
        )
}

fn check_absolute(field: &'static str, p: &str) -> Result<(), Invalid> {
    if p.is_empty() || p.len() > MAX_PATH_BYTES || p.contains('\0') {
        return Err(Invalid::new(field, "invalid path"));
    }
    let path = std::path::Path::new(p);
    if !path.is_absolute() || p.starts_with("\\\\") || p.starts_with("//") {
        return Err(Invalid::new(field, "must be an absolute local path"));
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Invalid::new(field, "must not contain .."));
    }
    Ok(())
}

/// A commit path: relative to the worktree, taken literally (never a
/// pathspec magic nor an option), without `..` or `.git`.
fn check_literal_relative(p: &str) -> Result<(), Invalid> {
    let bad = |why| Invalid::new("paths", why);
    if p.is_empty() || p.len() > MAX_PATH_BYTES || p.contains('\0') {
        return Err(bad("invalid path"));
    }
    if p.starts_with('/') || p.starts_with('\\') || p.starts_with('-') {
        return Err(bad("must be relative to the worktree"));
    }
    if p.chars().any(is_forbidden_char) {
        return Err(bad("contains control characters"));
    }
    for part in p.split(['/', '\\']) {
        if part == ".." || part.eq_ignore_ascii_case(".git") {
            return Err(bad("must stay inside the worktree"));
        }
    }
    Ok(())
}

/// A new branch name (L-02): `check-ref-format` rules for a branch, no
/// leading `-`, no `HEAD` or `@`, no `@{`, no 40 or 64 hex digits, no
/// `refs/` prefix and none of the SEC-12 characters; at most
/// [`MAX_BRANCH_BYTES`].
pub fn check_new_branch(name: &str) -> Result<(), Invalid> {
    let bad = |why| Invalid::new("branch", why);
    if name.is_empty() || name.len() > MAX_BRANCH_BYTES {
        return Err(bad("must be 1 to 200 bytes"));
    }
    if name == "HEAD" || name == "@" || name.contains("@{") {
        return Err(bad("reserved name"));
    }
    if name.starts_with('-') || name.starts_with("refs/") {
        return Err(bad("must not start with - or refs/"));
    }
    if matches!(name.len(), 40 | 64) && name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("looks like an object id"));
    }
    if name.chars().any(|c| {
        is_forbidden_char(c) || c == ' ' || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return Err(bad("contains a character Git does not allow"));
    }
    if name.contains("..")
        || name.contains("//")
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
    {
        return Err(bad("not a valid ref name"));
    }
    for part in name.split('/') {
        if part.starts_with('.') || part.ends_with(".lock") {
            return Err(bad("not a valid ref name"));
        }
    }
    Ok(())
}

/// `<branch>` for a path template (ADR-CKP-002 § 1, H-02): `/` becomes `-`;
/// an empty or `.` result is refused.
pub fn sanitized_branch(branch: &str) -> Result<String, Invalid> {
    let out = branch.replace('/', "-");
    if out.is_empty() || out == "." || out == ".." {
        return Err(Invalid::new("branch", "cannot name a worktree folder"));
    }
    Ok(out)
}

// ----- Methods ---------------------------------------------------------------

/// `operation.describe` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeResult {
    pub catalog_version: u32,
    pub operations: Vec<DescribedOperation>,
}

/// One operation as `operation.describe` shows it. Texts are the client's
/// (i18n, NFR-10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribedOperation {
    pub id: OperationId,
    pub class: OperationClass,
    pub governed: Option<GovernedAs>,
    pub cockpit: bool,
    pub mcp: bool,
    /// JSON Schema of the arguments for this connection.
    pub args_schema: Value,
}

/// The catalog as a connection sees it: over MCP only the operations with
/// the MCP mark, with the MCP schemas.
pub fn describe(mcp: bool) -> DescribeResult {
    let schema = |id: OperationId| -> Value {
        let s = match id {
            OperationId::CreateWorktree if mcp => schemars::schema_for!(McpCreateWorktreeArgs),
            OperationId::CreateWorktree => schemars::schema_for!(CreateWorktreeArgs),
            OperationId::Commit => schemars::schema_for!(CommitArgs),
            OperationId::Snapshot => schemars::schema_for!(SnapshotArgs),
            OperationId::OpenInEditor => schemars::schema_for!(OpenInEditorArgs),
            _ => schemars::schema_for!(NoArgs),
        };
        serde_json::to_value(s).unwrap_or(Value::Null)
    };
    DescribeResult {
        catalog_version: CATALOG_VERSION,
        operations: CATALOG
            .iter()
            .filter(|e| !mcp || e.mcp)
            .map(|e| DescribedOperation {
                id: e.id,
                class: e.class,
                governed: e.governed,
                cockpit: e.cockpit,
                mcp: e.mcp,
                args_schema: schema(e.id),
            })
            .collect(),
    }
}

/// A variable of the client's session that hooks may need (M-01). The
/// daemon validates each one and drops, with a diagnostic, what fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum SessionVar {
    #[serde(rename = "PATH")]
    Path,
    #[serde(rename = "SSH_AUTH_SOCK")]
    SshAuthSock,
    #[serde(rename = "GNUPGHOME")]
    GnupgHome,
    #[serde(rename = "LANG")]
    Lang,
    #[serde(rename = "LC_ALL")]
    LcAll,
    #[serde(rename = "LC_CTYPE")]
    LcCtype,
    #[serde(rename = "LC_COLLATE")]
    LcCollate,
    #[serde(rename = "LC_MESSAGES")]
    LcMessages,
    #[serde(rename = "LC_MONETARY")]
    LcMonetary,
    #[serde(rename = "LC_NUMERIC")]
    LcNumeric,
    #[serde(rename = "LC_TIME")]
    LcTime,
}

impl SessionVar {
    pub fn name(self) -> &'static str {
        match self {
            Self::Path => "PATH",
            Self::SshAuthSock => "SSH_AUTH_SOCK",
            Self::GnupgHome => "GNUPGHOME",
            Self::Lang => "LANG",
            Self::LcAll => "LC_ALL",
            Self::LcCtype => "LC_CTYPE",
            Self::LcCollate => "LC_COLLATE",
            Self::LcMessages => "LC_MESSAGES",
            Self::LcMonetary => "LC_MONETARY",
            Self::LcNumeric => "LC_NUMERIC",
            Self::LcTime => "LC_TIME",
        }
    }
}

/// One declared session variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionVarValue {
    pub name: SessionVar,
    pub value: String,
}

/// `operation.prepare` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareParams {
    /// An operation of the catalog. An unknown one fails the schema.
    pub operation: OperationId,
    /// Worktree the operation acts on. Required on a full connection; over
    /// MCP it comes from the caller's working folder and must be absent.
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub args: Map<String, Value>,
    #[serde(default)]
    pub surface: Option<Surface>,
    /// Session variables for the hooks (M-01).
    #[serde(default)]
    pub session_env: Vec<SessionVarValue>,
}

impl PrepareParams {
    /// Format checks before anything is read; the arguments themselves are
    /// checked against the operation by [`OperationArgs::parse`].
    pub fn validate(&self) -> Result<(), Invalid> {
        if self.args.len() > crate::timemachine::MAX_ARGS_KEYS {
            return Err(Invalid::new("args", "too many arguments"));
        }
        let size = serde_json::to_vec(&self.args).map_or(usize::MAX, |v| v.len());
        if size > crate::timemachine::MAX_ARGS_BYTES {
            return Err(Invalid::new("args", "too large"));
        }
        if self.session_env.len() > SESSION_VARS_MAX {
            return Err(Invalid::new("session_env", "too many variables"));
        }
        if self
            .session_env
            .iter()
            .any(|v| v.value.len() > SESSION_VALUE_MAX_BYTES)
        {
            return Err(Invalid::new("session_env", "value too long"));
        }
        Ok(())
    }
}

/// Most declared session variables.
const SESSION_VARS_MAX: usize = 16;
/// Longest declared session value (a `PATH` is checked again by the daemon).
const SESSION_VALUE_MAX_BYTES: usize = 8 * 1024;

/// A warning the client must enumerate to run the plan (§ 2, step 3).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    /// The base is behind the predicted conflict state (⚡).
    PredictedConflict,
    /// An agent session is active in the worktree.
    ActiveSession,
    /// The work affected belongs to another actor (needs confirmation).
    OtherActorsWork,
    /// Something the snapshot does not recover would be lost.
    Unrecoverable,
}

/// What the Guardrails decision says, as preview at prepare time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionView {
    Allow,
    Deny,
    /// The operation is not governed (BR-VAL-002).
    NotGoverned,
    /// Guardrails could not evaluate it (no decision engine yet, TS-CKP-003).
    /// Never read as "allowed": a governed operation is refused at run.
    NotEvaluated,
}

/// A diagnostic about the request, e.g. a session variable dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub subject: Untrusted,
}

/// `operation.prepare` result for a full connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareResult {
    pub plan_id: String,
    pub catalog_version: u32,
    pub operation: OperationId,
    pub layer: Layer,
    pub requester: RequesterView,
    /// Hex SHA-256 of the plan, including requester, layer and version.
    pub fingerprint: String,
    pub warnings: Vec<WarningCode>,
    pub decision: DecisionView,
    /// A one-use confirmation challenge bound to this plan (ADR-TMC-005 § 3),
    /// when the plan needs one and the caller may confirm.
    pub challenge: Option<String>,
    pub expires_in_ms: u64,
    pub diagnostics: Vec<Diagnostic>,
}

/// `operation.prepare` result over MCP: codes and typed fields only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpPrepareResult {
    pub plan_id: String,
    pub catalog_version: u32,
    pub operation: OperationId,
    pub fingerprint: String,
    pub warnings: Vec<WarningCode>,
    pub decision: DecisionView,
    pub expires_in_ms: u64,
}

impl PrepareResult {
    pub fn for_mcp(&self) -> McpPrepareResult {
        McpPrepareResult {
            plan_id: self.plan_id.clone(),
            catalog_version: self.catalog_version,
            operation: self.operation,
            fingerprint: self.fingerprint.clone(),
            warnings: self.warnings.clone(),
            decision: self.decision,
            expires_in_ms: self.expires_in_ms,
        }
    }
}

/// `operation.run` parameters: the execute phase of a prepared plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    pub plan_id: String,
    /// Every warning code of the plan, no more and no less.
    #[serde(default)]
    pub accepted_warnings: Vec<WarningCode>,
    /// The challenge of the plan, when it carried one.
    #[serde(default)]
    pub confirmation: Option<String>,
}

/// `operation.cancel` parameters (layer `cockpit` only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelParams {
    pub operation_id: String,
}

/// `operation.cancel` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelResult {
    /// The operation was running and was asked to stop.
    pub requested: bool,
}

/// How an executed plan ended (ADR-CKP-002 § 2, step 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OperationOutcome {
    Done,
    /// Git left an operation in progress (mode `stop`).
    Stopped,
    /// An atomic rebase hit a conflict and was aborted: the repo is as before.
    ConflictReverted,
    FailedUnchanged,
    /// Failed with changes: Undo is offered.
    FailedChanged,
    Rejected,
    /// The prior snapshot failed.
    Aborted,
    Cancelled,
}

/// Why a plan was refused before anything ran. No oplog entry is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RejectReason {
    /// The state changed between prepare and run (fingerprint, requester
    /// or layer).
    StateChanged,
    /// Unknown, expired, or prepared by another connection.
    PlanUnknown,
    /// The operation has no mark for the caller's layer (M-03).
    NotAvailableForLayer,
    /// An unattributed caller without layer `cockpit` (TQ-7).
    UnattributedWithoutCockpit,
    /// The caller descends from a child of the executor (H-01).
    ExecutorDescendant,
    /// The accepted warnings do not match the plan's.
    WarningsMismatch,
    /// The plan needs a confirmation the caller did not give.
    ConfirmationRequired,
    /// The confirmation challenge was unknown, reused, expired or bound to
    /// another plan or process.
    ChallengeInvalid,
    /// Another actor's work, and the caller may not confirm (ADR-TMC-005 § 2).
    ForeignWork,
    /// Another actor's work, and this platform does not offer its confirmation yet: Windows
    /// (BR-CKP-AUTH-003, TQ-14, ADR-CKP-002 § 3). Additive: an older client ignores it.
    ConfirmationUnavailable,
    /// Another agent session is present in the worktree (Q-MCP-21).
    OtherSessionPresent,
    OperationInProgress,
    DetachedHead,
    /// A Git lock is held in the scope; it is never removed.
    GitBusy,
    WorktreeLocked,
    BranchCheckedOutElsewhere,
    /// The repo has `info/grafts` (L-04).
    Grafts,
    /// The repo or worktree identity changed (M-05).
    RepoIdentityChanged,
    GuardrailsDenied,
    /// The new path of a worktree is refused (H-02).
    NewPathRefused,
    /// Too many requests wait for this repo.
    QueueFull,
    /// The daemon is stopping.
    DaemonStopping,
    /// The same requester's previous manual snapshot has not finished. Only a
    /// connection with `operation.snapshot` receives it.
    WriteInProgress,
}

/// `operation.run` result of a `snapshot` plan (only with `operation.snapshot`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRunResult {
    pub snapshot_id: String,
    /// Folder name of the worktree root, never its path.
    pub worktree: UntrustedName,
    pub label: UntrustedName,
    pub requester: RequesterView,
    pub layer: Layer,
    /// Always `done` on success.
    pub outcome: OperationOutcome,
}

/// `data` of an `OPERATION_REJECTED` error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RejectedData {
    pub reason: RejectReason,
}

/// Data of the `operation.queued`, `operation.started` and
/// `operation.finished` events (Q-CKP-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationEventData {
    pub repo_id: String,
    pub operation: OperationId,
    pub layer: Layer,
    /// The oplog id, once the operation has one.
    #[serde(default)]
    pub operation_id: Option<String>,
    /// Requests ahead in the queue (`operation.queued`).
    #[serde(default)]
    pub position: Option<u32>,
    #[serde(default)]
    pub outcome: Option<OperationOutcome>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_operation_has_one_entry() {
        let ids = [
            OperationId::MergeIntoBase,
            OperationId::RebaseOntoBase,
            OperationId::DiscardWorktree,
            OperationId::CreateWorktree,
            OperationId::Commit,
            OperationId::Snapshot,
            OperationId::AbortInProgress,
            OperationId::OpenInEditor,
        ];
        assert_eq!(CATALOG.len(), ids.len());
        for id in ids {
            let e = entry(id);
            assert_eq!(e.id, id);
            assert_eq!(serde_json::to_value(id).unwrap(), json!(e.name));
        }
    }

    /// ADR-CKP-002 § 1 and § 12: the marks of version 1.
    #[test]
    fn marks_match_the_adr() {
        let mcp: Vec<_> = CATALOG.iter().filter(|e| e.mcp).map(|e| e.name).collect();
        assert_eq!(
            mcp,
            ["rebase-onto-base", "create-worktree", "commit", "snapshot"]
        );
        let cockpit: Vec<_> = CATALOG
            .iter()
            .filter(|e| e.cockpit)
            .map(|e| e.name)
            .collect();
        assert_eq!(
            cockpit,
            [
                "merge-into-base",
                "rebase-onto-base",
                "discard-worktree",
                "create-worktree",
                "abort-in-progress",
                "open-in-editor"
            ]
        );
        // Every destructive operation is protected; the editor and the manual snapshot
        // are not.
        for e in CATALOG {
            let outside = matches!(e.id, OperationId::OpenInEditor | OperationId::Snapshot);
            assert_eq!(e.protected, !outside, "{}", e.name);
        }
        assert_eq!(describe(true).operations.len(), 4);
        assert_eq!(describe(false).catalog_version, CATALOG_VERSION);
    }

    #[test]
    fn unknown_operations_and_fields_fail_the_schema() {
        assert!(serde_json::from_value::<PrepareParams>(json!({ "operation": "push" })).is_err());
        assert!(
            serde_json::from_value::<PrepareParams>(
                json!({ "operation": "commit", "layer": "cockpit" })
            )
            .is_err()
        );
        let args = |v: Value| v.as_object().unwrap().clone();
        assert!(
            OperationArgs::parse(
                OperationId::RebaseOntoBase,
                &args(json!({"onto": "x"})),
                false
            )
            .is_err()
        );
        // H-02: over MCP `create-worktree` has no path.
        let abs = if cfg!(windows) { r"C:\tmp\x" } else { "/tmp/x" };
        let with_path = args(json!({ "branch": "feat/x", "path": abs }));
        assert!(OperationArgs::parse(OperationId::CreateWorktree, &with_path, true).is_err());
        assert!(OperationArgs::parse(OperationId::CreateWorktree, &with_path, false).is_ok());
    }

    /// L-02 (Validación 24).
    #[test]
    fn new_branch_names() {
        for bad in [
            "HEAD",
            "@",
            "a@{1}",
            "refs/heads/x",
            "-x",
            "a..b",
            "a/",
            "a.lock",
            "x\u{200b}y",
            "x\u{202e}y",
            "x\u{2061}y",
            "x\u{e0041}",
            "a b",
            "0123456789012345678901234567890123456789",
        ] {
            assert!(check_new_branch(bad).is_err(), "{bad:?}");
        }
        for good in ["feat/x", "fix-1", "user/feat.v2"] {
            assert!(check_new_branch(good).is_ok(), "{good:?}");
        }
        assert_eq!(sanitized_branch("feat/x").unwrap(), "feat-x");
        assert!(sanitized_branch(".").is_err());
    }

    #[test]
    fn commit_arguments() {
        let args = |v: Value| v.as_object().unwrap().clone();
        let ok = OperationArgs::parse(
            OperationId::Commit,
            &args(json!({ "message": "$(rm -rf ~) --amend", "paths": [":(glob)**"] })),
            true,
        )
        .unwrap();
        // The message enters the fingerprint only as length and digest.
        let view = ok.fingerprint_view().to_string();
        assert!(!view.contains("rm -rf"), "{view}");
        for bad in [
            json!({ "message": "" }),
            json!({ "message": "x\u{0}y" }),
            json!({ "message": "m", "paths": [] }),
            json!({ "message": "m", "paths": ["../etc/passwd"] }),
            json!({ "message": "m", "paths": ["/abs"] }),
            json!({ "message": "m", "paths": ["--amend"] }),
            json!({ "message": "m", "paths": ["a/.git/config"] }),
            json!({ "message": "m", "amend": true }),
        ] {
            assert!(
                OperationArgs::parse(OperationId::Commit, &args(bad.clone()), true).is_err(),
                "{bad}"
            );
        }
        // The message never shows in Debug output.
        assert!(!format!("{ok:?}").contains("rm -rf"));
    }

    #[test]
    fn snapshot_labels() {
        let args = |v: Value| v.as_object().unwrap().clone();
        assert!(
            OperationArgs::parse(
                OperationId::Snapshot,
                &args(json!({"label": "before x"})),
                true
            )
            .is_ok()
        );
        for bad in ["", "a\u{1b}[31m", &"x".repeat(65)] {
            assert!(
                OperationArgs::parse(OperationId::Snapshot, &args(json!({ "label": bad })), true)
                    .is_err()
            );
        }
    }

    #[test]
    fn a_label_over_256_bytes_is_refused_before_reading_chars() {
        // 65 characters of 4 bytes are over both limits; 64 of them are exactly 256 bytes
        // and pass the byte check, so the refusal below is the byte limit's alone.
        let wide = "\u{1f600}".repeat(64);
        assert_eq!(wide.len(), MAX_LABEL_BYTES);
        assert!(check_snapshot_label(&wide).is_ok());
        let over = format!("{wide}x");
        assert!(over.len() > MAX_LABEL_BYTES);
        let err = check_snapshot_label(&over).unwrap_err();
        assert_eq!(err.field, "label");
        // A huge string is refused by its length alone, even if every char is valid.
        assert!(check_snapshot_label(&"a".repeat(10 * 1024 * 1024)).is_err());
    }

    #[test]
    fn the_manual_snapshot_shapes_round_trip() {
        use crate::methods::{QuotaWindow, SnapshotQuotaData};
        let run = SnapshotRunResult {
            snapshot_id: "snap-1".into(),
            worktree: UntrustedName::new("app"),
            label: UntrustedName::new("antes de migrar"),
            requester: RequesterView {
                actor: crate::Actor::Unattributed,
                channel: crate::timemachine::RequestChannel::Mcp,
                via: crate::timemachine::ResolvedVia::Ancestry,
                confirmable: false,
            },
            layer: Layer::Mcp,
            outcome: OperationOutcome::Done,
        };
        let wire = serde_json::to_value(&run).unwrap();
        assert_eq!(wire["label"]["untrusted"], "antes de migrar");
        assert_eq!(
            serde_json::from_value::<SnapshotRunResult>(wire).unwrap(),
            run
        );

        let quota = SnapshotQuotaData {
            window: QuotaWindow::WorktreeDay,
            retry_after_s: Some(3_600),
            release_utc_ms: Some(1_700_000_000_000),
        };
        let wire = serde_json::to_value(&quota).unwrap();
        assert_eq!(wire["window"], "worktree-day");
        assert_eq!(
            serde_json::from_value::<SnapshotQuotaData>(wire).unwrap(),
            quota
        );
        let disk: SnapshotQuotaData = serde_json::from_value(json!({"window": "disk"})).unwrap();
        assert_eq!((disk.retry_after_s, disk.release_utc_ms), (None, None));
        assert!(
            serde_json::from_value::<SnapshotQuotaData>(json!({"window":"day","x":1})).is_err()
        );

        assert_eq!(
            serde_json::to_value(RejectReason::WriteInProgress).unwrap(),
            "write-in-progress"
        );
    }
}
