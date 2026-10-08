//! Diagnostics of the loader. They never carry content of the document or values read from it
//! (SEC-11): only a code, the source and a position or a JSON pointer.

/// Which source a diagnostic or a state refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceKind {
    /// Profile `settings.json` (US-GRP-013).
    Profile,
    /// Local `settings.local.json` (ADR-GRP-008).
    Local,
    /// Team level from the copy of the main branch: the floor (ADR-GRD-004 § 2).
    Floor,
    /// Team level from the confirmed floor, kept by the developer's confirmation (§ 4).
    ConfirmedFloor,
    /// Team level from the commit of the operation's worktree `HEAD`: only hardens.
    Worktree,
    /// The team level as a whole (main branch, base branch, confirmations).
    Team,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Local => "local",
            Self::Floor => "floor",
            Self::ConfirmedFloor => "confirmed-floor",
            Self::Worktree => "worktree",
            Self::Team => "team",
        }
    }
}

/// Which JSON limit was exceeded (L-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Size,
    Depth,
    Members,
    StringLength,
}

/// What a diagnostic says. The client translates it (i18n en/es).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Not valid JSON (position given). The source is ignored (PQ-8).
    InvalidJson,
    /// The document starts with a byte order mark. Ignored.
    ByteOrderMark,
    /// An object repeats a key. Ignored.
    DuplicateKey,
    /// A JSON limit was exceeded. Ignored.
    LimitExceeded(Limit),
    /// The committed entry is a symbolic link. Ignored.
    Symlink,
    /// The committed entry is a submodule. Ignored.
    Submodule,
    /// The committed entry is not a file, or a parent is not a directory. Ignored.
    NotAFile,
    /// A value has the wrong type (pointer given). Ignored (PQ-8).
    WrongType,
    /// The committed entry or object could not be read (missing object, broken ref). Ignored,
    /// never read as "no rules" (SEC-GRD-17).
    Unreadable,
    /// A number is out of its range (pointer given). Ignored (PQ-8).
    OutOfRange,
    /// A key the schema does not know. Only that key is ignored.
    UnknownKey,
    /// Discovery roots declared in a settings file (`discovery`, `codeRoots`, `roots`, at the
    /// top or in `engine`). Ignored: roots are profile state that only `raptor repo roots add`
    /// changes, because an agent can write a settings file (BR-AUTH-003, ADR-GRP-010 N7).
    DiscoveryRootsIgnored,
    /// An operation outside the catalog in `permissions`. The source is partial (D12).
    UnknownOperation,
    /// A policy this version does not apply yet. The source is partial (D12).
    PolicyNotSupported,
    /// A key in a level that does not admit it (Q24). Only that key is ignored.
    KeyNotAllowedAtLevel,
    /// A level other than the floor relaxes a policy below its default (`flexible`, Q-GRD-20,
    /// US-GRD-018). Ignored.
    RelaxationNotAllowed,
    /// A key that only applies with another value of its section (`onAgentCommit` without
    /// `human-author`). Ignored.
    KeyOutOfPlace,
    /// A pattern of `protectedBranches` or `forbiddenPaths` that is not valid (empty, control
    /// bytes, `!` or `#` first, `refs/heads/…`, too long). Only that pattern is dropped; the
    /// source is partial (US-GRD-008, D1).
    PolicyInvalid,
    /// A key that only the floor can set (base branch, safe minimum) in the worktree. No effect.
    FloorOnlyKey,
    /// `engine.baseBranch` is not a valid branch name (SEC-11). Never passed to Git.
    InvalidBaseBranch,
    /// No base branch was confirmed yet (D9).
    BaseUnconfirmed,
    /// The resolved base branch differs from the confirmed one (D6).
    BaseChangePending { invalid: bool },
    /// The floor relaxes something against the confirmed floor (D7).
    FloorRelaxPending,
    /// The confirmed floor blob is not readable any more (for example, after `gc`).
    ConfirmedFloorMissing,
    /// A `*.json` file in the committed `.gitraptor/` that is not `settings.json` (file name
    /// given): likely a misnamed settings file. Informational: it is never read.
    UnknownSettingsFile,
}

impl Code {
    /// Stable code for clients.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid-json",
            Self::ByteOrderMark => "byte-order-mark",
            Self::DuplicateKey => "duplicate-key",
            Self::LimitExceeded(Limit::Size) => "limit-size",
            Self::LimitExceeded(Limit::Depth) => "limit-depth",
            Self::LimitExceeded(Limit::Members) => "limit-members",
            Self::LimitExceeded(Limit::StringLength) => "limit-string-length",
            Self::Symlink => "not-regular-symlink",
            Self::Submodule => "not-regular-submodule",
            Self::NotAFile => "not-regular-file",
            Self::Unreadable => "unreadable",
            Self::WrongType => "wrong-type",
            Self::OutOfRange => "out-of-range",
            Self::UnknownKey => "unknown-key",
            Self::DiscoveryRootsIgnored => "discovery-roots-ignored",
            Self::UnknownOperation => "unknown-operation",
            Self::PolicyNotSupported => "policy-not-supported",
            Self::KeyNotAllowedAtLevel => "key-not-allowed-at-level",
            Self::RelaxationNotAllowed => "relaxation-not-allowed",
            Self::KeyOutOfPlace => "key-out-of-place",
            Self::PolicyInvalid => "policy-invalid",
            Self::FloorOnlyKey => "floor-only-key",
            Self::InvalidBaseBranch => "invalid-base-branch",
            Self::BaseUnconfirmed => "base-unconfirmed",
            Self::BaseChangePending { invalid: false } => "base-change-pending",
            Self::BaseChangePending { invalid: true } => "base-change-pending-invalid",
            Self::FloorRelaxPending => "floor-relax-pending",
            Self::ConfirmedFloorMissing => "confirmed-floor-missing",
            Self::UnknownSettingsFile => "unknown-settings-file",
        }
    }
}

/// Where in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// 1-based line and column of a syntax error.
    Position { line: usize, column: usize },
    /// JSON pointer (RFC 6901) of the key. Segments are truncated and neutralized.
    Pointer(String),
    /// Name of a committed file, truncated and neutralized like a pointer segment.
    File(String),
}

/// One diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub source: SourceKind,
    pub location: Option<Location>,
}

impl Diagnostic {
    pub fn new(code: Code, source: SourceKind) -> Self {
        Self {
            code,
            source,
            location: None,
        }
    }

    pub(crate) fn at(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    pub(crate) fn with_source(mut self, source: SourceKind) -> Self {
        self.source = source;
        self
    }
}

/// Longest pointer segment kept, in characters.
const SEGMENT_MAX: usize = 64;

/// A JSON pointer built from keys of a document that may be hostile: each segment is escaped
/// (RFC 6901), cut to 64 characters and stripped of control and format characters, so a key
/// cannot smuggle text into a client (SEC-11, SEC-GRD-06).
pub(crate) fn pointer(segments: &[String]) -> String {
    let mut out = String::new();
    for segment in segments {
        out.push('/');
        for (i, c) in segment.chars().enumerate() {
            if i == SEGMENT_MAX {
                out.push('…');
                break;
            }
            match c {
                '~' => out.push_str("~0"),
                '/' => out.push_str("~1"),
                c if is_neutralized(c) => out.push('\u{FFFD}'),
                c => out.push(c),
            }
        }
    }
    out
}

/// A committed file name made safe to show: cut to 64 characters, control and format
/// characters replaced (SEC-11).
pub(crate) fn file_name(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i == SEGMENT_MAX {
            out.push('…');
            break;
        }
        out.push(if is_neutralized(c) { '\u{FFFD}' } else { c });
    }
    out
}

/// Unicode Cc, Cf, Zl and Zp (bidi controls, zero-width characters, separators).
fn is_neutralized(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_escapes_truncates_and_neutralizes() {
        assert_eq!(pointer(&["a/b".into(), "c~d".into()]), "/a~1b/c~0d");
        assert_eq!(pointer(&["x\u{202E}y\n".into()]), "/x\u{FFFD}y\u{FFFD}");
        let long = "k".repeat(100);
        let p = pointer(&[long]);
        assert_eq!(p.chars().count(), 1 + SEGMENT_MAX + 1);
    }
}
