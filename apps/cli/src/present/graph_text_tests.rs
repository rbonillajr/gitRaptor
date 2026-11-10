//! US-CKP-022 T006: the catalog of the graph (`present/graph_text.rs`) has every text in en and
//! es (the pattern of `prediction_text_tests.rs`). Mounted by the expert in `present/mod.rs` as
//! `#[cfg(test)] mod graph_text_tests;`.

use super::graph_text::GraphText;
use super::i18n::Lang;

#[test]
fn every_graph_text_has_both_languages() {
    let every = GraphText::every();
    // The DS names at least: the three base messages and the confirm action, repo-unavailable,
    // five lane reasons, two cuts, "sin atribuir", "N más", "al menos N más", "(merge)",
    // "actualizando…", the title and the lane header.
    assert!(
        every.len() >= 16,
        "the catalog lists too few texts: {}",
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

    // Two languages, not one twice: most of the catalog differs between them.
    let differing = every
        .iter()
        .filter(|t| t.render(Lang::En) != t.render(Lang::Es))
        .count();
    assert!(
        differing * 2 >= every.len(),
        "the English and Spanish texts are the same for {} of {} entries",
        every.len() - differing,
        every.len()
    );

    // The Spanish the DS fixes word for word (D7, D12, D13).
    let es: Vec<String> = every.iter().map(|t| t.render(Lang::Es)).collect();
    for wanted in [
        "sin atribuir",
        "actualizando…",
        "no disponible",
        "sin commits",
        "tardó demasiado",
        "no se pudo leer",
        "truncado: más de 32 carriles",
        "truncado: respuesta demasiado grande",
    ] {
        assert!(
            es.iter().any(|t| t == wanted),
            "the Spanish catalog lacks \"{wanted}\": {es:#?}"
        );
    }
}
