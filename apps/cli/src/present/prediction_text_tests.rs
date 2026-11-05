//! CKP006-TEXT-I18N (AD1): the prediction catalog has every text in en and es, and none about
//! age (that is DS-US-CKP-007's `freshness_text`).

use super::i18n::Lang;
use super::prediction_text::PredictionText;

#[test]
fn every_prediction_text_has_both_languages() {
    let every = PredictionText::every();
    assert!(
        every.len() >= 8,
        "the catalog lists no texts (the 8 limits alone need 8): {}",
        every.len()
    );
    for text in &every {
        for lang in [Lang::En, Lang::Es] {
            let rendered = text.render(lang);
            assert!(
                !rendered.trim().is_empty(),
                "{text:?} has no text in {lang:?}"
            );
        }
    }
    let es: Vec<String> = every.iter().map(|t| t.render(Lang::Es)).collect();
    assert!(
        es.iter()
            .any(|t| t == "Sin ⚡ el merge aún puede tener conflictos"),
        "the `insertions-may-merge` limit must read exactly as the DS says in es: {es:#?}"
    );
}

#[test]
fn the_catalog_has_no_age_text() {
    let every = PredictionText::every();
    assert!(!every.is_empty(), "the catalog lists no texts");
    for text in every {
        let en = text.render(Lang::En).to_lowercase();
        let es = text.render(Lang::Es).to_lowercase();
        assert!(
            !en.contains(" ago") && !en.contains("min ") && !en.contains("outdated"),
            "age text in en (DS-US-CKP-007's): {en}"
        );
        assert!(
            !es.contains("hace ") && !es.contains("desactualizado"),
            "age text in es (DS-US-CKP-007's): {es}"
        );
    }
}
