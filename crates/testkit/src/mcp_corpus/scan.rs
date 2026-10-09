//! Scanners over what the server printed.

use serde_json::Value;

use super::judge::Secrets;

/// Whether `c` is invisible or a control character a terminal could act on.
pub fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            u32::from(c),
            0x034F
                | 0x061C
                | 0x115F
                | 0x1160
                | 0x200B..=0x200F
                | 0x2028..=0x202E
                | 0x2060..=0x2069
                | 0x3164
                | 0xFE00..=0xFE0F
                | 0xFEFF
                | 0xFFA0
                | 0xE0000..=0xE007F
        )
}

/// JSON pointers of every string value or key holding a hidden character.
pub fn hidden_characters(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    walk(value, &mut String::new(), &mut found);
    found
}

fn walk(value: &Value, pointer: &mut String, found: &mut Vec<String>) {
    match value {
        Value::String(text) if text.chars().any(is_hidden) => found.push(pointer.clone()),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let length = pointer.len();
                pointer.push('/');
                pointer.push_str(&index.to_string());
                walk(item, pointer, found);
                pointer.truncate(length);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let length = pointer.len();
                pointer.push('/');
                pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                if key.chars().any(is_hidden) {
                    found.push(pointer.clone());
                }
                walk(item, pointer, found);
                pointer.truncate(length);
            }
        }
        _ => {}
    }
}

/// Names of the planted secrets found in `text`, each once. A value is also looked for in its
/// JSON-escaped form, since the server prints JSON.
pub fn secrets_in(text: &str, secrets: &Secrets) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (name, value) in &secrets.0 {
        if value.is_empty() || names.contains(name) {
            continue;
        }
        let escaped = serde_json::to_string(value)
            .ok()
            .and_then(|quoted| {
                quoted
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        if text.contains(value.as_str()) || (!escaped.is_empty() && text.contains(&escaped)) {
            names.push(name.clone());
        }
    }
    names
}

/// Prefixes that must be followed by at least 16 characters of `[A-Za-z0-9_-]`.
const PREFIXED: [&str; 10] = [
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "sk-ant-",
];

/// Known credential shapes found in `text`, each once.
pub fn token_shapes(text: &str) -> Vec<&'static str> {
    let mut shapes = Vec::new();
    for prefix in PREFIXED {
        let hit = text.match_indices(prefix).any(|(at, _)| {
            text[at + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                .take(16)
                .count()
                == 16
        });
        if hit {
            shapes.push(prefix);
        }
    }
    let aws = text.match_indices("AKIA").any(|(at, _)| {
        text[at + 4..]
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            .take(16)
            .count()
            == 16
    });
    if aws {
        shapes.push("AKIA");
    }
    let pem = text.match_indices("-----BEGIN ").any(|(at, _)| {
        let rest = &text[at..];
        let line = rest.split('\n').next().unwrap_or(rest);
        line.contains("PRIVATE KEY")
    });
    if pem {
        shapes.push("-----BEGIN PRIVATE KEY");
    }
    shapes
}

/// Checks that stderr only carries fixed `raptor-mcp: <code>` lines.
///
/// # Errors
/// The 1-based number of the first bad line.
pub fn stderr_fixed_codes(stderr: &str) -> Result<(), usize> {
    for (index, line) in stderr.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let fixed = line.strip_prefix("raptor-mcp: ").is_some_and(|code| {
            let mut chars = code.chars();
            chars.next().is_some_and(|c| c.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c == '-')
        });
        if !fixed {
            return Err(index + 1);
        }
    }
    Ok(())
}
