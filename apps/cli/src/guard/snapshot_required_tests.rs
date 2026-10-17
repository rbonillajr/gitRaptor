//! US-GRD-017 D8: the texts of `system.snapshot-failed`. One hint per cause in English and
//! Spanish, a fixed template, the rule named in `raptor guard log`, and no text that mentions the
//! exception, `raptor guard exec` or how to lift the protection (ADR-GRD-003 § 3, M-05).
//!
//! The catalogs are read from `apps/cli/i18n/{en,es}/` in the source tree: a missing
//! `snapshotreq.txt` is a failed assertion that says so, never a panic in setup.

use std::path::PathBuf;

use gitraptor_api::guard::{Cause, Level, Reason, Rule};

use super::snapshot_required::{log_rule_key, reason_text};

const CAUSES: [Cause; 7] = [
    Cause::SnapshotTimeLimit,
    Cause::SnapshotNoSpace,
    Cause::SnapshotQuotaExceeded,
    Cause::SnapshotDiscarded,
    Cause::SnapshotNoWorktree,
    Cause::SnapshotUnavailable,
    Cause::SnapshotInternal,
];

fn reason(cause: Cause) -> Reason {
    Reason {
        rule: Rule::SnapshotFailed,
        level: Level::System,
        cause: Some(cause),
        params: Vec::new(),
    }
}

fn catalog_path(lang: &str, file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("i18n")
        .join(lang)
        .join(file)
}

/// The `key = text` lines of `i18n/<lang>/<file>`; the test fails if the file is not there.
fn catalog(lang: &str, file: &str) -> Vec<(String, String)> {
    let path = catalog_path(lang, file);
    assert!(path.is_file(), "missing catalog {}", path.display());
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

fn value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[test]
fn every_cause_has_a_hint() {
    // The text of each cause, in the language of the process: never empty, never a bare key.
    let mut texts = Vec::new();
    for cause in CAUSES {
        let text = reason_text(&reason(cause));
        assert!(!text.trim().is_empty(), "{cause:?}");
        assert!(
            !text.contains("snapshotreq."),
            "{cause:?} shows a key: {text}"
        );
        texts.push((cause, text));
    }
    // The hints differ where the way out differs: time, disk and "not running".
    let text_of = |c: Cause| &texts.iter().find(|(k, _)| *k == c).unwrap().1;
    assert_ne!(
        text_of(Cause::SnapshotTimeLimit),
        text_of(Cause::SnapshotUnavailable)
    );
    assert_ne!(
        text_of(Cause::SnapshotTimeLimit),
        text_of(Cause::SnapshotNoSpace)
    );
    assert_ne!(
        text_of(Cause::SnapshotNoSpace),
        text_of(Cause::SnapshotUnavailable)
    );

    // Both languages carry the template and every hint.
    for lang in ["en", "es"] {
        let entries = catalog(lang, "snapshotreq.txt");
        for key in [
            "snapshotreq.deny",
            "snapshotreq.hint.start",
            "snapshotreq.hint.unobserved",
            "snapshotreq.hint.large-file",
            "snapshotreq.hint.no-space",
            "snapshotreq.hint.retry",
            "snapshotreq.hint.no-worktree",
        ] {
            let text = value(&entries, key);
            assert!(
                text.is_some_and(|t| !t.is_empty()),
                "{lang}: missing or empty {key}"
            );
        }
    }
    // The way back for a repo that is no longer observed is named (Q-GRD-38).
    let es = catalog("es", "snapshotreq.txt");
    let en = catalog("en", "snapshotreq.txt");
    for entries in [&es, &en] {
        assert!(
            value(entries, "snapshotreq.hint.unobserved")
                .is_some_and(|t| t.contains("raptor repo add"))
        );
    }
    assert!(
        value(&es, "snapshotreq.hint.start")
            .is_some_and(|t| t.contains("arranca GitRaptor y repite la operación"))
    );
}

#[test]
fn messages_never_mention_the_exception() {
    const FORBIDDEN: [&str; 11] = [
        "exception",
        "excepción",
        "excepcion",
        "guard exec",
        "uninstall",
        "desinstal",
        "remove the protection",
        "lift the protection",
        "quitar la protección",
        "quitar la proteccion",
        "bypass",
    ];
    for lang in ["en", "es"] {
        let entries = catalog(lang, "snapshotreq.txt");
        assert!(
            value(&entries, "snapshotreq.deny").is_some(),
            "{lang}: no snapshotreq.deny"
        );
        for (key, text) in &entries {
            assert!(key.starts_with("snapshotreq."), "{lang}: stray key {key}");
            let lower = text.to_lowercase();
            for word in FORBIDDEN {
                assert!(
                    !lower.contains(word),
                    "{lang}: {key} mentions «{word}»: {text}"
                );
            }
        }
    }
    // What the agent reads, for every cause, in the language of the process.
    for cause in CAUSES {
        let lower = reason_text(&reason(cause)).to_lowercase();
        for word in FORBIDDEN {
            assert!(
                !lower.contains(word),
                "{cause:?} mentions «{word}»: {lower}"
            );
        }
    }
}

#[test]
fn log_names_the_rule_with_its_own_key() {
    assert_eq!(log_rule_key(), "snapshotreq.log.rule");
    for lang in ["en", "es"] {
        let entries = catalog(lang, "snapshotreq.txt");
        assert!(
            value(&entries, "snapshotreq.log.rule").is_some_and(|t| !t.is_empty()),
            "{lang}: no snapshotreq.log.rule"
        );
    }
}

/// With D2 a missing GitRaptor also blocks deleting branches: the explanation of the permission
/// (BR-AUTH-002) must say so.
#[test]
fn install_unavailable_mentions_branch_deletion() {
    let en = catalog("en", "guard.txt");
    let text = value(&en, "guard.install.unavailable")
        .expect("en: no guard.install.unavailable")
        .to_lowercase();
    assert!(
        ["delete branches", "deleting branches", "branch deletion"]
            .iter()
            .any(|w| text.contains(w)),
        "en: {text}"
    );
    let es = catalog("es", "guard.txt");
    let text = value(&es, "guard.install.unavailable")
        .expect("es: no guard.install.unavailable")
        .to_lowercase();
    assert!(text.contains("borrar ramas"), "es: {text}");
}

/// GRD017-LEGACY: the text of `system.internal-error` says how to refresh the
/// protection and to repeat the operation.
#[test]
fn internal_error_says_how_to_refresh_the_protection() {
    for (lang, phrase) in [
        ("en", "refresh the protection"),
        ("es", "refresca la protección"),
    ] {
        let entries = catalog(lang, "guard.txt");
        let text = value(&entries, "guard.deny.internal")
            .unwrap_or_else(|| panic!("{lang}: no guard.deny.internal"))
            .to_lowercase();
        assert!(text.contains("raptor guard install"), "{lang}: {text}");
        assert!(text.contains(phrase), "{lang}: {text}");
    }
}
