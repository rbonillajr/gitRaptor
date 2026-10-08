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
    let ToolRefusal { code, params: _ } = r.into();
    let (message, action) = texts(code, lang);
    let mut body = json!({"code": code.as_str(), "message": message, "action": action});
    if code == McpToolError::RateLimited {
        body["params"] = json!({"retry_after_s": MCP_RETRY_AFTER_S});
    }
    body
}

fn texts(code: McpToolError, lang: Lang) -> (&'static str, String) {
    use McpToolError as E;
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
        (E::RateLimited, Lang::En) => {
            return (
                "Too many calls on this connection.",
                format!("Wait {MCP_RETRY_AFTER_S} s and retry."),
            );
        }
        (E::RateLimited, Lang::Es) => {
            return (
                "Demasiadas llamadas en esta conexión.",
                format!("Espera {MCP_RETRY_AFTER_S} s y reintenta."),
            );
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
        (E::QuotaExceeded, _) => todo!("US-MCP-008"),
    };
    (message, action.to_owned())
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
