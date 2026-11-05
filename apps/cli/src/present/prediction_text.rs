//! The typed en/es catalog of the conflict prediction's texts (DS-US-CKP-006, AD1).

use gitraptor_api::prediction::LimitCode;

use super::i18n::Lang;

/// One text of the prediction panel and its `?` help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionText {
    Limit(LimitCode),
}

impl PredictionText {
    pub fn render(self, _lang: Lang) -> String {
        String::new()
    }

    /// Every text, for the test of the two languages.
    pub fn every() -> Vec<Self> {
        Vec::new()
    }
}
