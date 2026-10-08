//! The texts of a refused tool call, in en/es (US-MCP-005, ADR-MCP-001 § 5,
//! NFR-10): one fixed template per stable code. Codes, tool names and tool
//! descriptions stay in English.

use gitraptor_api::mcp_view::{MCP_RETRY_AFTER_S, McpToolError};
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

/// The `params` the tool gives for a code: only typed fields, rebuilt from what the engine
/// layer handed over, never forwarded as they came.
fn tool_params(code: McpToolError, params: Option<&Value>) -> Option<Value> {
    let field = |key: &str| params.and_then(|p| p.get(key)).filter(|v| !v.is_null());
    match code {
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

/// "HH:MM UTC" of an instant in ms since the epoch, with no date library.
fn release_utc(ms: i64) -> String {
    let secs_of_day = ms.div_euclid(1000).rem_euclid(86_400);
    format!(
        "{:02}:{:02} UTC",
        secs_of_day / 3_600,
        secs_of_day % 3_600 / 60
    )
}

fn texts(code: McpToolError, lang: Lang, params: Option<&Value>) -> (&'static str, String) {
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
            return match lang {
                Lang::En => (
                    "Too many calls on this connection.",
                    format!("Wait {wait} s and retry."),
                ),
                Lang::Es => (
                    "Demasiadas llamadas en esta conexión.",
                    format!("Espera {wait} s y reintenta."),
                ),
            };
        }
        // A snapshot that ran past the daemon's budget saved nothing.
        (E::TimeLimit, lang) if field("saved").is_some() => {
            return match lang {
                Lang::En => (
                    "The snapshot did not finish in time; nothing was saved.",
                    "Retry later.".to_owned(),
                ),
                Lang::Es => (
                    "El snapshot no terminó a tiempo; no se guardó nada.",
                    "Reintenta más tarde.".to_owned(),
                ),
            };
        }
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
            return match (lang, write) {
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
            };
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
            return match window {
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
            };
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
        (E::OutcomeUnknown, Lang::En) => (
            "It is not known whether the snapshot was saved.",
            "Check `raptor timeline` before retrying.",
        ),
        (E::OutcomeUnknown, Lang::Es) => (
            "No se sabe si el snapshot se guardó.",
            "Revisa `raptor timeline` antes de reintentar.",
        ),
    };
    (message, action.to_owned())
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
            params: Some(json!({"kind": "write"})),
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
