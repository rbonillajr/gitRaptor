//! Read-only detection of who created a linked worktree (US-WTL-005, BR-WTL-011).
//!
//! The only file system calls are `symlink_metadata` on two entries of the
//! worktree's administration directory (`<common>/worktrees/<id>/orca` and
//! `.../locked`). Nothing is opened or read inside a worktree root, and
//! nothing is written, moved or removed anywhere. No function fails: what
//! cannot be checked falls to the next rule and, at the end, to `unknown`.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use gitraptor_api::messages::{HeadView, WorktreeStatus, WorktreeView};
use gitraptor_api::worktree_origin::{
    HandsOffReasonView, OriginEvidenceView, OriginToolView, WorktreeOriginView,
};

use crate::observe::{canonical, home_folders};

/// The environment the path rules read. Tests build it by hand.
#[derive(Debug, Clone, Default)]
pub struct OriginEnv {
    /// `observe::home_folders()`: HOME, and USERPROFILE on Windows.
    pub homes: Vec<PathBuf>,
    /// `CODEX_HOME` when set; otherwise `<home>/.codex` is tried.
    pub codex_home: Option<PathBuf>,
}

impl OriginEnv {
    /// The environment of this process.
    pub fn from_process() -> Self {
        Self {
            homes: home_folders(),
            codex_home: std::env::var_os("CODEX_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
        }
    }
}

/// The administration directory of `admin_name`, only when the name is one
/// plain component (never a path that could leave `<common>/worktrees`).
fn admin_dir(common_dir: &Path, admin_name: Option<&str>) -> Option<PathBuf> {
    let id = admin_name?;
    let mut parts = Path::new(id).components();
    let one_name = matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none();
    one_name.then(|| common_dir.join("worktrees").join(id))
}

/// A real directory (not a link, not a file) at `path`.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// The main worktree's root: the parent of a common directory called `.git`.
/// A bare repo has none.
fn main_root_of(common_dir: &Path) -> Option<PathBuf> {
    let common = canonical(common_dir);
    if common.file_name() == Some(OsStr::new(".git")) {
        common.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

fn is_worktrunk_sibling(root: &Path, main_root: &Path) -> bool {
    let (Some(parent), Some(main_parent)) = (root.parent(), main_root.parent()) else {
        return false;
    };
    let (Some(name), Some(main_name)) = (
        root.file_name().and_then(OsStr::to_str),
        main_root.file_name().and_then(OsStr::to_str),
    ) else {
        return false;
    };
    parent == main_parent
        && name
            .strip_prefix(main_name)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|suffix| !suffix.is_empty())
}

/// Who created the worktree and how that is known, by the first rule that
/// matches: GitRaptor's record, Orca's mark, then the default locations of
/// Claude Code, Codex, Orca and worktrunk, then `unknown`.
pub fn detect(
    common_dir: &Path,
    root: &Path,
    admin_name: Option<&str>,
    registered_by_gitraptor: bool,
    env: &OriginEnv,
) -> (OriginToolView, OriginEvidenceView) {
    use OriginEvidenceView as Ev;
    use OriginToolView as Tool;
    if registered_by_gitraptor {
        return (Tool::Gitraptor, Ev::Registered);
    }
    // An `orca` link would let an agent forge a certain origin (SEC-11).
    if admin_dir(common_dir, admin_name).is_some_and(|d| is_real_dir(&d.join("orca"))) {
        return (Tool::Orca, Ev::AdminMarker);
    }
    let root = canonical(root);
    let main_root = main_root_of(common_dir);
    if main_root
        .as_ref()
        .is_some_and(|m| root.starts_with(m.join(".claude").join("worktrees")))
    {
        return (Tool::ClaudeCode, Ev::PathPattern);
    }
    let codex_bases: Vec<PathBuf> = match &env.codex_home {
        Some(c) => vec![canonical(c)],
        None => env
            .homes
            .iter()
            .map(|h| canonical(h).join(".codex"))
            .collect(),
    };
    if codex_bases
        .iter()
        .any(|b| root.starts_with(b.join("worktrees")))
    {
        return (Tool::Codex, Ev::PathPattern);
    }
    if env
        .homes
        .iter()
        .any(|h| root.starts_with(canonical(h).join("orca").join("workspaces")))
    {
        return (Tool::Orca, Ev::PathPattern);
    }
    if main_root
        .as_ref()
        .is_some_and(|m| is_worktrunk_sibling(&root, m))
    {
        return (Tool::Worktrunk, Ev::PathPattern);
    }
    (Tool::Unknown, Ev::None)
}

/// The tool locked the worktree: `<common>/worktrees/<id>/locked` exists.
/// Any entry counts (a link too): the check only ever withholds offers.
pub fn is_locked(common_dir: &Path, admin_name: Option<&str>) -> bool {
    admin_dir(common_dir, admin_name)
        .is_some_and(|d| std::fs::symlink_metadata(d.join("locked")).is_ok())
}

/// Why a foreign worktree is "hands off", first that applies: detached
/// head, locked, unreadable. Never for GitRaptor's own worktrees.
pub fn hands_off(
    tool: OriginToolView,
    status: &WorktreeStatus,
    locked: bool,
) -> Option<HandsOffReasonView> {
    if tool == OriginToolView::Gitraptor {
        return None;
    }
    match status {
        WorktreeStatus::Ready {
            head: HeadView::Detached,
            ..
        } => Some(HandsOffReasonView::Detached),
        _ if locked => Some(HandsOffReasonView::Locked),
        WorktreeStatus::Unavailable { .. } => Some(HandsOffReasonView::Unreadable),
        WorktreeStatus::Ready { .. } => None,
    }
}

/// The origin of a worktree; `None` for the main one.
pub fn origin_of(
    common_dir: &Path,
    root: &Path,
    main: bool,
    admin_name: Option<&str>,
    registered_by_gitraptor: bool,
    status: &WorktreeStatus,
    env: &OriginEnv,
) -> Option<WorktreeOriginView> {
    if main {
        return None;
    }
    let (tool, evidence) = detect(common_dir, root, admin_name, registered_by_gitraptor, env);
    let reason = hands_off(tool, status, is_locked(common_dir, admin_name));
    WorktreeOriginView::new(tool, evidence, reason).ok()
}

/// Hook for discard, cleanup and copy: `true` for a linked worktree with no
/// origin or with `hands_off`, so nothing proves it is safe to touch.
pub fn refuses_destructive(view: &WorktreeView) -> bool {
    !view.main && view.origin.as_ref().is_none_or(|o| o.is_hands_off())
}
