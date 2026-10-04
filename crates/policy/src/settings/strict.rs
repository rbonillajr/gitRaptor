//! Strict JSON parsing with the document limits of L-03 (ADR-GRD-004 § 1).
//!
//! The limits are enforced while parsing, so a hostile document never builds a large tree:
//! size before parsing, then depth, members per object or array, and string length in bytes
//! (keys included). A repeated key in an object, or a byte order mark, makes the document
//! invalid: an editor and the loader could otherwise read different values.

use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use super::diagnostic::{Code, Limit, Location};

/// Maximum size of a document, in bytes. ⚠️ ASSUMPTION of ADR-GRD-004 § 1.
pub const MAX_BYTES: usize = 64 * 1024;
/// Maximum nesting depth.
pub const MAX_DEPTH: usize = 16;
/// Maximum keys per object and elements per array.
pub const MAX_MEMBERS: usize = 256;
/// Maximum length of a string or a key, in bytes.
pub const MAX_STRING_BYTES: usize = 1024;

/// Marker that turns a serde error message back into a code.
const MARK: &str = "gitraptor-limit:";

/// Parse `bytes` as one strict JSON document.
pub fn parse(bytes: &[u8]) -> Result<Value, (Code, Option<Location>)> {
    if bytes.len() > MAX_BYTES {
        return Err((Code::LimitExceeded(Limit::Size), None));
    }
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        return Err((Code::ByteOrderMark, None));
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = Strict { depth: 1 }
        .deserialize(&mut de)
        .and_then(|v| de.end().map(|()| v))
        .map_err(|e| {
            let code = code_of(&e);
            let location = (e.line() > 0).then_some(Location::Position {
                line: e.line(),
                column: e.column(),
            });
            (code, location)
        })?;
    Ok(value)
}

fn code_of(e: &serde_json::Error) -> Code {
    let message = e.to_string();
    let Some(rest) = message.find(MARK).map(|i| &message[i + MARK.len()..]) else {
        return Code::InvalidJson;
    };
    match rest.split_whitespace().next().unwrap_or("") {
        "depth" => Code::LimitExceeded(Limit::Depth),
        "members" => Code::LimitExceeded(Limit::Members),
        "string" => Code::LimitExceeded(Limit::StringLength),
        "duplicate" => Code::DuplicateKey,
        _ => Code::InvalidJson,
    }
}

fn fail<E: de::Error>(what: &str) -> E {
    E::custom(format_args!("{MARK}{what}"))
}

fn check_string<E: de::Error>(s: &str) -> Result<(), E> {
    if s.len() > MAX_STRING_BYTES {
        return Err(fail("string"));
    }
    Ok(())
}

/// A value at `depth` (the top-level value has depth 1).
struct Strict {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        if self.depth > MAX_DEPTH {
            return Err(fail("depth"));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        check_string(v)?;
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        check_string(&v)?;
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(Strict {
            depth: self.depth + 1,
        })? {
            if items.len() == MAX_MEMBERS {
                return Err(fail("members"));
            }
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            check_string(&key)?;
            if out.len() == MAX_MEMBERS {
                return Err(fail("members"));
            }
            if out.contains_key(&key) {
                return Err(fail("duplicate"));
            }
            let value = map.next_value_seed(Strict {
                depth: self.depth + 1,
            })?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(input: &[u8]) -> Code {
        parse(input).unwrap_err().0
    }

    #[test]
    fn accepts_a_plain_document() {
        let v =
            parse(br#"{"engine":{"baseBranch":"main"},"permissions":{"deny":["push"]}}"#).unwrap();
        assert_eq!(v["engine"]["baseBranch"], "main");
    }

    #[test]
    fn syntax_errors_have_a_position_and_no_content() {
        let (code, location) = parse(b"{\n  \"a\": secret\n}").unwrap_err();
        assert_eq!(code, Code::InvalidJson);
        assert!(matches!(location, Some(Location::Position { line: 2, .. })));
        // Comments are not JSON.
        assert_eq!(super::parse(b"// c\n{}").unwrap_err().0, Code::InvalidJson);
        // Trailing data.
        assert_eq!(super::parse(b"{} {}").unwrap_err().0, Code::InvalidJson);
    }

    #[test]
    fn limits() {
        assert_eq!(
            code(&vec![b' '; MAX_BYTES + 1]),
            Code::LimitExceeded(Limit::Size)
        );
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert_eq!(code(deep.as_bytes()), Code::LimitExceeded(Limit::Depth));
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(ok.as_bytes()).is_ok());
        let wide = format!("[{}]", vec!["1"; MAX_MEMBERS + 1].join(","));
        assert_eq!(code(wide.as_bytes()), Code::LimitExceeded(Limit::Members));
        let keys: Vec<String> = (0..=MAX_MEMBERS).map(|i| format!("\"k{i}\":1")).collect();
        let keys = format!("{{{}}}", keys.join(","));
        assert_eq!(code(keys.as_bytes()), Code::LimitExceeded(Limit::Members));
        let long = format!("\"{}\"", "x".repeat(MAX_STRING_BYTES + 1));
        assert_eq!(
            code(long.as_bytes()),
            Code::LimitExceeded(Limit::StringLength)
        );
        let long_key = format!("{{\"{}\":1}}", "x".repeat(MAX_STRING_BYTES + 1));
        assert_eq!(
            code(long_key.as_bytes()),
            Code::LimitExceeded(Limit::StringLength)
        );
    }

    #[test]
    fn duplicate_keys_and_bom_are_rejected() {
        assert_eq!(code(br#"{"a":1,"a":2}"#), Code::DuplicateKey);
        assert_eq!(code(b"\xEF\xBB\xBF{}"), Code::ByteOrderMark);
    }
}
