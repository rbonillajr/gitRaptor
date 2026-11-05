//! The TUI's model of the published conflict prediction (DS-US-CKP-006, AD2).

use std::collections::BTreeMap;

use gitraptor_api::prediction::{
    ConflictKindView, LimitCode, LineRange, PairLevel, PairReason, PairStateView,
};

use crate::model::{Cmd, Model, SafeText};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PredictionModel {
    pub limits: Vec<LimitCode>,
    pub pairs: Vec<PairRow>,
    pub truncated: bool,
    pub details: BTreeMap<String, PairDetailRow>,
    early: BTreeMap<String, PairDetailRow>,
    asked: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairRow {
    pub pair_id: String,
    pub rev: u64,
    pub a: Side,
    pub b: Side,
    pub state: PairStateView,
    pub reason: Option<PairReason>,
    pub level: Option<PairLevel>,
    pub computed_at_utc_ms: Option<i64>,
    pub overlap_files: u32,
    pub overlap_partial: bool,
    pub conflict_files: u32,
    pub conflict_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    Worktree { key: u64, folder: SafeText },
    Base,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairDetailRow {
    pub rev: u64,
    pub overlap: Vec<SafeText>,
    pub conflict: Option<ConflictDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictDetail {
    pub files: Vec<ConflictFileRow>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFileRow {
    pub path: SafeText,
    pub kind: ConflictKindView,
    pub ours: Vec<LineRange>,
    pub theirs: Vec<LineRange>,
    pub hunks_truncated: bool,
    pub has_merge_rule: bool,
}

/// The `prediction.list` commands the model still needs.
pub fn wanted(_model: &mut Model) -> Vec<Cmd> {
    Vec::new()
}

impl PredictionModel {
    /// Some pair that includes the worktree has a known conflict.
    pub fn known_conflict(&self, _key: u64) -> bool {
        false
    }
}
