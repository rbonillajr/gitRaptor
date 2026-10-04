//! Guard (INF-GRP-001, Guarda): the harness only touches temporary roots that the testkit
//! created and marked as its own, and refuses outright any path inside the GitRaptor repository
//! or covering the real home directory (NFR-01).

use std::fmt;
use std::path::{Path, PathBuf};

/// File that marks a directory as a testkit root.
pub const MARKER: &str = ".gitraptor-testkit-root";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// The path is inside the GitRaptor repository (this checkout or its main worktree).
    InsideGitRaptorRepo(PathBuf),
    /// The path is `/`, the real home directory or one of its ancestors.
    CoversHome(PathBuf),
    /// The path is not inside a directory created and marked by the testkit.
    NotATestkitRoot(PathBuf),
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsideGitRaptorRepo(p) => {
                write!(
                    f,
                    "refusing {}: inside the GitRaptor repository",
                    p.display()
                )
            }
            Self::CoversHome(p) => write!(f, "refusing {}: covers the home directory", p.display()),
            Self::NotATestkitRoot(p) => {
                write!(f, "refusing {}: not inside a testkit root", p.display())
            }
        }
    }
}

impl std::error::Error for GuardError {}

/// Positive check: `path` is inside a marked testkit root, and not forbidden.
pub fn check(path: &Path) -> Result<(), GuardError> {
    check_not_forbidden(path)?;
    let canonical = canonical(path);
    if canonical.ancestors().any(|dir| dir.join(MARKER).is_file()) {
        Ok(())
    } else {
        Err(GuardError::NotATestkitRoot(path.to_owned()))
    }
}

/// Negative check only: not inside the GitRaptor repository, not `/`, home or an ancestor.
pub fn check_not_forbidden(path: &Path) -> Result<(), GuardError> {
    let canonical = canonical(path);
    for repo in gitraptor_roots() {
        if canonical.starts_with(&repo) {
            return Err(GuardError::InsideGitRaptorRepo(path.to_owned()));
        }
    }
    if canonical.parent().is_none() {
        return Err(GuardError::CoversHome(path.to_owned()));
    }
    if let Some(home) = real_home()
        && home.starts_with(&canonical)
    {
        return Err(GuardError::CoversHome(path.to_owned()));
    }
    Ok(())
}

/// Mark `dir` as a testkit root. Only the testkit calls this, on directories it just created.
pub(crate) fn mark(dir: &Path) {
    check_not_forbidden(dir).unwrap_or_else(|e| panic!("harness guard: {e}"));
    std::fs::write(dir.join(MARKER), "").expect("mark testkit root");
}

/// The checkout this crate was built from and, when it is a linked worktree, the main
/// worktree of the same repository.
pub fn gitraptor_roots() -> Vec<PathBuf> {
    let workspace = canonical(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let mut roots = vec![workspace.clone()];
    // A linked worktree has a `.git` file: `gitdir: <main>/.git/worktrees/<name>`.
    if let Ok(text) = std::fs::read_to_string(workspace.join(".git"))
        && let Some(gitdir) = text.trim().strip_prefix("gitdir:")
    {
        let gitdir = canonical(Path::new(gitdir.trim()));
        if let Some(main) = gitdir
            .ancestors()
            .find(|p| p.file_name().is_some_and(|n| n == ".git"))
            .and_then(Path::parent)
        {
            roots.push(main.to_owned());
        }
    }
    roots
}

fn real_home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|h| !h.is_empty())
        .map(|h| canonical(Path::new(&h)))
}

/// Canonical form of `path`, or of its closest existing ancestor joined with the rest.
fn canonical(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => canonical(parent).join(name),
        _ => path.to_owned(),
    }
}
