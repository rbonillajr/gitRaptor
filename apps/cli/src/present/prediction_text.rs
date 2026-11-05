//! The typed en/es catalog of the conflict prediction's texts (DS-US-CKP-006, AD1).
//!
//! Every string is whole: no sentence is built by joining pieces, so a language is free to word
//! it its own way. The view folds the result to ASCII when the symbol set asks for it.

use gitraptor_api::prediction::{ConflictKindView, LimitCode};

use super::i18n::Lang;

/// One text of the prediction panel and its `?` help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionText {
    /// One of the eight declared limits, in full (the `?` help).
    Limit(LimitCode),
    /// Title of the `?` group that lists the limits.
    LimitsTitle,
    /// Title of the panel.
    PanelTitle,
    /// The three pieces of the limits line under the panel.
    LineUncommitted,
    LineCommitted,
    LineDrivers,
    /// The base side of a pair without a branch name.
    Base,
    Recalculating,
    MergeRule,
    /// What a conflicting file is, when it is not a content conflict.
    Kind(ConflictKindView),
    /// Files of a pair whose detail has not arrived.
    Files(u32),
    /// More overlapping files than the detail lists.
    AndMore,
    /// Pairs that do not fit in the panel.
    MorePairs(usize),
    /// Line ranges of a file past the three shown.
    MoreRanges(usize),
}

impl PredictionText {
    pub fn render(self, lang: Lang) -> String {
        match lang {
            Lang::En => self.en(),
            Lang::Es => self.es(),
        }
    }

    /// Every text, for the test of the two languages.
    pub fn every() -> Vec<Self> {
        let mut all: Vec<Self> = LimitCode::ALL.into_iter().map(Self::Limit).collect();
        all.extend([
            Self::LimitsTitle,
            Self::PanelTitle,
            Self::LineUncommitted,
            Self::LineCommitted,
            Self::LineDrivers,
            Self::Base,
            Self::Recalculating,
            Self::MergeRule,
            Self::Files(1),
            Self::Files(3),
            Self::AndMore,
            Self::MorePairs(2),
            Self::MoreRanges(2),
        ]);
        all.extend(
            [
                ConflictKindView::Content,
                ConflictKindView::AddAdd,
                ConflictKindView::ModifyDelete,
                ConflictKindView::Rename,
                ConflictKindView::DirectoryFile,
                ConflictKindView::Binary,
                ConflictKindView::Submodule,
            ]
            .map(Self::Kind),
        );
        all
    }

    fn en(self) -> String {
        match self {
            Self::Limit(code) => limit_en(code).to_owned(),
            Self::LimitsTitle => "Prediction limits".to_owned(),
            Self::PanelTitle => "Predicted conflicts".to_owned(),
            Self::LineUncommitted => "uncommitted".to_owned(),
            Self::LineCommitted => "commits only".to_owned(),
            Self::LineDrivers => "no merge drivers or .gitattributes".to_owned(),
            Self::Base => "base".to_owned(),
            Self::Recalculating => "recalculating".to_owned(),
            Self::MergeRule => "merge rule".to_owned(),
            Self::Kind(kind) => kind_en(kind).to_owned(),
            Self::Files(1) => "1 file".to_owned(),
            Self::Files(n) => format!("{n} files"),
            Self::AndMore => "and more".to_owned(),
            Self::MorePairs(n) => format!("+{n} more pairs"),
            Self::MoreRanges(n) => format!("+{n}"),
        }
    }

    fn es(self) -> String {
        match self {
            Self::Limit(code) => limit_es(code).to_owned(),
            Self::LimitsTitle => "Límites de la predicción".to_owned(),
            Self::PanelTitle => "Conflictos previstos".to_owned(),
            Self::LineUncommitted => "con lo sin commitear".to_owned(),
            Self::LineCommitted => "solo commits".to_owned(),
            Self::LineDrivers => "sin drivers ni .gitattributes".to_owned(),
            Self::Base => "base".to_owned(),
            Self::Recalculating => "recalculando".to_owned(),
            Self::MergeRule => "regla de merge".to_owned(),
            Self::Kind(kind) => kind_es(kind).to_owned(),
            Self::Files(1) => "1 archivo".to_owned(),
            Self::Files(n) => format!("{n} archivos"),
            Self::AndMore => "y más".to_owned(),
            Self::MorePairs(n) => format!("+{n} pares más"),
            Self::MoreRanges(n) => format!("+{n}"),
        }
    }
}

// Each limit fits 48 columns, so it shows whole in the help (`HELP_WIDTH`).
fn limit_en(code: LimitCode) -> &'static str {
    match code {
        LimitCode::OverlapIncludesUncommitted => "⚠ counts work that is not committed",
        LimitCode::ConflictCommittedOnly => "⚡ uses only committed work",
        LimitCode::NoMergeDriversOrAttributes => "Merge drivers and .gitattributes are not applied",
        LimitCode::RenameLimit => "Rename detection stops at a limit",
        LimitCode::InsertionsMayMerge => "Without ⚡ the merge may still conflict",
        LimitCode::JoinedHunks => "Nearby edits may be joined in one hunk",
        LimitCode::PairIgnoresBaseMoves => "A pair ignores later moves of the base",
        LimitCode::LocalOnly => "Local only: nothing is fetched or pushed",
    }
}

fn limit_es(code: LimitCode) -> &'static str {
    match code {
        LimitCode::OverlapIncludesUncommitted => "⚠ cuenta lo que no está commiteado",
        LimitCode::ConflictCommittedOnly => "⚡ solo usa lo commiteado",
        LimitCode::NoMergeDriversOrAttributes => "No aplica drivers de merge ni .gitattributes",
        LimitCode::RenameLimit => "La detección de renombrados tiene tope",
        LimitCode::InsertionsMayMerge => "Sin ⚡ el merge aún puede tener conflictos",
        LimitCode::JoinedHunks => "Ediciones cercanas pueden unirse en un hunk",
        LimitCode::PairIgnoresBaseMoves => "El par ignora cambios posteriores de la base",
        LimitCode::LocalOnly => "Solo local: sin fetch ni push",
    }
}

fn kind_en(kind: ConflictKindView) -> &'static str {
    match kind {
        ConflictKindView::Content => "content",
        ConflictKindView::AddAdd => "added in both",
        ConflictKindView::ModifyDelete => "modified and deleted",
        ConflictKindView::Rename => "renamed in both",
        ConflictKindView::DirectoryFile => "file and directory",
        ConflictKindView::Binary => "binary",
        ConflictKindView::Submodule => "submodule",
    }
}

fn kind_es(kind: ConflictKindView) -> &'static str {
    match kind {
        ConflictKindView::Content => "contenido",
        ConflictKindView::AddAdd => "añadido en los dos",
        ConflictKindView::ModifyDelete => "modificado y borrado",
        ConflictKindView::Rename => "renombrado en los dos",
        ConflictKindView::DirectoryFile => "archivo y carpeta",
        ConflictKindView::Binary => "binario",
        ConflictKindView::Submodule => "submódulo",
    }
}
