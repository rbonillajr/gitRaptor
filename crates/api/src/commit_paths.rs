//! The lexical form of the paths of a `commit` (BR-MCP-VAL-001, ADR-MCP-001 § 4.2 and § 9).
//!
//! One check shared by `raptor-mcp` and [`crate::catalog::OperationArgs::parse`]: it looks at
//! the text only, never at the disk. The daemon still checks every path against the worktree
//! (links, nested repos, ignored files); this module only refuses what can never be a name of
//! the worktree, the same way on every platform where it can.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::{MAX_COMMIT_PATHS, MAX_PATH_BYTES, is_forbidden_char};

/// Why the lexical form of a commit path is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PathRefusal {
    /// Absolute, UNC, drive, backslash, control characters, leading `-`, or not canonical.
    Invalid,
    /// `..`, `.git` or one of its aliases.
    Outside,
    /// More than [`MAX_COMMIT_PATHS`] paths.
    TooMany,
}

impl PathRefusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Outside => "outside",
            Self::TooMany => "too-many",
        }
    }
}

/// Code points HFS+ ignores when comparing names, copied from
/// `crates/git/src/tm_write/tree_path.rs:HFS_IGNORABLE` (this crate does not depend on
/// `gitraptor-git`). U+206A to U+206F are not forbidden characters, so without them
/// `.g\u{206A}it` would open `.git` on macOS (CVE-2014-9390).
const HFS_IGNORABLE: &[char] = &[
    '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
    '\u{202E}', '\u{206A}', '\u{206B}', '\u{206C}', '\u{206D}', '\u{206E}', '\u{206F}', '\u{FEFF}',
];

/// Checks one path relative to the worktree root, with `/` separators. The first rule that
/// fails decides. Pathspec magic (`:(glob)**`, `:!x`) is a literal name here: Git receives every
/// path as `:(literal)`.
pub fn check_commit_path(path: &str) -> Result<(), PathRefusal> {
    // Bytes before characters: a long path is refused without walking it.
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err(PathRefusal::Invalid);
    }
    if path.chars().any(is_forbidden_char) {
        return Err(PathRefusal::Invalid);
    }
    // `\` and `C:` are legal names on Unix, refused everywhere so the same request has the
    // same answer on every platform.
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.starts_with('/') || path.contains('\\') || drive || path.starts_with('-') {
        return Err(PathRefusal::Invalid);
    }
    for component in path.split('/') {
        match component {
            "" | "." => return Err(PathRefusal::Invalid),
            ".." => return Err(PathRefusal::Outside),
            c if is_dotgit_alias(c) => return Err(PathRefusal::Outside),
            _ => {}
        }
    }
    if cfg!(windows) && !windows_form_ok(path) {
        return Err(PathRefusal::Invalid);
    }
    Ok(())
}

/// Too many paths before looking at any; otherwise the first refusal, with its index.
pub fn check_commit_paths(paths: &[String]) -> Result<(), (usize, PathRefusal)> {
    if paths.len() > MAX_COMMIT_PATHS {
        return Err((0, PathRefusal::TooMany));
    }
    paths
        .iter()
        .enumerate()
        .try_for_each(|(i, p)| check_commit_path(p).map_err(|r| (i, r)))
}

/// `.git` on some file system: any case, NTFS trailing dots and spaces, the 8.3 name `GIT~1`, a
/// stream suffix (`.git::$INDEX_ALLOCATION`) and the code points HFS+ ignores.
pub fn is_dotgit_alias(component: &str) -> bool {
    matches!(folded(component).as_str(), ".git" | "git~1")
}

/// `.gitmodules`, `.gitattributes`, `.gitignore` and their 8.3 names, with the folding of
/// [`is_dotgit_alias`].
pub fn is_git_special_name(component: &str) -> bool {
    matches!(
        folded(component).as_str(),
        ".gitmodules" | "gitmod~1" | ".gitattributes" | "gitatt~1" | ".gitignore" | "gitign~1"
    )
}

/// The name a file system that ignores HFS+ code points, case, streams and trailing dots and
/// spaces would see.
fn folded(component: &str) -> String {
    let text: String = component
        .chars()
        .filter(|c| !HFS_IGNORABLE.contains(c))
        .collect::<String>()
        .to_lowercase();
    let base = text.split(':').next().unwrap_or("");
    base.trim_end_matches(['.', ' ']).to_owned()
}

/// Windows: no `:` anywhere, no component ending in `.` or space, no reserved device name.
fn windows_form_ok(path: &str) -> bool {
    const RESERVED: &[&str] = &["con", "prn", "aux", "nul"];
    if path.contains(':') {
        return false;
    }
    path.split('/').all(|c| {
        if c.ends_with(['.', ' ']) {
            return false;
        }
        let stem = c.split('.').next().unwrap_or("").to_ascii_lowercase();
        let stem = stem.trim_end_matches(' ');
        let device = |prefix: &str| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        };
        !(RESERVED.contains(&stem) || device("com") || device("lpt"))
    })
}
