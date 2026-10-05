//! Governed refs and name normalization (ADR-GRD-002 § 4, M-01, H-06).
//!
//! The refs that are **not** governed are an explicit list; any other ref is
//! governed, so a new namespace never escapes through the fast path.

use unicode_normalization::UnicodeNormalization;

pub use super::fastpath::{is_governed, is_head};

/// The form two names are compared in (SEC-GRD-18): NFC always and, when
/// the repo's file system does not distinguish case, folded to lowercase.
pub fn normalize(name: &str, fold_case: bool) -> String {
    let nfc: String = name.nfc().collect();
    if fold_case { nfc.to_lowercase() } else { nfc }
}

/// The full ref of a branch.
pub fn branch_ref(short: &str) -> String {
    format!("refs/heads/{short}")
}

/// How a ref relates to a protected branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    None,
    /// The same bytes.
    Exact,
    /// The same after normalization but not byte-identical: ambiguous.
    Alias,
}

/// Compares `refname` with the branch `base` (short name).
pub fn matches_branch(refname: &str, base: &str, fold_case: bool) -> Match {
    let full = branch_ref(base);
    if refname == full {
        Match::Exact
    } else if normalize(refname, fold_case) == normalize(&full, fold_case) {
        Match::Alias
    } else {
        Match::None
    }
}

/// The short name of a branch ref, or the ref itself.
pub fn short(refname: &str) -> &str {
    refname.strip_prefix("refs/heads/").unwrap_or(refname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_list_and_everything_else_governed() {
        for name in [
            "refs/remotes/origin/main",
            "refs/tags/v1",
            "refs/notes/commits",
            "refs/stash",
            "refs/bisect/bad",
            "refs/rewritten/x",
            "refs/prefetch/remotes/origin/main",
            "ORIG_HEAD",
            "FETCH_HEAD",
            "AUTO_MERGE",
            "CHERRY_PICK_HEAD",
            "REBASE_HEAD",
            "MERGE_HEAD",
            "REVERT_HEAD",
            "BISECT_HEAD",
            "main-worktree/ORIG_HEAD",
            "worktrees/wt/REBASE_HEAD",
            "HEAD",
            "worktrees/wt/HEAD",
        ] {
            assert!(!is_governed(name), "{name}");
        }
        for name in [
            "refs/heads/main",
            "refs/heads/feat",
            "refs/for/main",
            "refs/new-namespace/x",
            "refs/stashed",
            "Head",
            "refs/heads/HEAD",
        ] {
            assert!(is_governed(name), "{name}");
        }
    }

    #[test]
    fn head_forms() {
        assert!(is_head("HEAD") && is_head("main-worktree/HEAD") && is_head("worktrees/a/HEAD"));
        assert!(!is_head("worktrees//HEAD") && !is_head("worktrees/a/b/HEAD"));
        assert!(!is_head("refs/heads/HEAD"));
    }

    #[test]
    fn aliases_of_the_base_branch() {
        assert_eq!(
            matches_branch("refs/heads/main", "main", true),
            Match::Exact
        );
        assert_eq!(
            matches_branch("refs/heads/Main", "main", true),
            Match::Alias
        );
        assert_eq!(
            matches_branch("refs/heads/Main", "main", false),
            Match::None
        );
        // NFD spelling of an NFC base: an alias even on a case-sensitive file system (D19b).
        let nfc = "caf\u{e9}";
        let nfd = "refs/heads/cafe\u{301}";
        assert_eq!(matches_branch(nfd, nfc, false), Match::Alias);
        assert_eq!(matches_branch("refs/heads/feat", "main", true), Match::None);
    }
}
