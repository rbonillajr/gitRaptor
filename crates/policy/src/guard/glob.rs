//! Patterns of the protected branches and the forbidden paths (DS-US-GRD-008 D6): pure and
//! bounded. `/` separates segments; `*` and `?` stay inside a segment; a whole segment `**`
//! crosses segments. There are no negations. A path pattern follows `.gitignore` reduced: one
//! with no `/` apart from a trailing one is looked for at any depth, one with a `/` in the
//! middle or at the start is anchored to the root, a trailing `/` means a directory only, and a
//! pattern that covers a directory covers everything below it. A branch pattern matches the
//! whole short name.
//!
//! Everything is compared normalized (NFC, lowercase, SEC-GRD-18), whatever the file system of
//! the repo: the file system of the remote is unknown and the content is portable.

use unicode_normalization::UnicodeNormalization;

/// Patterns one key may carry.
pub const MAX_PATTERNS: usize = 64;
/// Longest pattern, in bytes.
pub const MAX_PATTERN_BYTES: usize = 256;
/// Longest path or branch name matched, in bytes; a longer one cannot be verified.
pub const MAX_NAME_BYTES: usize = 4096;
/// Steps one evaluation may spend matching, across all its names and patterns.
pub const WORK_LIMIT: u64 = 50_000_000;

/// Why a pattern is not valid. The pattern alone is dropped; the others still apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Empty,
    TooLong,
    Control,
    /// Starts with `!` (negations do not exist) or `#` (neither do comments).
    Reserved,
    /// A branch pattern written as a ref (`refs/heads/…`).
    RefsPrefix,
    /// `a//b`.
    EmptySegment,
    /// Leading or trailing whitespace (never matches what it looks like), or a branch pattern
    /// ending in `/` (it would name no branch).
    Padded,
}

/// The work spent matching, bounded: past it nothing can be verified (fail-closed).
#[derive(Debug, Clone, Copy)]
pub struct Budget(u64);

impl Default for Budget {
    fn default() -> Self {
        Self(WORK_LIMIT)
    }
}

impl Budget {
    pub fn new(steps: u64) -> Self {
        Self(steps)
    }

    fn spend(&mut self, steps: usize) -> Result<(), Exceeded> {
        self.0 = self.0.checked_sub(steps as u64).ok_or(Exceeded)?;
        Ok(())
    }
}

/// The budget (or a size limit) was exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exceeded;

/// The form two names are compared in: NFC and lowercase, without what some file systems ignore
/// (the code points HFS+ skips) and without the trailing dots and spaces of a segment that NTFS
/// drops, so `Secrets./k` and `sec\u{200c}rets/k` are `secrets/k`.
pub fn fold(name: &str) -> String {
    let ignorable = |c: char| matches!(c, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}');
    let folded: String = name
        .nfc()
        .filter(|c| !ignorable(*c))
        .collect::<String>()
        .to_lowercase();
    folded
        .split('/')
        .map(|segment| {
            // Never empty a segment that is only dots: `..` stays itself.
            let trimmed = segment.trim_end_matches(['.', ' ']);
            if trimmed.is_empty() { segment } else { trimmed }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    /// `**`: any number of segments, also none.
    Any,
    Glob(Vec<char>),
}

/// A validated pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    raw: String,
    segs: Vec<Seg>,
    /// Looked for at any depth (paths without a `/` in the middle or at the start).
    floating: bool,
    dir_only: bool,
}

/// What a pattern is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Branch,
    Path,
}

/// The first reason a pattern is not valid, if any.
pub fn validate(raw: &str, kind: Kind) -> Result<(), Invalid> {
    if raw.is_empty() {
        return Err(Invalid::Empty);
    }
    if raw.len() > MAX_PATTERN_BYTES {
        return Err(Invalid::TooLong);
    }
    if raw.chars().any(char::is_control) {
        return Err(Invalid::Control);
    }
    if raw.starts_with(['!', '#']) {
        return Err(Invalid::Reserved);
    }
    if raw.starts_with(char::is_whitespace)
        || raw.ends_with(char::is_whitespace)
        || (kind == Kind::Branch && raw.ends_with('/'))
    {
        return Err(Invalid::Padded);
    }
    if kind == Kind::Branch && raw.starts_with("refs/heads/") {
        return Err(Invalid::RefsPrefix);
    }
    let body = raw.strip_prefix('/').unwrap_or(raw);
    let body = body.strip_suffix('/').unwrap_or(body);
    if body.is_empty() || body.split('/').any(str::is_empty) {
        return Err(Invalid::EmptySegment);
    }
    Ok(())
}

impl Pattern {
    pub fn new(raw: &str, kind: Kind) -> Result<Self, Invalid> {
        validate(raw, kind)?;
        let folded = fold(raw);
        let anchored_by_slash = folded.starts_with('/');
        let body = folded.strip_prefix('/').unwrap_or(&folded);
        let dir_only = kind == Kind::Path && body.ends_with('/');
        let body = body.strip_suffix('/').unwrap_or(body);
        let floating = kind == Kind::Path && !anchored_by_slash && !body.contains('/');
        let segs = body
            .split('/')
            .map(|s| {
                if s == "**" {
                    Seg::Any
                } else {
                    Seg::Glob(s.chars().collect())
                }
            })
            .collect();
        Ok(Self {
            raw: raw.to_owned(),
            segs,
            floating,
            dir_only,
        })
    }

    /// The pattern as written.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Whether the short name of a branch matches the whole pattern.
    pub fn matches_branch(&self, name: &str, budget: &mut Budget) -> Result<bool, Exceeded> {
        if name.len() > MAX_NAME_BYTES {
            return Err(Exceeded);
        }
        let folded = fold(name);
        let parts: Vec<Vec<char>> = folded.split('/').map(|s| s.chars().collect()).collect();
        let table = self.table(&parts, false, budget)?;
        Ok(table[self.segs.len()][parts.len()])
    }

    /// Whether the path, or a directory above it, matches (a directory-only pattern needs a
    /// directory above it).
    pub fn matches_path(&self, path: &str, budget: &mut Budget) -> Result<bool, Exceeded> {
        if path.len() > MAX_NAME_BYTES {
            return Err(Exceeded);
        }
        let folded = fold(path);
        // A path that ends in `/` is a directory (a submodule): a directory pattern covers it.
        let is_dir = folded.ends_with('/');
        let parts: Vec<Vec<char>> = folded
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().collect())
            .collect();
        if parts.is_empty() {
            return Ok(false);
        }
        let table = self.table(&parts, self.floating, budget)?;
        let last = self.segs.len() + usize::from(self.floating);
        let n = parts.len();
        let upto = if self.dir_only && !is_dir { n - 1 } else { n };
        Ok((1..=upto).any(|k| table[last][k]))
    }

    /// `table[i][j]`: the first `i` pattern segments match exactly the first `j` name segments
    /// (with a floating pattern, an implicit `**` in front).
    fn table(
        &self,
        parts: &[Vec<char>],
        floating: bool,
        budget: &mut Budget,
    ) -> Result<Vec<Vec<bool>>, Exceeded> {
        let mut segs: Vec<&Seg> = Vec::with_capacity(self.segs.len() + 1);
        if floating {
            segs.push(&Seg::Any);
        }
        segs.extend(self.segs.iter());
        let (p, n) = (segs.len(), parts.len());
        budget.spend((p + 1) * (n + 1))?;
        let mut t = vec![vec![false; n + 1]; p + 1];
        t[0][0] = true;
        for i in 1..=p {
            for j in 0..=n {
                t[i][j] = match segs[i - 1] {
                    // `**` takes none (same `j`) or one more segment.
                    Seg::Any => t[i - 1][j] || (j > 0 && t[i][j - 1]),
                    Seg::Glob(glob) => {
                        j > 0 && t[i - 1][j - 1] && segment(glob, &parts[j - 1], budget)?
                    }
                };
            }
        }
        Ok(t)
    }
}

/// `*` and `?` inside one segment, greedy with one backtracking point.
fn segment(pattern: &[char], text: &[char], budget: &mut Budget) -> Result<bool, Exceeded> {
    budget.spend(pattern.len() + text.len() + 1)?;
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        budget.spend(1)?;
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) && pattern[p] != '*' {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return Ok(false);
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    Ok(p == pattern.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(pattern: &str, p: &str) -> bool {
        Pattern::new(pattern, Kind::Path)
            .unwrap()
            .matches_path(p, &mut Budget::default())
            .unwrap()
    }

    fn branch(pattern: &str, name: &str) -> bool {
        Pattern::new(pattern, Kind::Branch)
            .unwrap()
            .matches_branch(name, &mut Budget::default())
            .unwrap()
    }

    #[test]
    fn branches_match_the_whole_name_and_stars_stay_in_a_segment() {
        assert!(branch("main", "main"));
        assert!(!branch("main", "main2"));
        assert!(!branch("main", "feat/main"));
        assert!(branch("release/*", "release/1.0"));
        assert!(!branch("release/*", "release/1.0/x"));
        assert!(!branch("release/*", "release"));
        assert!(branch("release/**", "release/1.0/x"));
        assert!(branch("release/**", "release"));
        assert!(branch("rel*", "release"));
        assert!(branch("v?.0", "v1.0"));
        assert!(!branch("v?.0", "v10.0"));
        assert!(branch("**/prod", "a/b/prod"));
        assert!(branch("**/prod", "prod"));
    }

    #[test]
    fn a_path_pattern_without_a_slash_is_found_at_any_depth() {
        assert!(path("*.pem", "key.pem"));
        assert!(path("*.pem", "a/b/key.pem"));
        assert!(!path("*.pem", "key.pem.txt"));
        assert!(path(".env", "services/api/.env"));
        // A directory name covers what is below it.
        assert!(path("secrets", "a/secrets/x.txt"));
        // …and so does a trailing slash, which needs a directory.
        assert!(path("secrets/", "secrets/api.txt"));
        assert!(path("secrets/", "src/secrets/deep/api.txt"));
        assert!(!path("secrets/", "secrets"));
        assert!(!path("secrets/", "secrets.txt"));
    }

    #[test]
    fn a_path_pattern_with_a_slash_is_anchored_to_the_root() {
        assert!(path("config/prod.yml", "config/prod.yml"));
        assert!(!path("config/prod.yml", "a/config/prod.yml"));
        assert!(path("/secrets", "secrets/x"));
        assert!(!path("/secrets", "a/secrets/x"));
        assert!(path(".github/workflows/", ".github/workflows/ci.yml"));
        assert!(!path(".github/workflows/", "a/.github/workflows/ci.yml"));
        assert!(path("**/.github/workflows/", "a/.github/workflows/ci.yml"));
        assert!(path("docs/**/draft.md", "docs/a/b/draft.md"));
        assert!(path("docs/**/draft.md", "docs/draft.md"));
        assert!(!path("docs/**/draft.md", "x/docs/draft.md"));
    }

    #[test]
    fn names_are_compared_folded_and_normalized() {
        assert!(path("Secrets/", "secrets/api.txt"));
        assert!(path("secrets/", "SECRETS/api.txt"));
        // NFD spelling of an NFC pattern.
        assert!(path("caf\u{e9}/", "cafe\u{301}/menu.txt"));
        assert!(branch("Main", "main"));
        assert!(branch("main", "MAIN"));
    }

    #[test]
    fn what_some_file_systems_ignore_does_not_hide_a_path() {
        // NTFS drops trailing dots and spaces; HFS+ skips some code points.
        assert!(path("secrets/", "secrets./key"));
        assert!(path("secrets/", "secrets /key"));
        assert!(path("secrets/", "sec\u{200c}rets/key"));
        assert!(path("*.pem", "key.pem."));
        // A segment of dots is not emptied.
        assert!(!path("secrets/", "../key"));
    }

    #[test]
    fn a_submodule_path_ends_in_a_slash_and_a_directory_pattern_covers_it() {
        assert!(path("vendor/keys/", "vendor/keys/"));
        assert!(path("vendor/keys", "vendor/keys/"));
        assert!(path("keys/", "vendor/keys/"));
        // A plain file at the same path is not a directory.
        assert!(!path("vendor/keys/", "vendor/keys"));
    }

    #[test]
    fn invalid_patterns_are_named() {
        let bad = |raw: &str, kind| validate(raw, kind).unwrap_err();
        assert_eq!(bad("", Kind::Path), Invalid::Empty);
        assert_eq!(bad(&"a".repeat(257), Kind::Path), Invalid::TooLong);
        assert_eq!(bad("a\nb", Kind::Path), Invalid::Control);
        assert_eq!(bad("a\u{1b}[0m", Kind::Branch), Invalid::Control);
        assert_eq!(bad("!secrets/", Kind::Path), Invalid::Reserved);
        assert_eq!(bad("#secrets", Kind::Path), Invalid::Reserved);
        assert_eq!(bad("refs/heads/main", Kind::Branch), Invalid::RefsPrefix);
        assert_eq!(bad("a//b", Kind::Path), Invalid::EmptySegment);
        assert_eq!(bad("/", Kind::Path), Invalid::EmptySegment);
        assert_eq!(bad(" secrets/", Kind::Path), Invalid::Padded);
        assert_eq!(bad("secrets/ ", Kind::Path), Invalid::Padded);
        assert_eq!(bad("release/", Kind::Branch), Invalid::Padded);
        // A trailing slash is how a path pattern names a directory.
        assert!(validate("release/", Kind::Path).is_ok());
        // Brackets and backslashes are literals.
        assert!(validate("[a]\\b", Kind::Path).is_ok());
        assert!(path("[a]", "[a]/x"));
        assert!(!path("[a]", "a/x"));
        // A path called like a ref is fine.
        assert!(validate("refs/heads/main", Kind::Path).is_ok());
    }

    #[test]
    fn work_is_bounded_and_running_out_is_an_error() {
        let p = Pattern::new("**/**/**/a*a*a*b", Kind::Path).unwrap();
        let long = format!("{}/x", "a".repeat(3_000));
        // Within the limit it answers; with a tiny budget it never guesses.
        assert!(p.matches_path(&long, &mut Budget::default()).is_ok());
        assert_eq!(p.matches_path(&long, &mut Budget::new(100)), Err(Exceeded));
        let huge = "a/".repeat(MAX_NAME_BYTES);
        assert_eq!(p.matches_path(&huge, &mut Budget::default()), Err(Exceeded));
    }
}
