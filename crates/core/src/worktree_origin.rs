//! Read-only detection of who created a linked worktree (US-WTL-005, BR-WTL-011).
//!
//! Compile-only skeleton for the contract tests; the behaviour lands with T003.

use std::path::{Path, PathBuf};

use gitraptor_api::messages::{WorktreeStatus, WorktreeView};
use gitraptor_api::worktree_origin::{
    HandsOffReasonView, OriginEvidenceView, OriginToolView, WorktreeOriginView,
};

/// The environment the path rules read.
#[derive(Debug, Clone, Default)]
pub struct OriginEnv {
    pub homes: Vec<PathBuf>,
    pub codex_home: Option<PathBuf>,
}

pub fn detect(
    _common_dir: &Path,
    _root: &Path,
    _admin_name: Option<&str>,
    _registered_by_gitraptor: bool,
    _env: &OriginEnv,
) -> (OriginToolView, OriginEvidenceView) {
    (OriginToolView::Unknown, OriginEvidenceView::None)
}

pub fn is_locked(_common_dir: &Path, _admin_name: Option<&str>) -> bool {
    false
}

pub fn hands_off(
    _tool: OriginToolView,
    _status: &WorktreeStatus,
    _locked: bool,
) -> Option<HandsOffReasonView> {
    None
}

pub fn origin_of(
    _common_dir: &Path,
    _root: &Path,
    _main: bool,
    _admin_name: Option<&str>,
    _registered_by_gitraptor: bool,
    _status: &WorktreeStatus,
    _env: &OriginEnv,
) -> Option<WorktreeOriginView> {
    None
}

pub fn refuses_destructive(_view: &WorktreeView) -> bool {
    false
}
