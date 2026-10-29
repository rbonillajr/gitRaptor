//! The texts of a refused tool call, in en/es (US-MCP-005, ADR-MCP-001 § 5,
//! NFR-10): one fixed template per stable code. Codes, tool names and tool
//! descriptions stay in English.

use gitraptor_api::catalog::{MAX_COMMIT_MESSAGE_BYTES, MAX_COMMIT_PATHS};
use gitraptor_api::mcp_view::{MAX_MCP_PATH_BYTES, MCP_RETRY_AFTER_S, McpToolError, for_mcp};
use serde_json::{Value, json};

/// The language of the messages, from `raptor-mcp`'s environment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    Es,
}

impl Lang {
    /// The first non-empty of `LC_ALL`, `LC_MESSAGES` and `LANG` decides, as
    /// in POSIX: Spanish when it starts with `es`, English otherwise.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|key| var(key).filter(|v| !v.is_empty()))
            .map_or(Self::En, |v| {
                if v.starts_with("es") {
                    Self::Es
                } else {
                    Self::En
                }
            })
    }
}

/// A refused call as the engine layer hands it over: the stable code and the typed
/// `params` the template needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRefusal {
    pub code: McpToolError,
    pub params: Option<Value>,
}

impl From<McpToolError> for ToolRefusal {
    fn from(code: McpToolError) -> Self {
        Self { code, params: None }
    }
}

impl From<&ToolRefusal> for ToolRefusal {
    fn from(r: &ToolRefusal) -> Self {
        r.clone()
    }
}

/// The body of a refused call: `{code, message, action}`, plus `params`
/// when the code has any.
pub fn refusal(r: impl Into<ToolRefusal>, lang: Lang) -> Value {
    let ToolRefusal { code, params } = r.into();
    let (message, action) = texts(code, lang, params.as_ref());
    let mut body = json!({"code": code.as_str(), "message": message, "action": action});
    if let Some(params) = tool_params(code, params.as_ref()) {
        body["params"] = params;
    }
    body
}

/// The budget of a refusal's body (RES-MCP-03): `policy-denied` never cuts its rules or its
/// decision id (RES-MCP-04), so it has its own.
#[cfg(test)]
pub fn refusal_budget(code: McpToolError) -> usize {
    use gitraptor_api::mcp_view::{MCP_POLICY_REFUSAL_TOKENS, MCP_REFUSAL_TOKENS};
    if code == McpToolError::PolicyDenied {
        MCP_POLICY_REFUSAL_TOKENS
    } else {
        MCP_REFUSAL_TOKENS
    }
}

/// The `params` the tool gives for a code: only typed fields, rebuilt from what the engine
/// layer handed over, never forwarded as they came.
fn tool_params(code: McpToolError, params: Option<&Value>) -> Option<Value> {
    let field = |key: &str| params.and_then(|p| p.get(key)).filter(|v| !v.is_null());
    let operation_id = || {
        field("operation_id")
            .filter(|v| v.is_string())
            .map(|id| json!({"operation_id": id}))
    };
    match code {
        McpToolError::InvalidText if is_message(params) => Some(json!({
            "field": "message",
            "reason": message_reason(params),
            "max_bytes": MAX_COMMIT_MESSAGE_BYTES,
        })),
        McpToolError::PathNotFound
        | McpToolError::InvalidPath
        | McpToolError::PathOutsideWorktree
        | McpToolError::PathIgnored => {
            untrusted_path(params).map(|path| json!({"path": {"untrusted": path}}))
        }
        McpToolError::TooManyPaths => Some(json!({"max": MAX_COMMIT_PATHS})),
        McpToolError::PolicyDenied => {
            let rules: Vec<&str> = denied_rules(params).collect();
            let reasons: Vec<Value> = denied_reasons(params)
                .into_iter()
                .map(|(rule, level)| json!({"rule": rule, "level": level}))
                .collect();
            let decision = field("decision_id").filter(|v| v.is_string())?;
            let mut out = json!({"rules": rules, "decision_id": decision});
            // Absent rather than empty: a denial without a valid pair keeps the body it had.
            if !reasons.is_empty() {
                out["reasons"] = reasons.into();
            }
            Some(out)
        }
        McpToolError::FailedUnchanged if field("reason").is_some() => field("reason")
            .and_then(Value::as_str)
            .map(|reason| json!({"reason": reason})),
        McpToolError::Aborted
        | McpToolError::TimeLimit
        | McpToolError::FailedUnchanged
        | McpToolError::FailedChanged => operation_id(),
        McpToolError::RateLimited => Some(
            json!({"retry_after_s": field("retry_after_s").and_then(Value::as_u64).unwrap_or(MCP_RETRY_AFTER_S)}),
        ),
        McpToolError::OperationInProgress => field("kind")
            .filter(|k| matches!(k.as_str(), Some("git" | "write")))
            .map(|kind| json!({"kind": kind})),
        McpToolError::InvalidText => Some(json!({"field": "label", "max_chars": 64})),
        McpToolError::QuotaExceeded => {
            let window = field("window")?.clone();
            let mut out = json!({"window": window});
            if let Some(wait) = field("retry_after_s") {
                out["retry_after_s"] = wait.clone();
            }
            if window != "minute"
                && let Some(release) = field("release_utc_ms").and_then(Value::as_i64)
            {
                out["release_utc"] = release_utc(release).into();
            }
            Some(out)
        }
        _ => None,
    }
}

/// Whether an `invalid-text` is about a commit message (otherwise, a snapshot's label).
fn is_message(params: Option<&Value>) -> bool {
    params.and_then(|p| p.get("field")).and_then(Value::as_str) == Some("message")
}

/// The `reason` of a refused commit message, one of the three the template knows.
fn message_reason(params: Option<&Value>) -> &'static str {
    match params.and_then(|p| p.get("reason")).and_then(Value::as_str) {
        Some("required") => "required",
        Some("too-long") => "too-long",
        _ => "invalid-chars",
    }
}

/// The refused path, escaped as every untrusted text of an answer and cut at its bound.
fn untrusted_path(params: Option<&Value>) -> Option<String> {
    let path = params?.get("path")?.get("untrusted")?.as_str()?;
    let mut value = json!({"untrusted": path});
    for_mcp(&mut value);
    let mut text = value["untrusted"].as_str()?.to_owned();
    let mut end = text.len().min(MAX_MCP_PATH_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    Some(text)
}

/// Whether `rule` is a policy's own rule code, never repo text.
fn is_rule_code(rule: &str) -> bool {
    rule.len() <= 64
        && rule
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
}

/// The levels a reason of a denial may name; anything else drops the pair, not the answer.
const DENIED_LEVELS: [&str; 6] = ["minimum", "floor", "worktree", "profile", "local", "system"];

/// The rule codes of a denial: the policy's own names, never repo text.
fn denied_rules(params: Option<&Value>) -> impl Iterator<Item = &str> {
    params
        .and_then(|p| p.get("rules"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|rule| is_rule_code(rule))
        .take(4)
}

/// The `(rule, level)` pairs of a denial, each field validated on its own (a rule code and one
/// of [`DENIED_LEVELS`]), one per rule: the first valid one.
fn denied_reasons(params: Option<&Value>) -> Vec<(&str, &'static str)> {
    let mut out: Vec<(&str, &'static str)> = Vec::new();
    let pairs = params
        .and_then(|p| p.get("reasons"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for pair in pairs {
        if out.len() == 4 {
            break;
        }
        let Some(rule) = pair
            .get("rule")
            .and_then(Value::as_str)
            .filter(|r| is_rule_code(r))
        else {
            continue;
        };
        let level = pair.get("level").and_then(Value::as_str);
        let Some(level) = DENIED_LEVELS.into_iter().find(|l| Some(*l) == level) else {
            continue;
        };
        if out.iter().all(|(r, _)| *r != rule) {
            out.push((rule, level));
        }
    }
    out
}

/// The level of a denial's rule as the CLI names it (`guard.level.*`); the safe minimum names
/// itself through its rule.
fn level_words(level: &str, es: bool) -> Option<&'static str> {
    Some(match (level, es) {
        ("floor", false) => "team configuration",
        ("floor", true) => "configuración del equipo",
        ("worktree", false) => "team configuration on this branch",
        ("worktree", true) => "configuración del equipo en esta rama",
        ("profile", false) => "personal configuration",
        ("profile", true) => "configuración personal",
        ("local", false) => "local configuration",
        ("local", true) => "configuración local",
        ("system", false) => "default value",
        ("system", true) => "valor por defecto",
        _ => return None,
    })
}

/// A template with a fixed message.
fn owned((message, action): (&str, String)) -> (String, String) {
    (message.to_owned(), action)
}

/// "HH:MM UTC" of an instant in ms since the epoch, with no date library.
fn release_utc(ms: i64) -> String {
    let secs_of_day = ms.div_euclid(1000).rem_euclid(86_400);
    format!(
        "{:02}:{:02} UTC",
        secs_of_day / 3_600,
        secs_of_day % 3_600 / 60
    )
}

fn texts(code: McpToolError, lang: Lang, params: Option<&Value>) -> (String, String) {
    use McpToolError as E;
    let field = |key: &str| params.and_then(|p| p.get(key)).filter(|v| !v.is_null());
    let (message, action) = match (code, lang) {
        (E::RepoNotEnabled, Lang::En) => (
            "This repo is not enabled for GitRaptor's MCP.",
            "Ask the developer to run `raptor mcp enable` in this repo.",
        ),
        (E::RepoNotEnabled, Lang::Es) => (
            "Este repo no está habilitado para el MCP de GitRaptor.",
            "Pide al desarrollador que ejecute `raptor mcp enable` en este repo.",
        ),
        (E::NotInObservedWorktree, Lang::En) => (
            "This session was not started inside a worktree GitRaptor observes.",
            "Start the session inside an observed repo.",
        ),
        (E::NotInObservedWorktree, Lang::Es) => (
            "Esta sesión no se inició dentro de un worktree que GitRaptor observe.",
            "Inicia la sesión dentro de un repo observado.",
        ),
        (E::RepoUnavailable, Lang::En) => (
            "GitRaptor cannot read this repo now.",
            "Ask the developer to check the repo in GitRaptor.",
        ),
        (E::RepoUnavailable, Lang::Es) => (
            "GitRaptor no puede leer este repo ahora.",
            "Pide al desarrollador que revise el repo en GitRaptor.",
        ),
        (E::EngineUnavailable, Lang::En) => (
            "GitRaptor is not running and could not be started.",
            "Check the GitRaptor installation.",
        ),
        (E::EngineUnavailable, Lang::Es) => (
            "GitRaptor no está en marcha y no se pudo arrancar.",
            "Revisa la instalación de GitRaptor.",
        ),
        (E::IdentityUnverified, Lang::En) => {
            ("GitRaptor could not verify who is calling.", "Retry later.")
        }
        (E::IdentityUnverified, Lang::Es) => (
            "GitRaptor no pudo verificar quién llama.",
            "Reintenta más tarde.",
        ),
        (E::RateLimited, lang) => {
            let wait = field("retry_after_s")
                .and_then(Value::as_u64)
                .unwrap_or(MCP_RETRY_AFTER_S);
            return owned(match lang {
                Lang::En => (
                    "Too many calls on this connection.",
                    format!("Wait {wait} s and retry."),
                ),
                Lang::Es => (
                    "Demasiadas llamadas en esta conexión.",
                    format!("Espera {wait} s y reintenta."),
                ),
            });
        }
        // A snapshot that ran past the daemon's budget saved nothing.
        (E::TimeLimit, lang) if field("saved").is_some() => {
            return owned(match lang {
                Lang::En => (
                    "The snapshot did not finish in time; nothing was saved.",
                    "Retry later.".to_owned(),
                ),
                Lang::Es => (
                    "El snapshot no terminó a tiempo; no se guardó nada.",
                    "Reintenta más tarde.".to_owned(),
                ),
            });
        }
        // A commit whose hook ran past the operation's limit.
        (E::TimeLimit, Lang::En) if field("operation_id").is_some() => (
            "The hook ran past the time limit; nothing was committed.",
            "Ask the developer to check the slow hook.",
        ),
        (E::TimeLimit, Lang::Es) if field("operation_id").is_some() => (
            "El hook superó el tiempo máximo; no se hizo el commit.",
            "Pide al desarrollador que revise el hook lento.",
        ),
        (E::TimeLimit, Lang::En) => ("GitRaptor did not answer in time.", "Retry later."),
        (E::TimeLimit, Lang::Es) => ("GitRaptor no respondió a tiempo.", "Reintenta más tarde."),
        (E::ResultTooLarge, Lang::En) => (
            "The answer exceeded its size limit and was not sent.",
            "Report it to the developer.",
        ),
        (E::ResultTooLarge, Lang::Es) => (
            "La respuesta superó su tamaño máximo y no se envió.",
            "Avisa al desarrollador.",
        ),
        (E::Internal, Lang::En) => ("GitRaptor could not answer this call.", "Retry later."),
        (E::Internal, Lang::Es) => (
            "GitRaptor no pudo responder a esta llamada.",
            "Reintenta más tarde.",
        ),
        (E::Unattributed, Lang::En) => (
            "The agent could not be identified.",
            "Register first: use register_agent.",
        ),
        (E::Unattributed, Lang::Es) => (
            "No se pudo identificar al agente.",
            "Regístrate: usa register_agent.",
        ),
        (E::OperationInProgress, lang) => {
            let write = field("kind").and_then(Value::as_str) == Some("write");
            let snapshot = field("tool").and_then(Value::as_str) == Some("snapshot");
            return owned(match (lang, write) {
                (Lang::En, true) if !snapshot => (
                    "Your previous write is still in progress.",
                    "Wait and check status or ask the developer for raptor timeline; do not \
repeat the commit."
                        .to_owned(),
                ),
                (Lang::Es, true) if !snapshot => (
                    "Tu escritura anterior sigue en curso.",
                    "Espera y revisa status o pide al desarrollador raptor timeline; no repitas \
el commit."
                        .to_owned(),
                ),
                (Lang::En, false) => (
                    "A Git operation is in progress in this worktree.",
                    "Finish or abort the Git operation and retry.".to_owned(),
                ),
                (Lang::En, true) => (
                    "An operation is in progress: your previous snapshot has not finished.",
                    "Wait for its answer before asking for another.".to_owned(),
                ),
                (Lang::Es, false) => (
                    "Hay una operación en curso en este worktree.",
                    "Termina o aborta la operación de Git y reintenta.".to_owned(),
                ),
                (Lang::Es, true) => (
                    "Hay una operación en curso: tu snapshot anterior aún no terminó.",
                    "Espera su respuesta antes de pedir otro.".to_owned(),
                ),
            });
        }
        (E::GitBusy, Lang::En) => ("Git is busy in this worktree.", "Retry when Git finishes."),
        (E::GitBusy, Lang::Es) => (
            "Git está ocupado en este worktree.",
            "Reintenta cuando Git termine.",
        ),
        (E::QuotaExceeded, lang) => {
            let window = field("window").and_then(Value::as_str).unwrap_or("minute");
            let wait = field("retry_after_s").and_then(Value::as_u64);
            let release = field("release_utc_ms")
                .and_then(Value::as_i64)
                .map(release_utc);
            let es = lang == Lang::Es;
            return owned(match window {
                "disk" => (
                    if es {
                        "No queda espacio para más snapshots."
                    } else {
                        "There is no space left for more snapshots."
                    },
                    if es {
                        "Pide al desarrollador que libere espacio."
                    } else {
                        "Ask the developer to free up space."
                    }
                    .to_owned(),
                ),
                "minute" => (
                    if es {
                        "Alcanzaste el límite de snapshots por minuto."
                    } else {
                        "You reached the snapshot limit per minute."
                    },
                    wait_action(es, wait),
                ),
                _ => (
                    if es {
                        "Alcanzaste el límite de snapshots de 24 h."
                    } else {
                        "You reached the 24 h snapshot limit."
                    },
                    match release {
                        Some(at) if es => format!("Reintenta después de las {at}."),
                        Some(at) => format!("Retry after {at}."),
                        None => wait_action(es, wait),
                    },
                ),
            });
        }
        (E::InvalidText, lang) if is_message(params) => {
            let reason = message_reason(params);
            return owned(match lang {
                Lang::En => (
                    match reason {
                        "required" => "Commit message: the message is required.",
                        "too-long" => "Commit message: message too long.",
                        _ => "Commit message: invalid text.",
                    },
                    "Write a message of up to 16 KiB without control characters.".to_owned(),
                ),
                Lang::Es => (
                    match reason {
                        "required" => "Mensaje de commit: el mensaje es obligatorio.",
                        "too-long" => "Mensaje de commit: mensaje demasiado largo.",
                        _ => "Mensaje de commit: texto no válido.",
                    },
                    "Escribe un mensaje de hasta 16 KiB sin caracteres de control.".to_owned(),
                ),
            });
        }
        (E::InvalidText, Lang::En) => (
            "Label: invalid text.",
            "Use 1 to 64 characters, no control characters.",
        ),
        (E::InvalidText, Lang::Es) => (
            "Etiqueta: texto no válido.",
            "Usa de 1 a 64 caracteres sin caracteres de control.",
        ),
        (E::StateChanged, Lang::En) => (
            "The worktree changed during the snapshot; nothing was saved.",
            "Retry.",
        ),
        (E::StateChanged, Lang::Es) => (
            "El worktree cambió durante el snapshot; no se guardó nada.",
            "Reintenta.",
        ),
        (E::OutcomeUnknown, Lang::En) if field("tool").is_some() => (
            "It is not known whether the snapshot was saved.",
            "Check `raptor timeline` before retrying.",
        ),
        (E::OutcomeUnknown, Lang::Es) if field("tool").is_some() => (
            "No se sabe si el snapshot se guardó.",
            "Revisa `raptor timeline` antes de reintentar.",
        ),
        (E::OutcomeUnknown, Lang::En) => (
            "It is not known whether the commit was made.",
            "Check status or ask the developer for raptor timeline before retrying.",
        ),
        (E::OutcomeUnknown, Lang::Es) => (
            "No se sabe si el commit se hizo.",
            "Revisa status o pide al desarrollador raptor timeline antes de reintentar.",
        ),
        (E::WorktreeMissing, Lang::En) => (
            "The worktree of this session no longer exists.",
            "Start the session in an existing worktree of the repo.",
        ),
        (E::WorktreeMissing, Lang::Es) => (
            "El worktree de esta sesión ya no existe.",
            "Inicia la sesión en un worktree existente del repo.",
        ),
        (E::RepoOtherOwner, Lang::En) => (
            "This repo belongs to another user, so GitRaptor does not read it.",
            "Ask the developer to check the repo's owner.",
        ),
        (E::RepoOtherOwner, Lang::Es) => (
            "Este repo pertenece a otro usuario, así que GitRaptor no lo lee.",
            "Pide al desarrollador que revise el propietario del repo.",
        ),
        (E::WorktreeUntrusted, Lang::En) => (
            "GitRaptor does not trust the folder of this worktree.",
            "Ask the developer to check the worktree in GitRaptor.",
        ),
        (E::WorktreeUntrusted, Lang::Es) => (
            "GitRaptor no confía en la carpeta de este worktree.",
            "Pide al desarrollador que revise el worktree en GitRaptor.",
        ),
        (E::InvalidCursor, Lang::En) => (
            "The cursor is not valid for this session.",
            "Call status again without a cursor and use the ones it returns.",
        ),
        (E::InvalidCursor, Lang::Es) => (
            "El cursor no es válido para esta sesión.",
            "Llama a status sin cursor y usa los que devuelve.",
        ),
        (E::ConfirmationRequired, Lang::En) => (
            "This action requires the developer's confirmation.",
            "Tell the developer: the decision is theirs. Retrying gets the same answer.",
        ),
        (E::ConfirmationRequired, Lang::Es) => (
            "Esta acción requiere confirmación del desarrollador.",
            "Avisa al desarrollador: la decisión es suya. Reintentar da la misma respuesta.",
        ),
        (E::NothingToCommit, Lang::En) => {
            ("Nothing to commit.", "Change or stage files and retry.")
        }
        (E::NothingToCommit, Lang::Es) => (
            "Nada que commitear.",
            "Modifica o prepara archivos y reintenta.",
        ),
        (E::DetachedHead, Lang::En) => ("HEAD is detached.", "Create or switch to a branch."),
        (E::DetachedHead, Lang::Es) => ("HEAD separado.", "Crea una rama o cámbiate a una."),
        (E::PolicyDenied, lang) => {
            let rule = denied_rules(params).next();
            let es = lang == Lang::Es;
            let level = rule.and_then(|rule| {
                denied_reasons(params)
                    .into_iter()
                    .find(|(r, _)| *r == rule)
                    .and_then(|(_, level)| level_words(level, es))
            });
            let message = match (rule, level, es) {
                (Some(rule), Some(level), false) => {
                    format!("A repo rule denied the commit: {rule} ({level}).")
                }
                (Some(rule), Some(level), true) => {
                    format!("Guardrails denegó el commit por una regla del repo: {rule} ({level}).")
                }
                (Some(rule), None, false) => {
                    format!("A repo rule denied the commit: {rule}.")
                }
                (Some(rule), None, true) => {
                    format!("Guardrails denegó el commit por una regla del repo: {rule}.")
                }
                (None, _, false) => "A repo rule denied the commit.".to_owned(),
                (None, _, true) => "Guardrails denegó el commit por una regla del repo.".to_owned(),
            };
            let action = if es {
                "Cumple la regla o pide al desarrollador que la cambie."
            } else {
                "Follow the rule or ask the developer to change it."
            };
            return (message, action.to_owned());
        }
        (E::Aborted, Lang::En) => (
            "The prior snapshot could not be taken; nothing was committed.",
            "Ask the developer to free up space or check GitRaptor, and retry.",
        ),
        (E::Aborted, Lang::Es) => (
            "No se pudo tomar el snapshot previo; no se hizo el commit.",
            "Pide al desarrollador que libere espacio o revise GitRaptor, y reintenta.",
        ),
        (E::FailedUnchanged, Lang::En) if field("reason").is_some() => (
            "No commit was made: the state of the worktree does not allow it.",
            "Ask the developer to check the worktree.",
        ),
        (E::FailedUnchanged, Lang::Es) if field("reason").is_some() => (
            "No se hizo el commit: el estado del worktree no lo permite.",
            "Pide al desarrollador que revise el worktree.",
        ),
        (E::FailedUnchanged, Lang::En) => (
            "Git did not commit: a hook refused it or failed.",
            "The hook's output is not returned over MCP: fix what it asks and retry.",
        ),
        (E::FailedUnchanged, Lang::Es) => (
            "Git no hizo el commit: un hook lo rechazó o falló.",
            "La salida del hook no se devuelve por MCP: corrige lo que pide y reintenta.",
        ),
        (E::FailedChanged, Lang::En) => (
            "The operation failed and left changes.",
            "Ask the developer to check it; raptor undo can undo it.",
        ),
        (E::FailedChanged, Lang::Es) => (
            "La operación falló y dejó cambios.",
            "Pide al desarrollador que lo revise; puede deshacerlo con raptor undo.",
        ),
        (E::OtherSessionPresent, Lang::En) => (
            "Another agent session is in this worktree.",
            "Name your paths: the worktree is shared.",
        ),
        (E::OtherSessionPresent, Lang::Es) => (
            "Hay otra sesión de agente en este worktree.",
            "Indica tus rutas: el worktree es compartido.",
        ),
        (E::TooManyPaths, lang) => {
            return match lang {
                Lang::En => (
                    format!("Too many paths: at most {MAX_COMMIT_PATHS}."),
                    NAME_FILES_EN.to_owned(),
                ),
                Lang::Es => (
                    format!("Demasiadas rutas: como mucho {MAX_COMMIT_PATHS}."),
                    NAME_FILES_ES.to_owned(),
                ),
            };
        }
        (E::PathNotFound | E::InvalidPath | E::PathOutsideWorktree | E::PathIgnored, lang) => {
            return path_texts(code, lang, untrusted_path(params).as_deref());
        }
    };
    (message.to_owned(), action.to_owned())
}

const NAME_FILES_EN: &str =
    "Name files of the worktree, relative to its root, by their exact name.";
const NAME_FILES_ES: &str =
    "Nombra archivos del worktree, relativos a su raíz, con su nombre exacto.";

/// The texts of a refused path, which the message names as data.
fn path_texts(code: McpToolError, lang: Lang, path: Option<&str>) -> (String, String) {
    use McpToolError as E;
    let es = lang == Lang::Es;
    let named = |text: &str| match path {
        Some(path) => format!("{text}: {path}."),
        None => format!("{text}."),
    };
    match code {
        E::PathNotFound if es => (
            named("Ruta no encontrada"),
            "Nombra archivos del worktree con su nombre exacto, relativos a su raíz.".to_owned(),
        ),
        E::PathNotFound => (
            named("Path not found"),
            "Name files of the worktree by their exact name, relative to its root.".to_owned(),
        ),
        E::PathIgnored => {
            let path = path.unwrap_or("?");
            if es {
                (
                    format!("La ruta {path} está ignorada por Git; no se añade."),
                    "Nombra solo archivos que Git no ignore.".to_owned(),
                )
            } else {
                (
                    format!("The path {path} is ignored by Git; it is not added."),
                    "Name only files Git does not ignore.".to_owned(),
                )
            }
        }
        E::PathOutsideWorktree if es => {
            (named("Ruta fuera del worktree"), NAME_FILES_ES.to_owned())
        }
        E::PathOutsideWorktree => (named("Path outside the worktree"), NAME_FILES_EN.to_owned()),
        _ if es => (named("Ruta no válida"), NAME_FILES_ES.to_owned()),
        _ => (named("Invalid path"), NAME_FILES_EN.to_owned()),
    }
}

fn wait_action(es: bool, wait: Option<u64>) -> String {
    match (es, wait) {
        (true, Some(n)) => format!("Espera {n} s."),
        (false, Some(n)) => format!("Wait {n} s."),
        (true, None) => "Espera y reintenta.".to_owned(),
        (false, None) => "Wait and retry.".to_owned(),
    }
}

#[cfg(test)]
#[path = "messages_snapshot_tests.rs"]
mod snapshot_tests;

#[cfg(test)]
#[path = "messages_confirmation_tests.rs"]
mod confirmation_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_its_texts_in_both_languages() {
        for code in McpToolError::ALL {
            let en = refusal(code, Lang::En);
            let es = refusal(code, Lang::Es);
            assert_eq!(en["code"], code.as_str());
            assert_eq!(es["code"], code.as_str());
            for body in [&en, &es] {
                assert!(!body["message"].as_str().unwrap().is_empty());
                assert!(!body["action"].as_str().unwrap().is_empty());
            }
            assert_ne!(en["message"], es["message"], "{code:?}");
        }
        let limited = refusal(McpToolError::RateLimited, Lang::En);
        assert_eq!(limited["params"]["retry_after_s"], MCP_RETRY_AFTER_S);
        assert!(
            refusal(McpToolError::Internal, Lang::En)
                .get("params")
                .is_none()
        );
    }

    #[test]
    fn the_spanish_snapshot_texts_say_what_the_agent_needs() {
        let es = |code| refusal(code, Lang::Es).to_string();
        assert!(es(McpToolError::InvalidText).contains("texto no válido"));
        assert!(es(McpToolError::Unattributed).contains("usa register_agent"));
        let busy = ToolRefusal {
            code: McpToolError::OperationInProgress,
            params: Some(json!({"kind": "write", "tool": "snapshot"})),
        };
        let body = refusal(&busy, Lang::Es);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("operación en curso")
        );
        assert_eq!(body["params"], json!({"kind": "write"}));
        assert!(es(McpToolError::OperationInProgress).contains("operación en curso"));
    }

    #[test]
    fn a_write_rate_limit_gives_its_own_wait() {
        let limited = ToolRefusal {
            code: McpToolError::RateLimited,
            params: Some(json!({"retry_after_s": 3})),
        };
        let body = refusal(&limited, Lang::En);
        assert_eq!(body["params"]["retry_after_s"], 3);
        assert!(body["action"].as_str().unwrap().contains("3 s"));
    }

    /// RES-MCP-03 with the `params` each snapshot refusal carries.
    #[test]
    fn every_snapshot_refusal_with_params_fits_its_budget() {
        use gitraptor_api::mcp_view::{MCP_REFUSAL_TOKENS, check_token_budget};
        let release = 20_000 * 86_400_000_i64;
        let cases = [
            (McpToolError::OperationInProgress, json!({"kind": "write"})),
            (McpToolError::OperationInProgress, json!({"kind": "git"})),
            (McpToolError::InvalidText, json!({})),
            (McpToolError::TimeLimit, json!({"saved": false})),
            (
                McpToolError::QuotaExceeded,
                json!({"window": "day", "retry_after_s": 86_399, "release_utc_ms": release}),
            ),
            (
                McpToolError::QuotaExceeded,
                json!({"window": "worktree-day", "retry_after_s": 3_600, "release_utc_ms": release}),
            ),
            (McpToolError::QuotaExceeded, json!({"window": "disk"})),
        ];
        for (code, params) in cases {
            for lang in [Lang::En, Lang::Es] {
                let text = refusal(
                    ToolRefusal {
                        code,
                        params: Some(params.clone()),
                    },
                    lang,
                )
                .to_string();
                let what = format!("{} {params} ({lang:?})", code.as_str());
                check_token_budget("RES-MCP-03", &what, &text, MCP_REFUSAL_TOKENS).unwrap();
            }
        }
    }

    /// RES-MCP-03: the usual `policy-denied` (one rule with its level, in Spanish, the longer
    /// language) stays within the budget it had before the reasons, so the worst case is the
    /// only one paying for them.
    #[test]
    fn a_typical_denial_stays_within_the_former_budget() {
        use gitraptor_api::mcp_view::check_token_budget;
        let body = refusal(
            ToolRefusal {
                code: McpToolError::PolicyDenied,
                params: Some(json!({
                    "rules": ["permission.commit"],
                    "reasons": [{"rule": "permission.commit", "level": "floor"}],
                    "decision_id": "f".repeat(32),
                })),
            },
            Lang::Es,
        );
        assert!(body["params"]["reasons"].is_array(), "{body}");
        check_token_budget(
            "RES-MCP-03",
            "policy-denied typical",
            &body.to_string(),
            125,
        )
        .unwrap();
    }

    /// RES-MCP-03 and RES-MCP-04: the worst `policy-denied` (4 rules at the longest codes, a
    /// 32-hex decision id) keeps every rule and the id, within its own budget; every other code
    /// keeps the common one.
    #[test]
    fn the_worst_denial_keeps_its_rules_and_id_within_its_budget() {
        use gitraptor_api::mcp_view::{
            MCP_POLICY_REFUSAL_TOKENS, MCP_REFUSAL_TOKENS, check_token_budget,
        };
        let rules = json!([
            "system.channel-not-authentic",
            "authorship.trailer-required",
            "permission.worktree-remove",
            "minimum.base-branch-delete"
        ]);
        // Four pairs, each at the longest level name.
        let reasons: Vec<Value> = rules
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| json!({"rule": rule, "level": "worktree"}))
            .collect();
        let decision = "f".repeat(32);
        for lang in [Lang::En, Lang::Es] {
            let body = refusal(
                ToolRefusal {
                    code: McpToolError::PolicyDenied,
                    params: Some(
                        json!({"rules": rules, "reasons": reasons, "decision_id": decision}),
                    ),
                },
                lang,
            );
            assert_eq!(body["params"]["rules"], rules, "{body}");
            assert_eq!(body["params"]["reasons"], json!(reasons), "{body}");
            assert_eq!(body["params"]["decision_id"], decision.as_str(), "{body}");
            let text = body.to_string();
            check_token_budget(
                "RES-MCP-03",
                "policy-denied",
                &text,
                MCP_POLICY_REFUSAL_TOKENS,
            )
            .unwrap();
        }
        for code in McpToolError::ALL {
            let expected = if code == McpToolError::PolicyDenied {
                MCP_POLICY_REFUSAL_TOKENS
            } else {
                MCP_REFUSAL_TOKENS
            };
            assert_eq!(refusal_budget(code), expected, "{code:?}");
        }
    }

    #[test]
    fn the_language_follows_the_posix_order() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(Lang::from_env(env(&[])), Lang::En);
        assert_eq!(Lang::from_env(env(&[("LANG", "es_ES.UTF-8")])), Lang::Es);
        assert_eq!(
            Lang::from_env(env(&[("LC_ALL", "en_US.UTF-8"), ("LANG", "es_ES.UTF-8")])),
            Lang::En
        );
        assert_eq!(
            Lang::from_env(env(&[
                ("LC_ALL", ""),
                ("LC_MESSAGES", "es_MX"),
                ("LANG", "C")
            ])),
            Lang::Es
        );
    }
}
