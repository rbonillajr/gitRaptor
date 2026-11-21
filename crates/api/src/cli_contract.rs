//! The JSON contract of the `raptor` CLI for agents: the version, the envelope every
//! `--json` output travels in, the stable error codes, the output types the CLI builds
//! itself, and the classifier that decides how a schema change bumps the version.
//!
//! The wire form is `{"schema_version": "MAJOR.MINOR", "data": …}` or
//! `{"schema_version": "MAJOR.MINOR", "error": {code, message, params, action}}`. A
//! breaking change raises the major, an additive one the minor ([`compat`]).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::mcp_view::McpToolError;
use crate::messages::{EngineStateView, RegistrationWithdrawResult, RepoStateView};
use crate::methods::McpStatus;
use crate::scope::{AutostartView, ConnectionRequester};
use crate::timemachine::{McpRedoResult, McpUndoResult};
use crate::untrusted::{Untrusted, UntrustedName};

pub const CLI_SCHEMA_MAJOR: u16 = 1;
pub const CLI_SCHEMA_MINOR: u16 = 0;
pub const CLI_SCHEMA_VERSION: &str = "1.0";

/// The success envelope: `data` is the output of the subcommand.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CliOk<T> {
    pub schema_version: &'static str,
    pub data: T,
}

/// The rejection envelope.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CliErr {
    pub schema_version: &'static str,
    pub error: CliError,
}

/// Why a command gives no data. `message` and `action` are fixed texts per code; `params`
/// is always an object (`{}` when there is nothing to say).
#[derive(Debug, Serialize, JsonSchema)]
pub struct CliError {
    pub code: CliErrorCode,
    pub message: String,
    pub params: Value,
    pub action: String,
}

/// Codes only the CLI emits (the MCP never does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliOnlyCode {
    /// The command is not available to this caller: `params.reason` is `agent-view` or
    /// `engine-version`.
    CommandUnavailable,
    /// A command reserved to the developer: `params.reason` is the engine's refusal reason.
    ReservedCommand,
    /// The arguments are not valid: `params.kind` and, when known, `params.argument`.
    InvalidArgs,
    /// The engine refused for a reason with no code of its own: `params.reason`,
    /// `params.detail?`.
    CommandFailed,
}

impl CliOnlyCode {
    pub const ALL: [Self; 4] = [
        Self::CommandUnavailable,
        Self::ReservedCommand,
        Self::InvalidArgs,
        Self::CommandFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommandUnavailable => "command-unavailable",
            Self::ReservedCommand => "reserved-command",
            Self::InvalidArgs => "invalid-args",
            Self::CommandFailed => "command-failed",
        }
    }
}

/// A stable error code of the CLI's JSON: the MCP's or a CLI-only one. It travels as the
/// kebab-case string of [`CliErrorCode::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliErrorCode {
    Tool(McpToolError),
    Cli(CliOnlyCode),
}

impl CliErrorCode {
    /// The codes the CLI emits: nine of the MCP's and the four of its own.
    pub const ALL: &'static [Self] = &[
        Self::Tool(McpToolError::EngineUnavailable),
        Self::Tool(McpToolError::RepoNotEnabled),
        Self::Tool(McpToolError::NotInObservedWorktree),
        Self::Tool(McpToolError::RepoUnavailable),
        Self::Tool(McpToolError::IdentityUnverified),
        Self::Tool(McpToolError::RateLimited),
        Self::Tool(McpToolError::ConfirmationRequired),
        Self::Tool(McpToolError::Aborted),
        Self::Tool(McpToolError::Internal),
        Self::Cli(CliOnlyCode::CommandUnavailable),
        Self::Cli(CliOnlyCode::ReservedCommand),
        Self::Cli(CliOnlyCode::CommandFailed),
        Self::Cli(CliOnlyCode::InvalidArgs),
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tool(code) => code.as_str(),
            Self::Cli(code) => code.as_str(),
        }
    }

    /// 2 for a usage error, 1 for any other rejection.
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Cli(CliOnlyCode::InvalidArgs) => 2,
            _ => 1,
        }
    }
}

impl Serialize for CliErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl JsonSchema for CliErrorCode {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CliErrorCode")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let names: Vec<&str> = Self::ALL.iter().map(|c| c.as_str()).collect();
        json_schema!({"type": "string", "enum": names})
    }
}

/// `raptor agent register --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentRegisterView {
    pub repo_id: String,
    pub session_id: String,
    pub outcome: String,
    pub agent: String,
    pub agent_name: Option<UntrustedName>,
    pub origin: String,
    pub support: String,
}

/// `raptor daemon status --json` on a connection the daemon fixed as `mcp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct McpDaemonStatusView {
    pub engine_state: EngineStateView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<ConnectionRequester>,
}

/// A repo of the engine's list in `raptor daemon status --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonRepoView {
    pub repo_id: String,
    pub state: RepoStateView,
    pub path: Untrusted,
}

/// `raptor daemon status --json` for the developer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonStatusView {
    pub engine_state: EngineStateView,
    pub pid: u32,
    pub protocol: u32,
    pub git_version: Option<Untrusted>,
    pub autostart: Option<AutostartView>,
    pub requester: Option<ConnectionRequester>,
    pub repos: Vec<DaemonRepoView>,
}

/// Every stable code, for the agent guide: the MCP's and the CLI-only ones.
pub fn stable_codes() -> Vec<&'static str> {
    McpToolError::ALL
        .iter()
        .map(|c| c.as_str())
        .chain(CliOnlyCode::ALL.iter().map(|c| c.as_str()))
        .collect()
}

fn schema_of<T: JsonSchema>() -> Value {
    // A schema is plain data: serializing it cannot fail; `Null` keeps the signature total
    // and the snapshot test would flag it.
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or(Value::Null)
}

/// The output schemas of the agent view (the `data` of each, never the envelope), in a
/// fixed order.
pub fn agent_output_schemas() -> Vec<(&'static str, Value)> {
    vec![
        ("status@agent", schema_of::<McpStatus>()),
        ("undo@agent", schema_of::<McpUndoResult>()),
        ("redo@agent", schema_of::<McpRedoResult>()),
        ("agent-register", schema_of::<AgentRegisterView>()),
        ("daemon-status@agent", schema_of::<McpDaemonStatusView>()),
        ("agent-withdraw", schema_of::<RegistrationWithdrawResult>()),
    ]
}

/// How a new schema differs from the published one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    None,
    Additive(Vec<String>),
    Breaking(Vec<String>),
}

/// A schema reduced to what a reader of the output depends on.
#[derive(Debug, Clone, Default)]
struct Node {
    types: BTreeSet<String>,
    enums: BTreeSet<String>,
    props: BTreeMap<String, (Node, bool)>,
    items: Option<Box<Node>>,
}

/// One path of a flattened schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatField {
    pub types: BTreeSet<String>,
    pub required: bool,
    pub enums: BTreeSet<String>,
}

fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    root.pointer(reference.strip_prefix('#')?)
}

/// Merges `b` into `a`: as an intersection of constraints (`allOf`, the keywords of one
/// schema) or as a union of alternatives (`anyOf`, `oneOf`).
fn merge(mut a: Node, b: Node, intersect: bool) -> Node {
    a.types.extend(b.types);
    a.enums.extend(b.enums);
    for (key, (node, required)) in b.props {
        match a.props.remove(&key) {
            Some((mine, mine_required)) => {
                let required = if intersect {
                    mine_required || required
                } else {
                    mine_required && required
                };
                a.props
                    .insert(key, (merge(mine, node, intersect), required));
            }
            None => {
                a.props.insert(key, (node, required));
            }
        }
    }
    a.items = match (a.items.take(), b.items) {
        (Some(x), Some(y)) => Some(Box::new(merge(*x, *y, intersect))),
        (x, y) => x.or(y),
    };
    a
}

fn build(schema: &Value, root: &Value, stack: &mut Vec<String>) -> Node {
    let Some(obj) = schema.as_object() else {
        return Node::default();
    };
    if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
        if stack.iter().any(|s| s == reference) {
            return Node::default();
        }
        let Some(target) = resolve(root, reference) else {
            return Node::default();
        };
        stack.push(reference.to_owned());
        let node = build(target, root, stack);
        stack.pop();
        return node;
    }
    let mut node = Node::default();
    match obj.get("type") {
        Some(Value::String(t)) => {
            node.types.insert(t.clone());
        }
        Some(Value::Array(ts)) => node
            .types
            .extend(ts.iter().filter_map(Value::as_str).map(str::to_owned)),
        _ => {}
    }
    if let Some(Value::Array(values)) = obj.get("enum") {
        node.enums.extend(values.iter().map(Value::to_string));
    }
    if let Some(value) = obj.get("const") {
        node.enums.insert(value.to_string());
    }
    let required: BTreeSet<&str> = obj
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(Value::Object(props)) = obj.get("properties") {
        for (key, value) in props {
            node.props.insert(
                key.clone(),
                (build(value, root, stack), required.contains(key.as_str())),
            );
        }
    }
    if let Some(items @ Value::Object(_)) = obj.get("items") {
        node.items = Some(Box::new(build(items, root, stack)));
    }
    if let Some(Value::Array(all)) = obj.get("allOf") {
        for branch in all {
            node = merge(node, build(branch, root, stack), true);
        }
    }
    for keyword in ["anyOf", "oneOf"] {
        let Some(Value::Array(branches)) = obj.get(keyword) else {
            continue;
        };
        let mut union: Option<Node> = None;
        let mut nullable = false;
        for branch in branches {
            let built = build(branch, root, stack);
            // The `null` of an `Option` says the parent may be absent, not that its fields are
            // optional: it is carried as a type and never touches `required`.
            if built.types.len() == 1
                && built.types.contains("null")
                && built.props.is_empty()
                && built.items.is_none()
            {
                nullable = true;
                continue;
            }
            union = Some(match union {
                None => built,
                Some(acc) => merge(acc, built, false),
            });
        }
        // `required` stays relative to the object that declares the field, so a variant that
        // does not have it neither hides nor invents a change.
        if nullable {
            union
                .get_or_insert_with(Node::default)
                .types
                .insert("null".to_owned());
        }
        if let Some(union) = union {
            node = merge(node, union, true);
        }
    }
    node
}

fn flatten(node: &Node, path: &str, required: bool, out: &mut BTreeMap<String, FlatField>) {
    if !path.is_empty() {
        out.insert(
            path.to_owned(),
            FlatField {
                types: node.types.clone(),
                required,
                enums: node.enums.clone(),
            },
        );
    }
    for (key, (child, child_required)) in &node.props {
        let child_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        flatten(child, &child_path, *child_required, out);
    }
    if let Some(items) = &node.items {
        flatten(items, &format!("{path}[]"), true, out);
    }
}

/// A schema as `path -> {types, required, enum values}`, with every `$ref` resolved.
pub fn flatten_schema(schema: &Value) -> BTreeMap<String, FlatField> {
    let node = build(schema, schema, &mut Vec::new());
    let mut out = BTreeMap::new();
    flatten(&node, "", true, &mut out);
    out
}

/// Classifies the change from `old` to `new` (JSON schemas, `$ref` allowed).
///
/// Breaking: a path removed or renamed, its type changed or narrowed, a required field made
/// optional, an enum value removed. Additive: a path or an enum value added, a type widened,
/// an optional field made required.
pub fn compat(old: &Value, new: &Value) -> Change {
    let (old, new) = (flatten_schema(old), flatten_schema(new));
    let mut breaking = Vec::new();
    let mut additive = Vec::new();
    for (path, was) in &old {
        let Some(now) = new.get(path) else {
            breaking.push(format!("{path}: removed"));
            continue;
        };
        if was.types != now.types {
            if !was.types.is_empty() && now.types.is_superset(&was.types) {
                additive.push(format!("{path}: type widened"));
            } else if !now.types.is_empty() || !was.types.is_empty() {
                breaking.push(format!("{path}: type changed"));
            }
        }
        if was.required && !now.required {
            breaking.push(format!("{path}: no longer required"));
        } else if !was.required && now.required {
            additive.push(format!("{path}: now required"));
        }
        if was.enums != now.enums {
            if !was.enums.is_empty() && !now.enums.is_empty() {
                for value in was.enums.difference(&now.enums) {
                    breaking.push(format!("{path}: enum value {value} removed"));
                }
                for value in now.enums.difference(&was.enums) {
                    additive.push(format!("{path}: enum value {value} added"));
                }
            } else if was.enums.is_empty() {
                breaking.push(format!("{path}: values restricted to an enum"));
            } else {
                additive.push(format!("{path}: enum lifted"));
            }
        }
    }
    additive.extend(
        new.keys()
            .filter(|path| !old.contains_key(*path))
            .map(|path| format!("{path}: added")),
    );
    if !breaking.is_empty() {
        Change::Breaking(breaking)
    } else if !additive.is_empty() {
        Change::Additive(additive)
    } else {
        Change::None
    }
}

/// The snapshot of this version: `{schema_version, outputs: {name: schema}, codes}`.
pub fn snapshot() -> Value {
    let outputs: serde_json::Map<String, Value> = agent_output_schemas()
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    let codes: Vec<Value> = CliErrorCode::ALL
        .iter()
        .map(|c| serde_json::json!({"code": c.as_str(), "exit": c.exit_code()}))
        .collect();
    serde_json::json!({
        "schema_version": CLI_SCHEMA_VERSION,
        "outputs": outputs,
        "codes": codes,
    })
}

/// Where each output is read from and who gets it.
fn output_command(name: &str) -> (&'static str, &'static str) {
    match name {
        "status@agent" => ("raptor status --json", "mcp (agente)"),
        "undo@agent" => ("raptor undo --json", "mcp (agente)"),
        "redo@agent" => ("raptor redo --json", "mcp (agente)"),
        "agent-register" => (
            "raptor agent register <agente> --json",
            "mcp (agente) y desarrollador",
        ),
        "daemon-status@agent" => ("raptor daemon status --json", "mcp (agente)"),
        "agent-withdraw" => (
            "raptor agent withdraw --json",
            "desarrollador (el agente recibe reserved-command)",
        ),
        _ => ("", ""),
    }
}

fn type_text(types: &BTreeSet<String>) -> String {
    if types.is_empty() {
        "cualquiera".to_owned()
    } else {
        types.iter().cloned().collect::<Vec<_>>().join(" \\| ")
    }
}

/// The agent reference (Markdown, in Spanish), generated from the schemas of this version.
pub fn reference_markdown() -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str("id: DOC-CLI-JSON-CONTRACT\n");
    out.push_str("title: \"Contrato JSON de la CLI raptor para agentes\"\n");
    out.push_str("type: reference\n");
    out.push_str("status: generated\n");
    out.push_str("---\n\n");
    out.push_str("# Contrato JSON de la CLI `raptor` para agentes\n\n");
    out.push_str(
        "> Generado por la prueba de `crates/api/src/cli_contract.rs` desde \
         `crates/api/schemas/cli-contract.json`. No lo edites a mano: regenéralo con \
         `GITRAPTOR_BLESS_CLI_CONTRACT=1 cargo test -p gitraptor-api cli_contract`.\n\n",
    );
    out.push_str(&format!(
        "Versión del contrato: `{CLI_SCHEMA_VERSION}`. Toda salida `--json` es una línea con \
         `{{\"schema_version\", \"data\"}}` o `{{\"schema_version\", \"error\"}}`; \
         `error` lleva `code`, `message`, `params` y `action`. Un cambio incompatible sube el \
         mayor; añadir sube el menor. Las salidas del desarrollador llevan el sobre pero su \
         `data` no tiene esquema congelado.\n\n"
    ));
    let emitted: Vec<String> = CliErrorCode::ALL
        .iter()
        .map(|c| format!("`{}` (salida {})", c.as_str(), c.exit_code()))
        .collect();
    let emitted = emitted.join(", ");
    for (name, schema) in agent_output_schemas() {
        let (command, profile) = output_command(name);
        out.push_str(&format!("\n## {name}\n\n"));
        out.push_str(&format!("- Subcomando: `{command}`\n"));
        out.push_str(&format!("- Perfil: {profile}\n"));
        out.push_str(&format!("- Versión: `{CLI_SCHEMA_VERSION}`\n"));
        out.push_str(&format!("- Códigos de error posibles: {emitted}\n\n"));
        out.push_str("| Campo | Tipo | Requerido | Valores |\n|---|---|---|---|\n");
        for (path, field) in flatten_schema(&schema) {
            let values = field.enums.iter().cloned().collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "| `{path}` | {} | {} | {values} |\n",
                type_text(&field.types),
                if field.required { "sí" } else { "no" },
            ));
        }
    }
    out.push_str("\n## Códigos estables\n\n");
    out.push_str("Familia de salida: 0 éxito, 1 rechazo o fallo, 2 uso (`invalid-args`).\n\n");
    for code in stable_codes() {
        let exit = if code == CliOnlyCode::InvalidArgs.as_str() {
            2
        } else {
            1
        };
        out.push_str(&format!("- `{code}` (salida {exit})\n"));
    }
    out.push_str(
        "\n### Motivos locales\n\n\
         Además del nombre estable del error del canal, `command-failed` lleva en \
         `params.reason` estos motivos de `raptor guard exec --json`, que no vienen del motor: \
         `no-folder` (no hay carpeta de trabajo), `cancelled` (se canceló la ventana), \
         `declined` (el desarrollador rechazó la excepción), `interrupted` (Ctrl-C) y \
         `git-failed` (git no llegó a ejecutarse o no devolvió estado). `invalid-args` lleva \
         `params.kind` (el tipo del error de uso, p. ej. `unknown-argument`; `invalid-utf8` \
         para un argumento que no es UTF-8) y, cuando el binario lo define, `params.argument`.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn rendered_snapshot() -> String {
        let mut text = serde_json::to_string_pretty(&snapshot()).expect("a snapshot is JSON");
        text.push('\n');
        text
    }

    /// The version rule of the compatibility check against the stored snapshot.
    fn check_version(stored: &Value) -> Result<(), String> {
        let codes: Vec<(String, u8)> = CliErrorCode::ALL
            .iter()
            .map(|c| (c.as_str().to_owned(), c.exit_code()))
            .collect();
        check_against(
            stored,
            &agent_output_schemas(),
            &codes,
            (CLI_SCHEMA_MAJOR, CLI_SCHEMA_MINOR),
        )
    }

    /// `check_version` over explicit current outputs, codes and version.
    fn check_against(
        stored: &Value,
        outputs: &[(&'static str, Value)],
        codes: &[(String, u8)],
        (major, minor): (u16, u16),
    ) -> Result<(), String> {
        let version = stored["schema_version"].as_str().unwrap_or("0.0");
        let (old_major, old_minor) = version.split_once('.').unwrap_or(("0", "0"));
        let (old_major, old_minor): (u16, u16) = (
            old_major.parse().unwrap_or(0),
            old_minor.parse().unwrap_or(0),
        );
        let mut breaking = false;
        let mut additive = false;
        for (name, schema) in outputs {
            match compat(&stored["outputs"][*name], schema) {
                Change::None => {}
                Change::Additive(_) => additive = true,
                Change::Breaking(_) => breaking = true,
            }
        }
        // An output that is no longer produced breaks whoever reads it.
        if let Some(stored_outputs) = stored["outputs"].as_object() {
            breaking |= stored_outputs
                .keys()
                .any(|name| !outputs.iter().any(|(now, _)| now == name));
        }
        // A code is the pair (name, exit): a new pair is additive; a pair that is gone, which
        // includes a changed exit, is breaking.
        let stored_codes: BTreeSet<(String, u8)> = stored["codes"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|c| {
                        let exit = u8::try_from(c["exit"].as_u64()?).ok()?;
                        Some((c["code"].as_str()?.to_owned(), exit))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let now: BTreeSet<(String, u8)> = codes.iter().cloned().collect();
        breaking |= stored_codes.difference(&now).next().is_some();
        additive |= now.difference(&stored_codes).next().is_some();
        if breaking && major <= old_major {
            return Err("a breaking change needs a major bump".into());
        }
        if additive && major == old_major && minor <= old_minor {
            return Err("an additive change needs a minor bump".into());
        }
        Ok(())
    }

    fn object_schema(required: &[&str]) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": { "a": {"type": "string"}, "b": {"type": "string"} },
            "required": required,
        })
    }

    #[test]
    fn a_required_field_of_a_nullable_object_becoming_optional_is_breaking() {
        let wrap = |inner: Value| {
            serde_json::json!({
                "type": "object",
                "properties": { "x": { "anyOf": [ {"$ref": "#/$defs/X"}, {"type": "null"} ] } },
                "required": ["x"],
                "$defs": { "X": inner },
            })
        };
        let flat = flatten_schema(&wrap(object_schema(&["a"])));
        assert!(flat["x.a"].required, "null must not degrade the field");
        assert!(flat["x"].types.contains("null"));
        let change = compat(&wrap(object_schema(&["a"])), &wrap(object_schema(&[])));
        assert!(
            matches!(&change, Change::Breaking(m) if m.iter().any(|m| m.starts_with("x.a:"))),
            "{change:?}"
        );
    }

    #[test]
    fn a_field_absent_from_another_variant_keeps_its_own_required() {
        let wrap = |first: Value| {
            serde_json::json!({
                "oneOf": [first, {"type": "object", "properties": {"c": {"type": "string"}}}],
            })
        };
        let flat = flatten_schema(&wrap(object_schema(&["a"])));
        assert!(flat["a"].required);
        let change = compat(&wrap(object_schema(&["a"])), &wrap(object_schema(&[])));
        assert!(matches!(change, Change::Breaking(_)), "{change:?}");
    }

    fn stored_with(outputs: Value, codes: Value) -> Value {
        serde_json::json!({"schema_version": "1.0", "outputs": outputs, "codes": codes})
    }

    #[test]
    fn a_removed_output_needs_a_major_bump() {
        let schema = object_schema(&["a"]);
        let stored = stored_with(
            serde_json::json!({"one": schema.clone(), "two": schema.clone()}),
            serde_json::json!([]),
        );
        let now = [("one", schema)];
        assert!(check_against(&stored, &now, &[], (1, 1)).is_err());
        assert!(check_against(&stored, &now, &[], (2, 0)).is_ok());
    }

    #[test]
    fn codes_are_compared_as_code_and_exit() {
        let stored = stored_with(
            serde_json::json!({}),
            serde_json::json!([{"code": "a", "exit": 1}, {"code": "b", "exit": 1}]),
        );
        let pair = |code: &str, exit: u8| (code.to_owned(), exit);
        // Same codes: nothing to bump.
        let same = [pair("a", 1), pair("b", 1)];
        assert!(check_against(&stored, &[], &same, (1, 0)).is_ok());
        // Added: additive, so the minor must move.
        let added = [pair("a", 1), pair("b", 1), pair("c", 1)];
        assert!(check_against(&stored, &[], &added, (1, 0)).is_err());
        assert!(check_against(&stored, &[], &added, (1, 1)).is_ok());
        // Removed, or the exit changed: breaking, so the major must move.
        let removed = [pair("a", 1)];
        let exit_changed = [pair("a", 1), pair("b", 2)];
        for now in [&removed[..], &exit_changed[..]] {
            assert!(check_against(&stored, &[], now, (1, 5)).is_err());
            assert!(check_against(&stored, &[], now, (2, 0)).is_ok());
        }
    }

    #[test]
    fn snapshot_and_reference_are_current() {
        let snapshot_path = root().join("schemas/cli-contract.json");
        let reference_path = root().join("../../docs/architecture/design/cli-json-contract.md");
        let (snapshot, reference) = (rendered_snapshot(), reference_markdown());
        if std::env::var_os("GITRAPTOR_BLESS_CLI_CONTRACT").is_some_and(|v| v == "1") {
            if let Ok(stored) = std::fs::read_to_string(&snapshot_path) {
                let stored: Value = serde_json::from_str(&stored).expect("the snapshot is JSON");
                check_version(&stored).expect("the version covers the change");
            }
            std::fs::create_dir_all(snapshot_path.parent().expect("has a parent"))
                .expect("create schemas/");
            std::fs::write(&snapshot_path, snapshot).expect("write the snapshot");
            std::fs::write(&reference_path, reference).expect("write the reference");
            return;
        }
        let stored = std::fs::read_to_string(&snapshot_path).unwrap_or_default();
        assert_eq!(
            stored, snapshot,
            "the snapshot is stale; regenerate with GITRAPTOR_BLESS_CLI_CONTRACT=1"
        );
        let doc = std::fs::read_to_string(&reference_path).unwrap_or_default();
        assert_eq!(
            doc, reference,
            "the reference is stale; regenerate with GITRAPTOR_BLESS_CLI_CONTRACT=1"
        );
    }

    #[test]
    fn the_stored_snapshot_passes_the_version_rule() {
        let stored =
            std::fs::read_to_string(root().join("schemas/cli-contract.json")).unwrap_or_default();
        let stored: Value = serde_json::from_str(&stored).expect("the snapshot is JSON");
        check_version(&stored).expect("the version covers the change");
    }

    #[test]
    fn all_codes_are_known_and_unique() {
        let mut names: Vec<&str> = CliErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        let stable = stable_codes();
        assert!(names.iter().all(|n| stable.contains(n)));
        assert_eq!(
            stable.len(),
            McpToolError::ALL.len() + CliOnlyCode::ALL.len()
        );
    }

    #[test]
    fn the_error_envelope_serializes_the_code_as_its_name() {
        let err = CliErr {
            schema_version: CLI_SCHEMA_VERSION,
            error: CliError {
                code: CliErrorCode::Cli(CliOnlyCode::InvalidArgs),
                message: "m".into(),
                params: serde_json::json!({}),
                action: "a".into(),
            },
        };
        let value = serde_json::to_value(&err).expect("serializes");
        assert_eq!(value["error"]["code"], "invalid-args");
        assert_eq!(value["schema_version"], "1.0");
    }
}
