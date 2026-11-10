//! The typed en/es catalog of the diff view (DS-US-CKP-012, T007).
//!
//! Every string is whole: no sentence is built by joining pieces, so a language is free to word
//! it its own way. The view folds the result to ASCII when the symbol set asks for it.

use gitraptor_api::diff::{CommittedGap, DiffCut, DiffUnavailable, FileChange};

use super::diff::FileNote;
use super::i18n::Lang;

/// One text of the diff view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffText<'a> {
    /// The title of the view: the worktree's folder and the time of the photo (`HH:MM:SS`).
    Title {
        worktree: &'a str,
        clock: &'a str,
    },
    /// The title while there is no photo yet, or none came.
    TitlePending {
        worktree: &'a str,
    },
    /// What would enter the merge, with the files of the section.
    Committed(u32),
    /// What the worktree has not committed, with the files of the section.
    Uncommitted(u32),
    /// Why the section of the branch cannot be computed.
    Gap(CommittedGap),
    /// Why a section lists fewer files than it has.
    Cut(DiffCut),
    Change(FileChange),
    Note(FileNote),
    Loading,
    Busy,
    Unavailable(DiffUnavailable),
    Lost,
    Refused,
}

impl DiffText<'_> {
    pub fn render(self, lang: Lang) -> String {
        match lang {
            Lang::En => self.en(),
            Lang::Es => self.es(),
        }
    }

    /// Every text, for the test of the two languages.
    pub fn every() -> Vec<DiffText<'static>> {
        let mut all = vec![
            DiffText::Title {
                worktree: "feat-pagos",
                clock: "14:03:12",
            },
            DiffText::TitlePending {
                worktree: "feat-pagos",
            },
            DiffText::Committed(1),
            DiffText::Committed(3),
            DiffText::Uncommitted(1),
            DiffText::Uncommitted(3),
            DiffText::Loading,
            DiffText::Busy,
            DiffText::Lost,
            DiffText::Refused,
        ];
        all.extend(
            [
                CommittedGap::BaseUnconfirmed,
                CommittedGap::NoBase,
                CommittedGap::BaseMissing,
                CommittedGap::NoMergeBase,
                CommittedGap::TooSlow,
            ]
            .map(DiffText::Gap),
        );
        all.extend([DiffCut::Files, DiffCut::Bytes, DiffCut::Time].map(DiffText::Cut));
        all.extend(
            [
                FileChange::Added,
                FileChange::Modified,
                FileChange::Deleted,
                FileChange::TypeChanged,
            ]
            .map(DiffText::Change),
        );
        all.extend(
            [
                FileNote::Binary,
                FileNote::TooLarge,
                FileNote::Symlink,
                FileNote::Submodule,
                FileNote::Omitted,
                FileNote::LinesCut,
            ]
            .map(DiffText::Note),
        );
        all.extend(
            [
                DiffUnavailable::WorktreeUnavailable,
                DiffUnavailable::RepoUnavailable,
            ]
            .map(DiffText::Unavailable),
        );
        all
    }

    fn en(self) -> String {
        match self {
            Self::Title { worktree, clock } => format!("{worktree} · diff at {clock} UTC"),
            Self::TitlePending { worktree } => format!("{worktree} · diff"),
            Self::Committed(1) => "would enter the merge (1 file)".to_owned(),
            Self::Committed(n) => format!("would enter the merge ({n} files)"),
            Self::Uncommitted(1) => "uncommitted (1 file)".to_owned(),
            Self::Uncommitted(n) => format!("uncommitted ({n} files)"),
            Self::Gap(CommittedGap::BaseUnconfirmed) => {
                "not computable: the base is not confirmed".to_owned()
            }
            Self::Gap(CommittedGap::NoBase) => "not computable: there is no base".to_owned(),
            Self::Gap(CommittedGap::BaseMissing) => {
                "not computable: the base branch does not exist".to_owned()
            }
            Self::Gap(CommittedGap::NoMergeBase) => {
                "not computable: no common ancestor with the base".to_owned()
            }
            Self::Gap(CommittedGap::TooSlow) => "not computable: it took too long".to_owned(),
            Self::Cut(DiffCut::Files) => "truncated: more than 1,000 files".to_owned(),
            Self::Cut(DiffCut::Bytes) => "truncated: the reply is too large".to_owned(),
            Self::Cut(DiffCut::Time) => "truncated: more than 2 s".to_owned(),
            Self::Change(FileChange::Added) => "added".to_owned(),
            Self::Change(FileChange::Modified) => "modified".to_owned(),
            Self::Change(FileChange::Deleted) => "deleted".to_owned(),
            Self::Change(FileChange::TypeChanged) => "type changed".to_owned(),
            Self::Note(FileNote::Binary) => "binary".to_owned(),
            Self::Note(FileNote::TooLarge) => "truncated: more than 8 MiB".to_owned(),
            Self::Note(FileNote::Symlink) => "symbolic link".to_owned(),
            Self::Note(FileNote::Submodule) => "submodule".to_owned(),
            Self::Note(FileNote::Omitted) => "no content: a limit was reached".to_owned(),
            Self::Note(FileNote::LinesCut) => "truncated: more than 2,000 lines".to_owned(),
            Self::Loading => "computing the diff…".to_owned(),
            Self::Busy => "another query is running; try again in a moment".to_owned(),
            Self::Unavailable(DiffUnavailable::WorktreeUnavailable) => {
                "the worktree is not available".to_owned()
            }
            Self::Unavailable(DiffUnavailable::RepoUnavailable) => {
                "the repo is not available".to_owned()
            }
            Self::Lost => "the connection was lost; ask for it again".to_owned(),
            Self::Refused => "the engine did not give the diff".to_owned(),
        }
    }

    fn es(self) -> String {
        match self {
            Self::Title { worktree, clock } => format!("{worktree} · diff a las {clock} UTC"),
            Self::TitlePending { worktree } => format!("{worktree} · diff"),
            Self::Committed(1) => "entraría al merge (1 archivo)".to_owned(),
            Self::Committed(n) => format!("entraría al merge ({n} archivos)"),
            Self::Uncommitted(1) => "sin commitear (1 archivo)".to_owned(),
            Self::Uncommitted(n) => format!("sin commitear ({n} archivos)"),
            Self::Gap(CommittedGap::BaseUnconfirmed) => {
                "no calculable: base sin confirmar".to_owned()
            }
            Self::Gap(CommittedGap::NoBase) => "no calculable: no hay base".to_owned(),
            Self::Gap(CommittedGap::BaseMissing) => {
                "no calculable: la rama base no existe".to_owned()
            }
            Self::Gap(CommittedGap::NoMergeBase) => {
                "no calculable: sin ancestro común con la base".to_owned()
            }
            Self::Gap(CommittedGap::TooSlow) => "no calculable: tardó demasiado".to_owned(),
            Self::Cut(DiffCut::Files) => "truncado: más de 1.000 archivos".to_owned(),
            Self::Cut(DiffCut::Bytes) => "truncado: respuesta demasiado grande".to_owned(),
            Self::Cut(DiffCut::Time) => "truncado: más de 2 s".to_owned(),
            Self::Change(FileChange::Added) => "añadido".to_owned(),
            Self::Change(FileChange::Modified) => "modificado".to_owned(),
            Self::Change(FileChange::Deleted) => "borrado".to_owned(),
            Self::Change(FileChange::TypeChanged) => "cambio de tipo".to_owned(),
            Self::Note(FileNote::Binary) => "binario".to_owned(),
            Self::Note(FileNote::TooLarge) => "truncado: más de 8 MiB".to_owned(),
            Self::Note(FileNote::Symlink) => "enlace simbólico".to_owned(),
            Self::Note(FileNote::Submodule) => "submódulo".to_owned(),
            Self::Note(FileNote::Omitted) => "sin contenido: se alcanzó un tope".to_owned(),
            Self::Note(FileNote::LinesCut) => "truncado: más de 2.000 líneas".to_owned(),
            Self::Loading => "calculando el diff…".to_owned(),
            Self::Busy => "otra consulta en curso; inténtalo en un momento".to_owned(),
            Self::Unavailable(DiffUnavailable::WorktreeUnavailable) => {
                "el worktree no está disponible".to_owned()
            }
            Self::Unavailable(DiffUnavailable::RepoUnavailable) => {
                "el repo no está disponible".to_owned()
            }
            Self::Lost => "se perdió la conexión; vuelve a pedirlo".to_owned(),
            Self::Refused => "el motor no dio el diff".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_has_both_languages() {
        let every = DiffText::every();
        assert!(every.len() >= 30, "the catalog lists {} texts", every.len());
        for text in &every {
            let (en, es) = (text.render(Lang::En), text.render(Lang::Es));
            assert!(!en.trim().is_empty(), "{text:?} has no en text");
            assert!(!es.trim().is_empty(), "{text:?} has no es text");
            // The same sentence in both languages would be an untranslated one; only the
            // title while pending (a name and a word) is the same.
            if !matches!(text, DiffText::TitlePending { .. }) {
                assert_ne!(en, es, "{text:?} is the same in en and es");
            }
        }
        assert_eq!(
            DiffText::Committed(3).render(Lang::Es),
            "entraría al merge (3 archivos)"
        );
        assert_eq!(
            DiffText::Uncommitted(1).render(Lang::Es),
            "sin commitear (1 archivo)"
        );
    }
}
