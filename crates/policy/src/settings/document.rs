//! One document of one level: strict parsing, then validation against the embedded schema.
//!
//! The same criterion of validity for every consumer (ADR-GRP-007, ADR-GRD-004 § 1):
//! - invalid JSON, a wrong type or range, a limit exceeded → the whole source is `Ignored`
//!   (PQ-8);
//! - an unknown key, or a key in a level that does not admit it → only that key is dropped, with
//!   a diagnostic (Q24);
//! - an unknown key or operation inside `permissions` or `policies` → the source is `Partial`
//!   (D12): what is readable applies and the safe minimum is forced.

use std::sync::Arc;

use serde_json::Value;

use super::diagnostic::{Code, Diagnostic, Location, SourceKind, pointer};
use super::model::{Level, Settings};
use super::schema::SCHEMA;
use super::strict;
use crate::guard::glob::{Kind, MAX_PATTERNS, validate};

/// State of one source (ADR-GRP-007, "Estado por fuente del cargador").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    /// No file, no copy of the main branch, unborn `HEAD`, or no entry at the path.
    Absent,
    /// The document is valid. Unknown or out-of-level keys were dropped with a diagnostic.
    Readable,
    /// Unknown keys or operations in `permissions` or `policies` (D12).
    Partial,
    /// Invalid JSON or schema, limits exceeded or not a regular file (PQ-8).
    Ignored,
}

/// A parsed source: its state, its settings (readable or partial) and its diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub status: SourceStatus,
    pub settings: Option<Arc<Settings>>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// A source with no document.
    pub fn absent() -> Self {
        Self {
            status: SourceStatus::Absent,
            settings: None,
            diagnostics: Vec::new(),
        }
    }

    /// A source ignored as a whole, with one diagnostic.
    pub fn ignored(diagnostic: Diagnostic) -> Self {
        Self {
            status: SourceStatus::Ignored,
            settings: None,
            diagnostics: vec![diagnostic],
        }
    }

    /// The settings that apply: those of a readable or partial source.
    pub fn applicable(&self) -> Option<&Settings> {
        self.settings.as_deref()
    }

    /// The same parse, with its diagnostics attributed to `source`.
    pub fn for_source(&self, source: SourceKind) -> Self {
        let mut out = self.clone();
        for d in &mut out.diagnostics {
            *d = d.clone().with_source(source);
        }
        out
    }
}

/// Parse and validate one document of `level`. The entry point for every level: the team
/// sources here, the profile and local files in US-GRP-013 (ADR-GRP-008).
pub fn parse_document(bytes: &[u8], level: Level, source: SourceKind) -> Parsed {
    let mut value = match strict::parse(bytes) {
        Ok(value) => value,
        Err((code, location)) => {
            let mut d = Diagnostic::new(code, source);
            d.location = location;
            return Parsed::ignored(d);
        }
    };
    let mut walk = Walk {
        level,
        diagnostics: Vec::new(),
        invalid: None,
        partial: false,
    };
    walk.node(&SCHEMA, &mut value, &mut Vec::new());
    if let Some((code, path)) = walk.invalid {
        return Parsed::ignored(
            Diagnostic::new(code, source).at(Location::Pointer(pointer(&path))),
        );
    }
    let Ok(settings) = serde_json::from_value::<Settings>(value) else {
        // Unreachable while the schema matches the types (drift test); never fall open.
        return Parsed::ignored(
            Diagnostic::new(Code::WrongType, source).at(Location::Pointer(String::new())),
        );
    };
    Parsed {
        status: if walk.partial {
            SourceStatus::Partial
        } else {
            SourceStatus::Readable
        },
        settings: Some(Arc::new(settings)),
        diagnostics: walk
            .diagnostics
            .into_iter()
            .map(|(code, path)| Diagnostic::new(code, source).at(Location::Pointer(pointer(&path))))
            .collect(),
    }
}

/// Sections where an unknown key makes the source partial (D12).
const OPEN_SECTIONS: [&str; 2] = ["permissions", "policies"];

struct Walk {
    level: Level,
    diagnostics: Vec<(Code, Vec<String>)>,
    invalid: Option<(Code, Vec<String>)>,
    partial: bool,
}

/// What to do with a value after visiting it.
#[derive(PartialEq)]
enum Keep {
    Yes,
    Drop,
}

fn resolve(mut schema: &Value) -> &Value {
    while let Some(Value::String(r)) = schema.get("$ref") {
        let name = r.strip_prefix("#/$defs/").expect("local $ref");
        schema = &SCHEMA["$defs"][name];
    }
    schema
}

/// Whether the array is a list of patterns (`x-gitraptor-patterns`).
fn declared_patterns(declared: &Value, schema: &Value) -> bool {
    [declared, schema]
        .iter()
        .any(|n| n.get("x-gitraptor-patterns").and_then(Value::as_bool) == Some(true))
}

fn levels(schema: &Value) -> Option<Vec<&str>> {
    schema
        .get("x-gitraptor-levels")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
}

impl Walk {
    fn fail(&mut self, code: Code, path: &[String]) -> Keep {
        if self.invalid.is_none() {
            self.invalid = Some((code, path.to_vec()));
        }
        Keep::Drop
    }

    fn note(&mut self, code: Code, path: &[String]) {
        self.diagnostics.push((code, path.to_vec()));
    }

    fn open_section(path: &[String]) -> Option<&str> {
        path.first()
            .map(String::as_str)
            .filter(|s| OPEN_SECTIONS.contains(s))
    }

    /// A key that declares discovery roots, at the top or in `engine` (ADR-GRP-010 N7).
    fn roots_key(path: &[String]) -> bool {
        const KEYS: &[&str] = &["discovery", "codeRoots", "roots"];
        match path {
            [key] => KEYS.contains(&key.as_str()),
            [section, key] => section == "engine" && KEYS.contains(&key.as_str()),
            _ => false,
        }
    }

    fn node(&mut self, declared: &Value, value: &mut Value, path: &mut Vec<String>) -> Keep {
        if self.invalid.is_some() {
            return Keep::Drop;
        }
        let schema = resolve(declared);
        for node in [declared, schema] {
            if let Some(levels) = levels(node)
                && !levels.contains(&self.level.as_str())
            {
                self.note(Code::KeyNotAllowedAtLevel, path);
                return Keep::Drop;
            }
        }
        let expected = schema.get("type").and_then(Value::as_str).unwrap_or("");
        match (expected, &mut *value) {
            ("object", Value::Object(map)) => {
                let properties = schema.get("properties");
                let keys: Vec<String> = map.keys().cloned().collect();
                for key in keys {
                    path.push(key.clone());
                    let keep = match properties.and_then(|p| p.get(&key)) {
                        Some(child) => {
                            let v = map.get_mut(&key).expect("key exists");
                            self.node(child, v, path)
                        }
                        None => {
                            match Self::open_section(path) {
                                Some("policies") => {
                                    self.partial = true;
                                    self.note(Code::PolicyNotSupported, path);
                                }
                                Some(_) => {
                                    self.partial = true;
                                    self.note(Code::UnknownKey, path);
                                }
                                None if Self::roots_key(path) => {
                                    self.note(Code::DiscoveryRootsIgnored, path);
                                }
                                None => self.note(Code::UnknownKey, path),
                            }
                            Keep::Drop
                        }
                    };
                    path.pop();
                    if keep == Keep::Drop {
                        map.remove(&key);
                    }
                }
                Keep::Yes
            }
            ("array", Value::Array(items)) if declared_patterns(declared, schema) => {
                // A list of patterns: more than the limit is a limit exceeded (PQ-8); an invalid
                // one is dropped alone, the others still apply (US-GRD-008, D1).
                if items.len() > MAX_PATTERNS {
                    return self.fail(Code::OutOfRange, path);
                }
                let kind = if path.iter().any(|p| p == "protectedBranches") {
                    Kind::Branch
                } else {
                    Kind::Path
                };
                let mut kept = Vec::with_capacity(items.len());
                for (i, item) in std::mem::take(items).into_iter().enumerate() {
                    path.push(i.to_string());
                    match &item {
                        Value::String(raw) if validate(raw, kind).is_ok() => kept.push(item),
                        Value::String(_) => {
                            self.partial = true;
                            self.note(Code::PolicyInvalid, path);
                        }
                        _ => {
                            path.pop();
                            return self.fail(Code::WrongType, path);
                        }
                    }
                    path.pop();
                }
                *items = kept;
                Keep::Yes
            }
            ("array", Value::Array(items)) => {
                let item_schema = schema.get("items").cloned().unwrap_or(Value::Null);
                let mut kept = Vec::with_capacity(items.len());
                for (i, mut item) in std::mem::take(items).into_iter().enumerate() {
                    path.push(i.to_string());
                    if self.node(&item_schema, &mut item, path) == Keep::Yes {
                        kept.push(item);
                    }
                    path.pop();
                }
                *items = kept;
                Keep::Yes
            }
            ("string", Value::String(s)) => {
                let Some(allowed) = schema.get("enum").and_then(Value::as_array) else {
                    return Keep::Yes;
                };
                if allowed.iter().any(|a| a == s.as_str()) {
                    return Keep::Yes;
                }
                if Self::open_section(path) == Some("policies")
                    && path.last().is_some_and(|k| k == "appliesTo")
                {
                    // Who a rule applies to: a value this version does not know is read as the
                    // stricter one, never as the default that would silently let the person
                    // through (US-GRD-008, D1).
                    *s = "everyone".to_owned();
                    self.partial = true;
                    self.note(Code::PolicyInvalid, path);
                    Keep::Yes
                } else if Self::open_section(path).is_some() {
                    // An operation a newer binary knows: drop it, keep the rest (D12).
                    self.partial = true;
                    self.note(Code::UnknownOperation, path);
                    Keep::Drop
                } else {
                    self.fail(Code::WrongType, path)
                }
            }
            ("integer", Value::Number(n)) => {
                let Some(n) = n.as_i64().map(i128::from).or(n.as_u64().map(i128::from)) else {
                    return self.fail(Code::WrongType, path);
                };
                let bound = |k: &str| schema.get(k).and_then(Value::as_i64).map(i128::from);
                if bound("minimum").is_some_and(|min| n < min)
                    || bound("maximum").is_some_and(|max| n > max)
                {
                    return self.fail(Code::OutOfRange, path);
                }
                Keep::Yes
            }
            ("boolean", Value::Bool(_)) => Keep::Yes,
            _ => self.fail(Code::WrongType, path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::Operation;

    fn team(json: &str) -> Parsed {
        parse_document(json.as_bytes(), Level::Team, SourceKind::Floor)
    }

    fn codes(p: &Parsed) -> Vec<&'static str> {
        p.diagnostics.iter().map(|d| d.code.as_str()).collect()
    }

    fn at(p: &Parsed) -> Vec<String> {
        p.diagnostics
            .iter()
            .map(|d| match &d.location {
                Some(Location::Pointer(s)) => s.clone(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn readable_team_document() {
        let p = team(
            r#"{"$schema":"x","engine":{"baseBranch":"develop"},
               "permissions":{"deny":["push"],"allow":["commit"],"disableSafeMinimum":true}}"#,
        );
        assert_eq!(p.status, SourceStatus::Readable, "{:?}", p.diagnostics);
        let s = p.applicable().unwrap();
        assert_eq!(
            s.engine.as_ref().unwrap().base_branch.as_deref(),
            Some("develop")
        );
        let perms = s.permissions.as_ref().unwrap();
        assert_eq!(perms.deny.as_deref(), Some(&[Operation::Push][..]));
        assert_eq!(perms.disable_safe_minimum, Some(true));
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn invalid_json_ignores_the_whole_level_with_a_position() {
        let p = team("{\"engine\": {\"baseBranch\": }}");
        assert_eq!(p.status, SourceStatus::Ignored);
        assert!(p.settings.is_none());
        assert!(matches!(
            p.diagnostics[0].location,
            Some(Location::Position { line: 1, .. })
        ));
    }

    #[test]
    fn wrong_type_or_range_ignores_the_whole_level_with_a_pointer() {
        let p = parse_document(
            br#"{"engine":{"idleThresholdMinutes":0}}"#,
            Level::Profile,
            SourceKind::Profile,
        );
        assert_eq!(p.status, SourceStatus::Ignored);
        assert_eq!(codes(&p), ["out-of-range"]);
        assert_eq!(at(&p), ["/engine/idleThresholdMinutes"]);

        for doc in [
            r#"{"engine":{"baseBranch":7}}"#,
            r#"{"engine":{"baseBranch":null}}"#,
            r#"{"permissions":{"deny":[1]}}"#,
            r#"{"permissions":{"deny":"push"}}"#,
            r#"{"permissions":{"disableSafeMinimum":"yes"}}"#,
            r#"[]"#,
        ] {
            let p = team(doc);
            assert_eq!(p.status, SourceStatus::Ignored, "{doc}");
            assert_eq!(codes(&p), ["wrong-type"], "{doc}");
        }
        let p = parse_document(
            br#"{"engine":{"idleThresholdMinutes":1.5}}"#,
            Level::Local,
            SourceKind::Local,
        );
        assert_eq!(codes(&p), ["wrong-type"]);
    }

    #[test]
    fn discovery_roots_ignored_in_any_settings_file() {
        let p = team(
            r#"{"discovery":{"roots":["~/proyectos"]},"codeRoots":["/x"],
               "engine":{"roots":["/y"],"baseBranch":"dev"}}"#,
        );
        assert_eq!(p.status, SourceStatus::Readable);
        assert_eq!(
            codes(&p),
            [
                "discovery-roots-ignored",
                "discovery-roots-ignored",
                "discovery-roots-ignored"
            ]
        );
        let engine = p.applicable().unwrap().engine.clone().unwrap();
        assert_eq!(engine.base_branch.as_deref(), Some("dev"));
    }

    #[test]
    fn unknown_key_is_dropped_and_the_level_applies() {
        let p = team(r#"{"engin":{"baseBranch":"x"},"engine":{"baseBranch":"dev","other":1}}"#);
        assert_eq!(p.status, SourceStatus::Readable);
        assert_eq!(codes(&p), ["unknown-key", "unknown-key"]);
        assert_eq!(at(&p), ["/engin", "/engine/other"]);
        let engine = p.applicable().unwrap().engine.clone().unwrap();
        assert_eq!(engine.base_branch.as_deref(), Some("dev"));
    }

    #[test]
    fn key_out_of_level_is_dropped_with_a_diagnostic() {
        let p = team(
            r#"{"engine":{"idleThresholdMinutes":10,"gitPath":"/x","watcher":{"fallbackPollSeconds":30}},
               "timeMachine":{"retentionDays":3}}"#,
        );
        assert_eq!(p.status, SourceStatus::Readable);
        assert_eq!(
            codes(&p),
            [
                "key-not-allowed-at-level",
                "key-not-allowed-at-level",
                "key-not-allowed-at-level",
                "key-not-allowed-at-level"
            ]
        );
        assert_eq!(
            p.applicable()
                .unwrap()
                .engine
                .as_ref()
                .unwrap()
                .idle_threshold_minutes,
            None
        );

        let p = parse_document(
            br#"{"engine":{"baseBranch":"dev","idleThresholdMinutes":15},"permissions":{"disableSafeMinimum":true}}"#,
            Level::Local,
            SourceKind::Local,
        );
        assert_eq!(
            at(&p),
            ["/engine/baseBranch", "/permissions/disableSafeMinimum"]
        );
        let s = p.applicable().unwrap();
        assert_eq!(s.engine.as_ref().unwrap().idle_threshold_minutes, Some(15));
        assert_eq!(s.engine.as_ref().unwrap().base_branch, None);
    }

    /// TS-GRP-006 (ADR-GRP-010, Enmienda 2026-10-07, N7): the observation
    /// tiers are never read from the team level; the threshold is also local,
    /// the two intervals only profile.
    #[test]
    fn observation_keys_follow_their_levels() {
        let doc = br#"{"engine":{"observation":{"dormantAfterHours":2,"dormantPollSeconds":60,"dormantReconcileMinutes":30}}}"#;
        let p = parse_document(doc, Level::Team, SourceKind::Team);
        assert_eq!(
            at(&p),
            [
                "/engine/observation/dormantAfterHours",
                "/engine/observation/dormantPollSeconds",
                "/engine/observation/dormantReconcileMinutes"
            ]
        );
        let p = parse_document(doc, Level::Local, SourceKind::Local);
        assert_eq!(
            at(&p),
            [
                "/engine/observation/dormantPollSeconds",
                "/engine/observation/dormantReconcileMinutes"
            ]
        );
        let o = p
            .applicable()
            .unwrap()
            .engine
            .clone()
            .unwrap()
            .observation
            .unwrap();
        assert_eq!(o.dormant_after_hours, Some(2));
        let p = parse_document(doc, Level::Profile, SourceKind::Profile);
        assert!(codes(&p).is_empty());
        let o = p
            .applicable()
            .unwrap()
            .engine
            .clone()
            .unwrap()
            .observation
            .unwrap();
        assert_eq!(
            (o.dormant_poll_seconds, o.dormant_reconcile_minutes),
            (Some(60), Some(30))
        );
        // Out of range: 0 hours never sleeps a repo by accident.
        let p = parse_document(
            br#"{"engine":{"observation":{"dormantAfterHours":0}}}"#,
            Level::Profile,
            SourceKind::Profile,
        );
        assert_eq!(at(&p), ["/engine/observation/dormantAfterHours"]);
    }

    #[test]
    fn unknown_key_or_operation_in_permissions_or_policies_is_partial() {
        let p = team(r#"{"permissions":{"deny":["push","tag-delete"],"disableSafeMinimum":true}}"#);
        assert_eq!(p.status, SourceStatus::Partial);
        assert_eq!(codes(&p), ["unknown-operation"]);
        assert_eq!(at(&p), ["/permissions/deny/1"]);
        let perms = p.applicable().unwrap().permissions.clone().unwrap();
        assert_eq!(perms.deny.as_deref(), Some(&[Operation::Push][..]));

        let p = team(r#"{"permissions":{"denyy":["push"]}}"#);
        assert_eq!(p.status, SourceStatus::Partial);
        assert_eq!(codes(&p), ["unknown-key"]);

        let p = team(r#"{"policies":{"diffSizeLimit":400}}"#);
        assert_eq!(p.status, SourceStatus::Partial);
        assert_eq!(codes(&p), ["policy-not-supported"]);

        // An empty `policies` object is fine.
        assert_eq!(team(r#"{"policies":{}}"#).status, SourceStatus::Readable);
    }

    /// US-GRD-018 § 4: `commitAuthorship` is a supported key in every level.
    #[test]
    fn protected_branches_and_forbidden_paths_are_supported_policies() {
        let json = r#"{"policies":{
            "protectedBranches":{"patterns":["main","release/*"],"appliesTo":"everyone"},
            "forbiddenPaths":{"patterns":["secrets/","*.pem"]}}}"#;
        let p = team(json);
        assert_eq!(p.status, SourceStatus::Readable, "{:?}", p.diagnostics);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        let policies = p.applicable().unwrap().policies.clone().unwrap();
        let branches = policies.protected_branches.unwrap();
        assert_eq!(
            branches.patterns.as_deref(),
            Some(&["main".to_owned(), "release/*".to_owned()][..])
        );
        assert_eq!(
            branches.applies_to,
            Some(crate::settings::model::AppliesTo::Everyone)
        );
        let paths = policies.forbidden_paths.unwrap();
        assert_eq!(paths.applies_to, None);
        assert_eq!(paths.patterns.unwrap().len(), 2);
        // The three personal and team levels admit them.
        for level in [Level::Profile, Level::Local] {
            let p = parse_document(json.as_bytes(), level, SourceKind::Profile);
            assert!(p.diagnostics.is_empty(), "{level:?} {:?}", p.diagnostics);
        }
    }

    #[test]
    fn an_invalid_pattern_is_dropped_alone_and_the_source_is_partial() {
        let p = team(
            r##"{"policies":{
                "protectedBranches":{"patterns":["main","refs/heads/x","","!y"]},
                "forbiddenPaths":{"patterns":["#c","secrets/","a\u0007b",
                                              "a//b"]}}}"##,
        );
        assert_eq!(p.status, SourceStatus::Partial);
        assert_eq!(
            codes(&p),
            ["policy-invalid"; 6].to_vec(),
            "{:?}",
            p.diagnostics
        );
        assert_eq!(
            at(&p),
            [
                "/policies/forbiddenPaths/patterns/0",
                "/policies/forbiddenPaths/patterns/2",
                "/policies/forbiddenPaths/patterns/3",
                "/policies/protectedBranches/patterns/1",
                "/policies/protectedBranches/patterns/2",
                "/policies/protectedBranches/patterns/3",
            ]
        );
        let policies = p.applicable().unwrap().policies.clone().unwrap();
        assert_eq!(
            policies.protected_branches.unwrap().patterns.unwrap(),
            ["main"]
        );
        // `a//b` is invalid as well: the one that is left is `secrets/`.
        assert!(
            policies
                .forbidden_paths
                .unwrap()
                .patterns
                .unwrap()
                .contains(&"secrets/".to_owned())
        );
    }

    #[test]
    fn limits_and_wrong_types_ignore_the_source_like_any_other_value() {
        let many: Vec<String> = (0..65).map(|i| format!("\"b{i}\"")).collect();
        let p = team(&format!(
            r#"{{"policies":{{"protectedBranches":{{"patterns":[{}]}}}}}}"#,
            many.join(",")
        ));
        assert_eq!(p.status, SourceStatus::Ignored);
        assert_eq!(codes(&p), ["out-of-range"]);
        let long = "a".repeat(257);
        let p = team(&format!(
            r#"{{"policies":{{"forbiddenPaths":{{"patterns":["{long}"]}}}}}}"#
        ));
        assert_eq!(p.status, SourceStatus::Partial);
        assert_eq!(codes(&p), ["policy-invalid"]);
        for bad in [
            r#"{"policies":{"forbiddenPaths":{"patterns":"secrets/"}}}"#,
            r#"{"policies":{"forbiddenPaths":{"patterns":[1]}}}"#,
            r#"{"policies":{"protectedBranches":["main"]}}"#,
        ] {
            let p = team(bad);
            assert_eq!(p.status, SourceStatus::Ignored, "{bad}");
        }
        // A value of `appliesTo` this version does not know is read as the stricter one
        // (`everyone`), never as the default: the source is partial and the minimum is forced.
        for typo in ["robots", "Everyone", "all"] {
            let p = team(&format!(
                r#"{{"policies":{{"forbiddenPaths":{{"appliesTo":"{typo}"}}}}}}"#
            ));
            assert_eq!(p.status, SourceStatus::Partial, "{typo}");
            assert_eq!(codes(&p), ["policy-invalid"]);
            let paths = p.applicable().unwrap().policies.clone().unwrap();
            assert_eq!(
                paths.forbidden_paths.unwrap().applies_to,
                Some(crate::settings::model::AppliesTo::Everyone)
            );
        }
    }

    #[test]
    fn commit_authorship_is_a_supported_policy() {
        use crate::settings::model::{AuthorshipMode, OnAgentCommit};
        let doc =
            br#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#;
        for (level, source) in [
            (Level::Team, SourceKind::Floor),
            (Level::Profile, SourceKind::Profile),
            (Level::Local, SourceKind::Local),
        ] {
            let p = parse_document(doc, level, source);
            assert_eq!(p.status, SourceStatus::Readable, "{:?}", p.diagnostics);
            assert!(p.diagnostics.is_empty());
            let ca = p
                .applicable()
                .unwrap()
                .policies
                .clone()
                .unwrap()
                .commit_authorship
                .unwrap();
            assert_eq!(ca.mode, Some(AuthorshipMode::HumanAuthor));
            assert_eq!(ca.on_agent_commit, Some(OnAgentCommit::Warn));
        }
    }

    /// An unknown value leaves the source partial and the key without effect (D12).
    #[test]
    fn commit_authorship_unknown_value_is_partial() {
        let p = team(r#"{"policies":{"commitAuthorship":{"mode":"robots-only"}}}"#);
        assert_eq!(p.status, SourceStatus::Partial);
        assert_eq!(codes(&p), ["unknown-operation"]);
        let ca = p
            .applicable()
            .unwrap()
            .policies
            .clone()
            .unwrap()
            .commit_authorship
            .unwrap();
        assert_eq!(ca.mode, None);

        let p = team(r#"{"policies":{"commitAuthorship":{"mode":"flexible","extra":1}}}"#);
        assert_eq!(p.status, SourceStatus::Partial);
        // Unknown keys anywhere under `policies` are reported as an unsupported policy.
        assert_eq!(codes(&p), ["policy-not-supported"]);

        let p = team(r#"{"policies":{"commitAuthorship":{"mode":7}}}"#);
        assert_eq!(p.status, SourceStatus::Ignored);
    }

    #[test]
    fn diagnostics_never_carry_values() {
        let secret = "ghp_SECRETVALUE";
        for doc in [
            format!(r#"{{"engine":{{"baseBranch":{{"x":"{secret}"}}}}}}"#),
            format!(r#"{{"engine":{{"idleThresholdMinutes":"{secret}"}}}}"#),
            format!(r#"{{"permissions":{{"deny":["{secret}"]}}}}"#),
            format!(r#"{{"x": {secret}}}"#),
        ] {
            let p = team(&doc);
            assert!(!format!("{:?}", p.diagnostics).contains(secret), "{doc}");
        }
    }
}
