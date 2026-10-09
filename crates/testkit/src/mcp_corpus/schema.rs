//! The JSON Schema subset of the compact output schemas.

use serde_json::Value;

/// Keywords the checker understands. Anything else fails closed, so a new keyword in an output
/// schema can never pass the allowlist unnoticed.
const SUPPORTED: [&str; 16] = [
    "$ref",
    "$defs",
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "oneOf",
    "anyOf",
    "maxLength",
    "minimum",
    "maximum",
    "format",
    "description",
];

const MAX_DEPTH: usize = 64;

enum Problem {
    Mismatch(String),
    Unsupported(String),
}

impl Problem {
    fn into_message(self) -> String {
        match self {
            Self::Mismatch(message) | Self::Unsupported(message) => message,
        }
    }
}

fn mismatch(path: &str, what: &str) -> Problem {
    Problem::Mismatch(format!(
        "{} {what}",
        if path.is_empty() { "/" } else { path }
    ))
}

/// Checks `value` against `schema` (with `root` for `$ref`), failing closed on any keyword
/// outside the supported subset. Messages name pointers and keys, never values.
///
/// # Errors
/// A description of the first mismatch or unsupported keyword.
pub fn conforms(root: &Value, schema: &Value, value: &Value) -> Result<(), String> {
    check(root, schema, value, "", 0).map_err(Problem::into_message)
}

fn check(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), Problem> {
    if depth > MAX_DEPTH {
        return Err(mismatch(path, "is nested too deep"));
    }
    let Value::Object(map) = schema else {
        return Err(Problem::Unsupported(format!(
            "unsupported schema form at {}",
            if path.is_empty() { "/" } else { path }
        )));
    };
    if let Some(keyword) = map
        .keys()
        .find(|key| !SUPPORTED.contains(&key.as_str()) && key.as_str() != "title")
    {
        return Err(Problem::Unsupported(format!(
            "unsupported keyword {keyword}"
        )));
    }

    if let Some(reference) = map.get("$ref") {
        let target = reference
            .as_str()
            .and_then(|r| r.strip_prefix("#/$defs/"))
            .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)))
            .ok_or_else(|| Problem::Unsupported("unsupported or dangling $ref".to_owned()))?;
        check(root, target, value, path, depth + 1)?;
    }
    if let Some(kind) = map.get("type") {
        let names: Vec<&str> = match kind {
            Value::String(name) => vec![name.as_str()],
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            _ => return Err(Problem::Unsupported("unsupported type form".to_owned())),
        };
        if !names.iter().any(|name| type_matches(name, value)) {
            return Err(mismatch(path, "has the wrong type"));
        }
    }
    if let Some(Value::Array(allowed)) = map.get("enum")
        && !allowed.contains(value)
    {
        return Err(mismatch(path, "is not one of the enum values"));
    }
    if let Some(expected) = map.get("const")
        && expected != value
    {
        return Err(mismatch(path, "is not the const value"));
    }
    if let Value::Object(object) = value {
        check_object(root, map, object, path, depth)?;
    }
    if let (Some(items), Value::Array(list)) = (map.get("items"), value) {
        for (index, item) in list.iter().enumerate() {
            check(root, items, item, &format!("{path}/{index}"), depth + 1)?;
        }
    }
    if let Value::String(text) = value
        && let Some(max) = map.get("maxLength").and_then(Value::as_u64)
        && u64::try_from(text.chars().count()).is_ok_and(|n| n > max)
    {
        return Err(mismatch(path, "is longer than maxLength"));
    }
    if let Some(number) = value.as_f64() {
        if map
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|min| number < min)
        {
            return Err(mismatch(path, "is below minimum"));
        }
        if map
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|max| number > max)
        {
            return Err(mismatch(path, "is above maximum"));
        }
    }
    if let Some(Value::Array(branches)) = map.get("anyOf") {
        let mut any = false;
        for branch in branches {
            match check(root, branch, value, path, depth + 1) {
                Ok(()) => any = true,
                Err(Problem::Mismatch(_)) => {}
                Err(unsupported) => return Err(unsupported),
            }
        }
        if !any {
            return Err(mismatch(path, "matches no anyOf branch"));
        }
    }
    if let Some(Value::Array(branches)) = map.get("oneOf") {
        let mut matched = 0usize;
        for branch in branches {
            match check(root, branch, value, path, depth + 1) {
                Ok(()) => matched += 1,
                Err(Problem::Mismatch(_)) => {}
                Err(unsupported) => return Err(unsupported),
            }
        }
        if matched != 1 {
            return Err(mismatch(path, "does not match exactly one oneOf branch"));
        }
    }
    Ok(())
}

fn check_object(
    root: &Value,
    map: &serde_json::Map<String, Value>,
    object: &serde_json::Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<(), Problem> {
    let declared = map.get("properties").and_then(Value::as_object);
    if let Some(declared) = declared {
        for (name, sub) in declared {
            if let Some(item) = object.get(name) {
                check(root, sub, item, &format!("{path}/{name}"), depth + 1)?;
            }
        }
    }
    if let Some(Value::Array(required)) = map.get("required") {
        for name in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(name) {
                return Err(mismatch(path, &format!("lacks required property {name}")));
            }
        }
    }
    match map.get("additionalProperties") {
        None | Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => {
            if let Some(extra) = object
                .keys()
                .find(|key| !declared.is_some_and(|d| d.contains_key(key.as_str())))
            {
                return Err(mismatch(path, &format!("has undeclared property {extra}")));
            }
        }
        Some(sub) => {
            for (name, item) in object {
                if !declared.is_some_and(|d| d.contains_key(name.as_str())) {
                    check(root, sub, item, &format!("{path}/{name}"), depth + 1)?;
                }
            }
        }
    }
    Ok(())
}

fn type_matches(name: &str, value: &Value) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value
                    .as_f64()
                    .is_some_and(|f| f.is_finite() && f.fract() == 0.0)
        }
        _ => false,
    }
}
