//! Paths of a tree to restore, checked before the file system is touched (SEC-TMC-04,
//! SEC-TMC-09). The store is untrusted input: a path that could leave the worktree, land in a
//! `.git` folder or collide with another on the target file system rejects the **whole** tree.
//!
//! These checks never replace the root-relative opens of [`super::files`]; they are the first of
//! two barriers.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use super::files::Folding;

/// Why a tree was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRejection {
    /// A path that is empty, absolute, has `.`, `..`, NUL, or names `.git` in any spelling.
    Hostile { path: Vec<u8>, reason: &'static str },
    /// Two paths that a case-insensitive or normalizing file system would make one
    /// (CVE-2021-21300 class).
    Collision { first: Vec<u8>, second: Vec<u8> },
}

impl std::fmt::Display for PathRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hostile { path, reason } => {
                write!(
                    f,
                    "hostile path {:?}: {reason}",
                    String::from_utf8_lossy(path)
                )
            }
            Self::Collision { first, second } => write!(
                f,
                "paths {:?} and {:?} collide on the target file system",
                String::from_utf8_lossy(first),
                String::from_utf8_lossy(second)
            ),
        }
    }
}

/// Code points HFS+ ignores when comparing names (Git's `is_hfs_dotgit`).
const HFS_IGNORABLE: &[char] = &[
    '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
    '\u{202E}', '\u{206A}', '\u{206B}', '\u{206C}', '\u{206D}', '\u{206E}', '\u{206F}', '\u{FEFF}',
];

/// Checks one worktree-relative path with `/` separators.
pub fn check(path: &[u8]) -> Result<(), PathRejection> {
    let hostile = |reason| PathRejection::Hostile {
        path: path.to_vec(),
        reason,
    };
    if path.is_empty() {
        return Err(hostile("empty"));
    }
    if path.contains(&0) {
        return Err(hostile("contains NUL"));
    }
    if path.starts_with(b"/") {
        return Err(hostile("absolute"));
    }
    if cfg!(windows) && (path.contains(&b'\\') || path.contains(&b':')) {
        return Err(hostile("backslash or drive/stream separator"));
    }
    for component in path.split(|b| *b == b'/') {
        match component {
            b"" => return Err(hostile("empty component")),
            b"." | b".." => return Err(hostile("dot component")),
            c if is_dotgit(c) => return Err(hostile("names .git")),
            _ => {}
        }
    }
    Ok(())
}

/// Whether a component names `.git` on some file system: any case, trailing dots and spaces
/// (NTFS), the 8.3 short name `GIT~1`, a stream suffix (`.git::$INDEX_ALLOCATION`) and the code
/// points HFS+ ignores.
pub fn is_dotgit(component: &[u8]) -> bool {
    let text: String = String::from_utf8_lossy(component)
        .chars()
        .filter(|c| !HFS_IGNORABLE.contains(c))
        .collect::<String>()
        .to_lowercase();
    let base = text.split(':').next().unwrap_or("");
    let trimmed = base.trim_end_matches(['.', ' ']);
    trimmed == ".git" || trimmed == "git~1"
}

/// The key two paths share when the target file system would treat them as one: lowercased
/// when it ignores case, in NFC when it normalizes.
fn fold(path: &[u8], folding: Folding) -> Vec<u8> {
    if !folding.case_insensitive && !folding.normalizing {
        return path.to_vec();
    }
    let mut text: String = String::from_utf8_lossy(path).into_owned();
    if folding.normalizing {
        text = text.nfc().collect();
    }
    if folding.case_insensitive {
        text = text.to_lowercase();
        if folding.normalizing {
            text = text.nfc().collect();
        }
    }
    text.into_bytes()
}

/// Checks every path of a tree and that no two of them collide on a file system that compares
/// names with `folding` (probed at the worktree root, [`super::files::RootDir::probe_folding`]).
pub fn check_tree<'a>(
    paths: impl IntoIterator<Item = &'a [u8]>,
    folding: Folding,
) -> Result<(), PathRejection> {
    let mut seen: HashMap<Vec<u8>, &'a [u8]> = HashMap::new();
    for path in paths {
        check(path)?;
        // Every prefix folder too: `A/x` and `a/y` collide on the folder.
        let mut end = 0;
        loop {
            let next = path[end..].iter().position(|b| *b == b'/').map(|i| end + i);
            let prefix = &path[..next.unwrap_or(path.len())];
            let key = fold(prefix, folding);
            match seen.get(&key) {
                Some(other) if *other != prefix => {
                    return Err(PathRejection::Collision {
                        first: other.to_vec(),
                        second: prefix.to_vec(),
                    });
                }
                Some(_) => {}
                None => {
                    seen.insert(key, prefix);
                }
            }
            match next {
                Some(i) => end = i + 1,
                None => break,
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_regular_paths() {
        for p in [
            "a.txt",
            "dir/b.txt",
            ".gitignore",
            ".github/workflows/ci.yml",
            "x/.gitmodules",
            "git",
            "ñandú/ü.txt",
        ] {
            assert!(check(p.as_bytes()).is_ok(), "{p}");
        }
    }

    #[test]
    fn rejects_the_hostile_corpus() {
        for p in [
            "",
            "/etc/passwd",
            "../outside",
            "a/../../outside",
            "a/./b",
            "a//b",
            "a/",
            ".git/config",
            ".GIT/hooks/pre-commit",
            "x/.Git/config",
            "GIT~1/config",
            "git~1",
            ".git./config",
            ".git /config",
            ".git::$INDEX_ALLOCATION/config",
            "\u{200C}.git/config",
            ".g\u{200D}it/config",
            "a\0b",
        ] {
            assert!(check(p.as_bytes()).is_err(), "{p:?}");
        }
    }

    #[test]
    fn rejects_case_and_normalization_collisions() {
        let folding = Folding {
            case_insensitive: true,
            normalizing: true,
        };
        let tree = |paths: &[&str]| {
            check_tree(paths.iter().map(|p| p.as_bytes()), folding).map_err(|e| e.to_string())
        };
        assert!(tree(&["a.txt", "b.txt", "dir/a.txt"]).is_ok());
        assert!(tree(&["README", "readme"]).is_err());
        assert!(tree(&["Dir/x", "dir/y"]).is_err());
        // "é" precomposed (NFC) and decomposed (NFD).
        assert!(tree(&["caf\u{e9}", "cafe\u{301}"]).is_err());
        assert!(tree(&["dir/x", "dir/y"]).is_ok());
        // A case-sensitive file system keeps both (ext4).
        let strict = check_tree(
            ["README", "readme"].iter().map(|p| p.as_bytes()),
            Folding::default(),
        );
        assert!(strict.is_ok());
    }
}
