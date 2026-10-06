//! User-facing messages in the user's language, English or Spanish
//! (NFR-10, ADR-GRP-003: messages live in resource files and are never
//! built by concatenation).
//!
//! Minimal by design (Decisión del orquestador, 2026-10-04, validada por el
//! Arquitecto): catalogs embedded at build time, `key = text` lines with
//! named placeholders, no dependencies. The language comes from `LC_ALL`,
//! `LC_MESSAGES` or `LANG`, in that order; anything that is not Spanish is
//! English.
//!
//! One file per feature and language, `i18n/<lang>/<feature>.txt`
//! (ADR-GRP-016 § 4): `build.rs` registers every file it finds, so a story
//! adds its messages without editing a shared catalog. A key group (the
//! text before the first dot) belongs to one feature file.

use std::collections::HashMap;
use std::sync::OnceLock;

mod catalogs {
    include!(concat!(env!("OUT_DIR"), "/i18n_catalogs.rs"));
}

use catalogs::{EN, ES};

/// `key = text` lines of one file.
fn entries(file: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    file.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim(), v.trim()))
}

fn parse(catalog: &'static [(&'static str, &'static str)]) -> HashMap<&'static str, &'static str> {
    catalog.iter().flat_map(|(_, file)| entries(file)).collect()
}

/// The text of every file of `feature` in both languages (tests of the callers).
#[cfg(test)]
pub fn feature_files(feature: &str) -> Vec<&'static str> {
    EN.iter()
        .chain(ES)
        .filter(|(name, _)| *name == feature)
        .map(|(_, file)| *file)
        .collect()
}

fn spanish() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .is_some_and(|lang| lang.starts_with("es"))
}

fn catalog() -> &'static HashMap<&'static str, &'static str> {
    static CATALOG: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    CATALOG.get_or_init(|| parse(if spanish() { ES } else { EN }))
}

/// The message `key` with its `{name}` placeholders filled. A missing key
/// shows the key itself (the catalog test keeps that from shipping).
pub fn t(key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut text = catalog().get(key).copied().unwrap_or(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), &value.to_string());
    }
    text
}

/// Both catalogs have `key` (tests of the callers).
#[cfg(test)]
pub fn has_key(key: &str) -> bool {
    parse(EN).contains_key(key) && parse(ES).contains_key(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<&str> {
        let mut out: Vec<&str> = text
            .split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
            .collect();
        out.sort_unstable();
        out
    }

    /// No key is defined twice, in one file or across files: the second
    /// definition would silently win.
    #[test]
    fn no_key_is_defined_twice() {
        for catalog in [EN, ES] {
            let mut keys: Vec<&str> = catalog
                .iter()
                .flat_map(|(_, file)| entries(file).map(|(k, _)| k))
                .collect();
            let total = keys.len();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), total, "duplicate keys in a catalog");
        }
    }

    /// Both languages have the same files, each file the same keys and each
    /// key the same placeholders.
    #[test]
    fn catalogs_match() {
        let names = |c: &'static [(&'static str, &'static str)]| -> Vec<&'static str> {
            c.iter().map(|(n, _)| *n).collect()
        };
        assert_eq!(names(EN), names(ES), "the same feature files in en and es");
        for ((feature, en), (_, es)) in EN.iter().zip(ES) {
            let en: HashMap<_, _> = entries(en).collect();
            let es: HashMap<_, _> = entries(es).collect();
            let mut keys: Vec<_> = en.keys().collect();
            keys.sort();
            let mut es_keys: Vec<_> = es.keys().collect();
            es_keys.sort();
            assert_eq!(keys, es_keys, "{feature}");
            for key in keys {
                assert_eq!(placeholders(en[key]), placeholders(es[key]), "{key}");
            }
        }
    }

    /// A key group (`guard.` in `guard.deny.force-push`) lives in one feature
    /// file: two stories never add keys of one group in two places.
    #[test]
    fn each_key_group_belongs_to_one_file() {
        let mut owner: HashMap<&str, &str> = HashMap::new();
        for (feature, file) in EN {
            for (key, _) in entries(file) {
                let group = key.split('.').next().unwrap_or(key);
                let first = *owner.entry(group).or_insert(feature);
                assert_eq!(
                    first, *feature,
                    "the group {group}. is in {first} and {feature}"
                );
            }
        }
    }

    #[test]
    fn placeholders_are_filled_by_name() {
        let en = parse(EN);
        let text = en["repo.not-a-repo"].replace("{path}", "notas");
        assert_eq!(text, "notas is not a Git repository");
    }
}
