//! The IPC envelope every command reads: `{"params": {…}}`, with no unknown field at either level.
//!
//! Commands read the raw `invoke` body instead of named arguments because Tauri silently ignores
//! top-level arguments it does not know.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::ipc::{InvokeBody, Request};

/// Error a command returns to the webview. It serialises as `{"code": "<kebab-case>"}` and never
/// carries engine text or paths; the frontend translates it with the key `error.<code>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum CommandError {
    /// The body is not the JSON envelope `{"params": {…}}` of the command.
    InvalidParams,
    /// `engine_retry` was asked before `engine_watch` started the supervisor.
    NotWatching,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParams => f.write_str("invalid-params"),
            Self::NotWatching => f.write_str("not-watching"),
        }
    }
}

impl std::error::Error for CommandError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<P> {
    params: P,
}

/// Parses `{"params": P}` from a JSON body.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] when the body is not an object, lacks `params`, has another key,
/// or `params` does not deserialise as `P`.
pub fn parse<P: DeserializeOwned>(body: &serde_json::Value) -> Result<P, CommandError> {
    Envelope::<P>::deserialize(body)
        .map(|e| e.params)
        .map_err(|_| CommandError::InvalidParams)
}

/// Parses the params of an `invoke` body: only a JSON body is accepted.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] for a raw (binary) body or whatever [`parse`] rejects.
pub fn from_body<P: DeserializeOwned>(body: &InvokeBody) -> Result<P, CommandError> {
    match body {
        InvokeBody::Json(value) => parse(value),
        InvokeBody::Raw(_) => Err(CommandError::InvalidParams),
    }
}

/// Parses the params of an `invoke` request.
///
/// # Errors
///
/// See [`from_body`].
pub fn params<P: DeserializeOwned>(request: &Request<'_>) -> Result<P, CommandError> {
    from_body(request.body())
}

#[cfg(test)]
mod tests {
    use super::{CommandError, from_body, parse};
    use serde::Deserialize;
    use serde_json::json;
    use tauri::ipc::InvokeBody;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}

    #[test]
    fn envelope_accepts_only_params_without_unknown_fields() {
        assert!(parse::<Empty>(&json!({"params": {}})).is_ok());
        for body in [
            json!({"params": {"x": 1}}),
            json!({"params": {}, "extra": 1}),
            json!({}),
            json!("x"),
            json!([]),
            serde_json::Value::Null,
        ] {
            assert_eq!(
                parse::<Empty>(&body).err(),
                Some(CommandError::InvalidParams),
                "{body}"
            );
        }
    }

    #[test]
    fn raw_body_is_invalid_params_and_json_body_is_parsed() {
        assert!(from_body::<Empty>(&InvokeBody::Json(json!({"params": {}}))).is_ok());
        assert_eq!(
            from_body::<Empty>(&InvokeBody::Raw(vec![1, 2, 3])).err(),
            Some(CommandError::InvalidParams)
        );
    }

    #[test]
    fn error_serialises_as_its_kebab_code_only() {
        assert_eq!(
            serde_json::to_value(CommandError::InvalidParams).ok(),
            Some(json!({"code": "invalid-params"}))
        );
    }
}
