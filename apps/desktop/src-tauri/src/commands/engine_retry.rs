//! `engine_retry`: STUB of the parameter type only (DS-US-DSK-003 T005); the command comes later.

use serde::Deserialize;

/// Params of `engine_retry`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRetryParams {
    pub start: bool,
}
