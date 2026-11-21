//! US-MCP-023 (DS-US-MCP-023 T002): the compatibility rules of the CLI's JSON contract (D9), the
//! snapshot of the agent view's output schemas and the agent reference generated from them.

use std::path::PathBuf;

use gitraptor_api::cli_contract::{
    CLI_SCHEMA_MAJOR, CLI_SCHEMA_MINOR, CLI_SCHEMA_VERSION, Change, CliErrorCode, CliOnlyCode,
    agent_output_schemas, compat, stable_codes,
};
use gitraptor_api::mcp_view::McpToolError;
use serde_json::{Value, json};

/// An object schema, already flattened (no `$ref`): `properties` and `required`.
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required})
}

fn base() -> Value {
    object(
        json!({
            "branch": {"type": "string"},
            "state": {"type": "string", "enum": ["idle", "busy", "stuck"]},
            "count": {"type": "integer"},
            "nested": object(json!({"inner": {"type": "string"}}), &["inner"]),
        }),
        &["branch", "state", "count"],
    )
}

fn without(mut schema: Value, field: &str) -> Value {
    schema["properties"].as_object_mut().unwrap().remove(field);
    schema["required"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r != field);
    schema
}

fn is_breaking(change: &Change) -> bool {
    matches!(change, Change::Breaking(_))
}

#[test]
fn compat_removing_or_renaming_a_field_is_breaking() {
    let old = base();
    let removed = without(base(), "branch");
    assert!(
        is_breaking(&compat(&old, &removed)),
        "removing a field is breaking: {:?}",
        compat(&old, &removed)
    );

    let mut renamed = without(base(), "branch");
    renamed["properties"]["branch_name"] = json!({"type": "string"});
    renamed["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("branch_name"));
    assert!(
        is_breaking(&compat(&old, &renamed)),
        "renaming a field is breaking: {:?}",
        compat(&old, &renamed)
    );

    let nested_removed = {
        let mut s = base();
        s["properties"]["nested"] = object(json!({}), &[]);
        s
    };
    assert!(
        is_breaking(&compat(&old, &nested_removed)),
        "removing a nested field is breaking: {:?}",
        compat(&old, &nested_removed)
    );
    assert_eq!(compat(&old, &base()), Change::None, "no change is None");
}

#[test]
fn compat_adding_a_field_is_additive() {
    let old = base();
    let mut new = base();
    new["properties"]["extra"] = json!({"type": "string"});
    match compat(&old, &new) {
        Change::Additive(paths) => assert!(
            paths.iter().any(|p| p.contains("extra")),
            "the added path is named: {paths:?}"
        ),
        other => panic!("adding a field is additive, got {other:?}"),
    }

    let mut with_enum = base();
    with_enum["properties"]["state"]["enum"] = json!(["idle", "busy", "stuck", "gone"]);
    assert!(
        matches!(compat(&old, &with_enum), Change::Additive(_)),
        "adding an enum value is additive: {:?}",
        compat(&old, &with_enum)
    );
}

#[test]
fn compat_narrowing_a_type_or_making_optional_is_breaking() {
    let old = base();

    let mut retyped = base();
    retyped["properties"]["count"] = json!({"type": "string"});
    assert!(
        is_breaking(&compat(&old, &retyped)),
        "changing a type is breaking: {:?}",
        compat(&old, &retyped)
    );

    let mut narrowed = base();
    narrowed["properties"]["count"] = json!({"type": ["integer", "null"]});
    let widened = compat(&narrowed, &old);
    assert!(
        is_breaking(&widened),
        "narrowing a type (integer|null to integer) is breaking: {widened:?}"
    );

    let mut optional = base();
    optional["required"] = json!(["state", "count"]);
    assert!(
        is_breaking(&compat(&old, &optional)),
        "a required field made optional is breaking: {:?}",
        compat(&old, &optional)
    );
}

#[test]
fn compat_removing_an_enum_value_is_breaking() {
    let old = base();
    let mut new = base();
    new["properties"]["state"]["enum"] = json!(["idle", "busy"]);
    assert!(
        is_breaking(&compat(&old, &new)),
        "removing an enum value is breaking: {:?}",
        compat(&old, &new)
    );
}

#[test]
fn the_version_constants_agree() {
    assert_eq!(
        CLI_SCHEMA_VERSION,
        format!("{CLI_SCHEMA_MAJOR}.{CLI_SCHEMA_MINOR}"),
        "CLI_SCHEMA_VERSION is MAJOR.MINOR"
    );
}

fn kebab(code: &str) -> bool {
    !code.is_empty()
        && code
            .split('-')
            .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase()))
}

#[test]
fn cli_only_codes_are_kebab_and_disjoint_from_the_mcp_codes() {
    let cli_only = [
        (CliOnlyCode::CommandUnavailable, "command-unavailable", 1),
        (CliOnlyCode::ReservedCommand, "reserved-command", 1),
        (CliOnlyCode::InvalidArgs, "invalid-args", 2),
        (CliOnlyCode::CommandFailed, "command-failed", 1),
    ];
    let mcp: Vec<&str> = McpToolError::ALL.iter().map(|c| c.as_str()).collect();
    for (code, name, exit) in cli_only {
        let wrapped = CliErrorCode::Cli(code);
        assert_eq!(wrapped.as_str(), name, "the wire name of {code:?}");
        assert!(kebab(wrapped.as_str()), "{name} is kebab-case");
        assert!(
            !mcp.contains(&wrapped.as_str()),
            "{name} must not collide with an MCP code"
        );
        assert_eq!(wrapped.exit_code(), exit, "the exit code of {name}");
        assert!(
            CliErrorCode::ALL.contains(&wrapped),
            "CliErrorCode::ALL lists {name}: {:?}",
            CliErrorCode::ALL
        );
    }
    for code in CliErrorCode::ALL {
        if let CliErrorCode::Tool(tool) = code {
            assert!(McpToolError::ALL.contains(tool), "{tool:?} is an MCP code");
            assert_eq!(code.as_str(), tool.as_str(), "a reused code keeps its name");
        }
    }
    let mut names: Vec<&str> = CliErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "every code of ALL is unique");
    for reused in [
        "engine-unavailable",
        "repo-not-enabled",
        "confirmation-required",
    ] {
        assert!(
            names.contains(&reused),
            "ALL carries the reused MCP code {reused}"
        );
    }
}

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas/cli-contract.json")
}

fn reference_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/architecture/design/cli-json-contract.md")
}

fn version_of(text: &str) -> (u16, u16) {
    let (major, minor) = text
        .split_once('.')
        .unwrap_or_else(|| panic!("a version is MAJOR.MINOR: {text:?}"));
    (major.parse().unwrap(), minor.parse().unwrap())
}

/// The agent view's outputs of D10.
const OUTPUTS: [&str; 6] = [
    "status@agent",
    "undo@agent",
    "redo@agent",
    "agent-register",
    "daemon-status@agent",
    "agent-withdraw",
];

/// Snapshot format (assumed, DS T002 does not fix it): `{"schema_version": "M.m",
/// "outputs": {"<name>": <schema>}, "codes": [...]}`.
#[test]
fn the_schemas_match_the_snapshot_for_this_version() {
    let raw = std::fs::read_to_string(snapshot_path()).unwrap_or_else(|e| {
        panic!(
            "the snapshot crates/api/schemas/cli-contract.json does not exist yet ({e}); \
             regenerate with GITRAPTOR_BLESS_CLI_CONTRACT=1"
        )
    });
    let snapshot: Value = serde_json::from_str(&raw).expect("the snapshot is JSON");
    let (old_major, old_minor) = version_of(snapshot["schema_version"].as_str().unwrap_or("?"));

    let current = agent_output_schemas();
    let names: Vec<&str> = current.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, OUTPUTS, "the outputs of D10, in a fixed order");

    let mut breaking = false;
    let mut additive = false;
    let mut notes = Vec::new();
    for (name, schema) in &current {
        let old = snapshot["outputs"]
            .get(*name)
            .unwrap_or_else(|| panic!("the snapshot has no schema for {name}"));
        match compat(old, schema) {
            Change::None => {}
            Change::Additive(paths) => {
                additive = true;
                notes.push(format!("{name}: additive {paths:?}"));
            }
            Change::Breaking(paths) => {
                breaking = true;
                notes.push(format!("{name}: breaking {paths:?}"));
            }
        }
    }
    let (major, minor) = (CLI_SCHEMA_MAJOR, CLI_SCHEMA_MINOR);
    if breaking {
        assert!(
            major > old_major,
            "a breaking change needs a major bump: {notes:?}"
        );
    } else if additive {
        assert!(
            major > old_major || (major == old_major && minor > old_minor),
            "an additive change needs a minor bump: {notes:?}"
        );
    } else {
        assert_eq!(
            (major, minor),
            (old_major, old_minor),
            "no change in the schemas, the version stays"
        );
    }
    assert!(
        !breaking && !additive || (major, minor) != (old_major, old_minor),
        "regenerate with GITRAPTOR_BLESS_CLI_CONTRACT=1 once the version is bumped"
    );
    let stored: Vec<(&str, &Value)> = current.iter().map(|(n, s)| (*n, s)).collect();
    for (name, schema) in stored {
        assert_eq!(
            &snapshot["outputs"][name], schema,
            "{name}: the snapshot differs from the schema of this version; \
             regenerate with GITRAPTOR_BLESS_CLI_CONTRACT=1"
        );
    }
}

#[test]
fn each_output_schema_describes_its_data_not_the_envelope() {
    let schemas = agent_output_schemas();
    assert_eq!(
        schemas.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        OUTPUTS,
        "the six outputs of D10, in a fixed order"
    );
    for (name, schema) in &schemas {
        let properties = schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: the schema describes an object: {schema}"));
        assert!(!properties.is_empty(), "{name}: the schema has fields");
        for envelope in ["schema_version", "data", "error"] {
            assert!(
                !properties.contains_key(envelope),
                "{name}: the schema describes data, not the envelope ({envelope}): {schema}"
            );
        }
    }
}

/// Every field name (any depth) of a schema.
fn field_names(schema: &Value, out: &mut Vec<String>) {
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (key, value) in props {
            out.push(key.clone());
            field_names(value, out);
        }
    }
}

fn reference() -> String {
    std::fs::read_to_string(reference_path()).unwrap_or_else(|e| {
        panic!(
            "docs/architecture/design/cli-json-contract.md does not exist yet ({e}); \
             it is generated with GITRAPTOR_BLESS_CLI_CONTRACT=1"
        )
    })
}

#[test]
fn the_agent_reference_is_generated_from_the_schemas() {
    let doc = reference();
    let schemas = agent_output_schemas();
    assert!(!schemas.is_empty(), "there are output schemas to document");
    for (name, schema) in &schemas {
        assert!(doc.contains(name), "the reference documents {name}");
        let mut fields = Vec::new();
        field_names(schema, &mut fields);
        for field in fields {
            assert!(
                doc.contains(&field),
                "the reference lists the field {field} of {name}; regenerate it"
            );
        }
    }
    for code in stable_codes() {
        assert!(doc.contains(code), "the reference lists the code {code}");
    }
    assert!(
        doc.contains(CLI_SCHEMA_VERSION),
        "the reference states the version {CLI_SCHEMA_VERSION}"
    );
}

#[test]
fn every_agent_output_lists_fields_version_and_codes() {
    let doc = reference();
    let codes = stable_codes();
    assert!(!codes.is_empty(), "there are stable codes to document");
    let sections: Vec<&str> = doc.split("\n## ").collect();
    for (name, schema) in agent_output_schemas() {
        let section = sections
            .iter()
            .find(|s| s.lines().next().is_some_and(|h| h.contains(name)))
            .unwrap_or_else(|| panic!("the reference has a section for {name}"));
        let mut fields = Vec::new();
        field_names(&schema, &mut fields);
        assert!(!fields.is_empty(), "{name}: the schema has fields");
        for field in &fields {
            assert!(
                section.contains(field.as_str()),
                "{name}: its section lists the field {field}"
            );
        }
        assert!(
            section.contains(CLI_SCHEMA_VERSION),
            "{name}: its section states the version"
        );
        assert!(
            codes.iter().any(|c| section.contains(c)),
            "{name}: its section lists the error codes it can return"
        );
    }
}
