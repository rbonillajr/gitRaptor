//! The typed en/es catalog of the alert of a new predicted conflict.
//!
//! Every string is whole per language and per shape (no concatenation of pieces). The ⚡ is
//! painted by the toast widget, so it is not part of the text; the view folds `↔` to ASCII
//! when the symbol set asks for it.

use super::i18n::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAlertText<'a> {
    /// "Predicted conflict: a ↔ b in file and 2 more": `file` is `None` while its detail has
    /// not arrived; `more` counts the files after the first.
    Toast {
        left: &'a str,
        right: &'a str,
        file: Option<&'a str>,
        more: u32,
    },
    /// The mark of an entry of the panel that appeared in this run.
    New,
}

impl ConflictAlertText<'_> {
    pub fn render(self, lang: Lang) -> String {
        match lang {
            Lang::En => self.en(),
            Lang::Es => self.es(),
        }
    }

    /// Every shape of text, for the test of the two languages.
    pub fn every() -> Vec<ConflictAlertText<'static>> {
        let toast = |file, more| ConflictAlertText::Toast {
            left: "a",
            right: "b",
            file,
            more,
        };
        vec![
            toast(None, 0),
            toast(Some("f.rs"), 0),
            toast(Some("f.rs"), 1),
            toast(Some("f.rs"), 3),
            ConflictAlertText::New,
        ]
    }

    fn en(self) -> String {
        match self {
            Self::Toast {
                left,
                right,
                file: None,
                ..
            } => format!("Predicted conflict: {left} ↔ {right}"),
            Self::Toast {
                left,
                right,
                file: Some(file),
                more: 0,
            } => format!("Predicted conflict: {left} ↔ {right} in {file}"),
            Self::Toast {
                left,
                right,
                file: Some(file),
                more: 1,
            } => format!("Predicted conflict: {left} ↔ {right} in {file} and 1 more"),
            Self::Toast {
                left,
                right,
                file: Some(file),
                more,
            } => format!("Predicted conflict: {left} ↔ {right} in {file} and {more} more"),
            Self::New => "new".to_owned(),
        }
    }

    fn es(self) -> String {
        match self {
            Self::Toast {
                left,
                right,
                file: None,
                ..
            } => format!("Conflicto previsto: {left} ↔ {right}"),
            Self::Toast {
                left,
                right,
                file: Some(file),
                more: 0,
            } => format!("Conflicto previsto: {left} ↔ {right} en {file}"),
            Self::Toast {
                left,
                right,
                file: Some(file),
                more: 1,
            } => format!("Conflicto previsto: {left} ↔ {right} en {file} y 1 más"),
            Self::Toast {
                left,
                right,
                file: Some(file),
                more,
            } => format!("Conflicto previsto: {left} ↔ {right} en {file} y {more} más"),
            Self::New => "nuevo".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_is_whole_in_both_languages_and_names_the_plural() {
        for text in ConflictAlertText::every() {
            let (en, es) = (text.render(Lang::En), text.render(Lang::Es));
            assert!(!en.is_empty() && !es.is_empty());
            assert_ne!(en, es, "{text:?} is not translated");
        }
        let more = |n| ConflictAlertText::Toast {
            left: "a",
            right: "b",
            file: Some("f"),
            more: n,
        };
        assert_eq!(
            more(1).render(Lang::En),
            "Predicted conflict: a ↔ b in f and 1 more"
        );
        assert_eq!(
            more(3).render(Lang::Es),
            "Conflicto previsto: a ↔ b en f y 3 más"
        );
    }
}
