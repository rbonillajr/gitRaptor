//! JSON-RPC 2.0 envelopes (ADR-GRP-005 § 5).
//!
//! Every client message is a request with an id; batches are not supported.
//! The daemon answers with a [`Response`] and pushes stream events as
//! [`Notification`]s.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The only accepted value of `jsonrpc`.
pub const JSONRPC: &str = "2.0";

/// Longest accepted string id.
pub const MAX_ID_LEN: usize = 64;

/// Error codes. Standard JSON-RPC codes plus the engine's own range.
pub mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// The first message was not a valid `hello`.
    pub const HANDSHAKE_REQUIRED: i64 = -32001;
    /// Client and daemon speak different protocol versions.
    pub const INCOMPATIBLE_PROTOCOL: i64 = -32002;
    /// A reserved command was refused by the daemon (ADR-GRP-005 § 6).
    pub const RESERVED_REFUSED: i64 = -32003;
    /// The method is declared in the contract but its story has not
    /// implemented it yet.
    pub const NOT_IMPLEMENTED: i64 = -32004;
    /// Too many requests on this connection (SEC-08).
    pub const RATE_LIMITED: i64 = -32005;
    /// A connection or subscription limit was reached (SEC-08).
    pub const LIMIT_REACHED: i64 = -32006;
    /// The subscription could not continue from the requested point; an
    /// `events.resync` notification says why. Take a new snapshot.
    pub const RESYNC_REQUIRED: i64 = -32007;
    /// The prior snapshot failed: the operation was not run and the repo
    /// did not change (BR-TMC-CONS-001). `data` says why.
    pub const PRIOR_SNAPSHOT_FAILED: i64 = -32008;
    /// The thing asked for does not exist for this caller: an id of another
    /// repo answers the same as an unknown one (SEC-TMC-07).
    pub const NOT_FOUND: i64 = -32009;
    /// The caller's scope is refused: no readable working folder, not in an
    /// observed repo, or the repo is not in the MCP allowlist (SEC-TMC-15).
    pub const SCOPE_REFUSED: i64 = -32010;
    /// The operation started after its prior snapshot and failed: the oplog
    /// marks it interrupted and the prior snapshot can undo it.
    pub const OPERATION_FAILED: i64 = -32011;
    /// The caller's identity changed since the connection was accepted.
    pub const IDENTITY_UNVERIFIED: i64 = -32012;
    /// A repo command was rejected for what it names (not who asks):
    /// `data` is a `RepoRejectedData` (US-GRP-001).
    pub const REPO_REJECTED: i64 = -32013;
}

/// Request id: a number or a short string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Id {
    Num(u64),
    Str(String),
}

impl Id {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Num(_) => true,
            Self::Str(s) => s.len() <= MAX_ID_LEN,
        }
    }
}

/// A client request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    pub fn new(id: u64, method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id: Id::Num(id),
            method: method.into(),
            params: Some(serde_json::to_value(params).unwrap_or(Value::Null)),
        }
    }

    /// Decodes the parameters strictly into `T`. Missing parameters are an
    /// empty object.
    pub fn params<T: DeserializeOwned>(&self) -> Result<T, ErrorObject> {
        // Deserialized from a reference: a large `params` is never copied.
        let empty = Value::Object(Default::default());
        T::deserialize(self.params.as_ref().unwrap_or(&empty)).map_err(|err| {
            ErrorObject::new(code::INVALID_PARAMS, &format!("invalid params: {err}"))
        })
    }
}

/// A JSON-RPC error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    pub fn new(code: i64, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: impl Serialize) -> Self {
        self.data = serde_json::to_value(data).ok();
        self
    }
}

impl std::fmt::Display for ErrorObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl std::error::Error for ErrorObject {}

/// The daemon's answer to one request. `id` is null when the request could
/// not be parsed far enough to know it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    pub fn ok(id: Id, result: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            error: None,
        }
    }

    pub fn err(id: Option<Id>, error: ErrorObject) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// The result decoded strictly into `T`, or the error.
    pub fn into_result<T: DeserializeOwned>(self) -> Result<T, ErrorObject> {
        if let Some(error) = self.error {
            return Err(error);
        }
        serde_json::from_value(self.result.unwrap_or(Value::Null)).map_err(|err| {
            ErrorObject::new(code::INTERNAL, &format!("unexpected result shape: {err}"))
        })
    }
}

/// A message pushed by the daemon without a request (stream events).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

impl Notification {
    pub fn new(method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            method: method.into(),
            params: serde_json::to_value(params).unwrap_or(Value::Null),
        }
    }
}

/// Anything the daemon sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ServerMessage {
    Response(Response),
    Notification(Notification),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_are_refused() {
        let bad = r#"{"jsonrpc":"2.0","id":1,"method":"x","extra":true}"#;
        assert!(serde_json::from_str::<Request>(bad).is_err());
        let ok = r#"{"jsonrpc":"2.0","id":"a","method":"x"}"#;
        assert!(serde_json::from_str::<Request>(ok).is_ok());
    }

    #[test]
    fn server_messages_are_told_apart() {
        let resp = serde_json::to_string(&Response::ok(Id::Num(1), 5)).unwrap();
        assert!(matches!(
            serde_json::from_str::<ServerMessage>(&resp).unwrap(),
            ServerMessage::Response(_)
        ));
        let note = serde_json::to_string(&Notification::new("events.event", 1)).unwrap();
        assert!(matches!(
            serde_json::from_str::<ServerMessage>(&note).unwrap(),
            ServerMessage::Notification(_)
        ));
    }
}
