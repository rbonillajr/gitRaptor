//! JSON Schema of the settings document, generated from the Rust types with `schemars`,
//! versioned in `crates/policy/schema/settings.schema.json` and embedded in the binary to
//! validate without network (ADR-GRP-007 § Schema). A test fails if they drift apart.

use std::sync::LazyLock;

use schemars::generate::SchemaSettings;
use serde_json::Value;

use super::model::Settings;

/// The versioned schema, embedded.
pub const EMBEDDED: &str = include_str!("../../schema/settings.schema.json");

/// Keywords the validator understands. The generated schema must not use any other: the
/// validator is a closed subset, not a JSON Schema interpreter.
pub const SUPPORTED_KEYWORDS: &[&str] = &[
    "$schema",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "properties",
    "items",
    "enum",
    "minimum",
    "maximum",
    "format",
    "x-gitraptor-levels",
];

/// The schema generated from the types now.
pub fn generate() -> Value {
    let generator = SchemaSettings::draft2020_12().into_generator();
    let mut schema =
        serde_json::to_value(generator.into_root_schema_for::<Settings>()).expect("schema is JSON");
    strip_null(&mut schema);
    schema
}

/// An absent key and a `null` are not the same in a strict document: `Option` fields admit
/// only their type, so `null` fails validation like any other wrong type.
fn strip_null(node: &mut Value) {
    if let Value::Object(map) = node {
        if let Some(Value::Array(types)) = map.get_mut("type") {
            types.retain(|t| t != "null");
            if types.len() == 1 {
                let only = types.remove(0);
                map.insert("type".into(), only);
            }
        }
        if let Some(Value::Array(any_of)) = map.get("anyOf") {
            let not_null: Vec<&Value> = any_of
                .iter()
                .filter(|s| s.get("type").is_none_or(|t| t != "null"))
                .collect();
            if let [only] = not_null[..] {
                let only = only.clone();
                map.remove("anyOf");
                if let Value::Object(inner) = only {
                    map.extend(inner);
                }
            }
        }
        map.values_mut().for_each(strip_null);
    } else if let Value::Array(items) = node {
        items.iter_mut().for_each(strip_null);
    }
}

/// The embedded schema, parsed once.
pub(crate) static SCHEMA: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(EMBEDDED).expect("embedded schema is valid JSON"));

#[cfg(test)]
mod tests {
    use super::*;

    fn keywords(node: &Value, out: &mut Vec<String>) {
        let Value::Object(map) = node else {
            return;
        };
        for (k, v) in map {
            out.push(k.clone());
            match k.as_str() {
                // Maps of names to schemas: their keys are names, not keywords.
                "properties" | "$defs" => v
                    .as_object()
                    .into_iter()
                    .flatten()
                    .for_each(|(_, s)| keywords(s, out)),
                "items" => keywords(v, out),
                _ => {}
            }
        }
    }

    /// ADR-GRP-007 Validación 10: the generated schema equals the versioned and embedded one.
    /// Regenerate with `GITRAPTOR_WRITE_SCHEMA=1 cargo test -p gitraptor-policy schema_`.
    #[test]
    fn schema_has_not_drifted() {
        let generated = serde_json::to_string_pretty(&generate()).unwrap() + "\n";
        if std::env::var_os("GITRAPTOR_WRITE_SCHEMA").is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/settings.schema.json");
            std::fs::write(path, &generated).unwrap();
            return;
        }
        assert_eq!(
            EMBEDDED, generated,
            "schema drift: regenerate with GITRAPTOR_WRITE_SCHEMA=1"
        );
    }

    /// The validator understands a closed subset of JSON Schema; the schema must stay inside it.
    #[test]
    fn schema_uses_only_supported_keywords() {
        let mut found = Vec::new();
        keywords(&generate(), &mut found);
        let unsupported: Vec<_> = found
            .iter()
            .filter(|k| !SUPPORTED_KEYWORDS.contains(&k.as_str()))
            .collect();
        assert!(
            unsupported.is_empty(),
            "unsupported keywords: {unsupported:?}"
        );
    }
}
