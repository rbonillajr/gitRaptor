//! `app_info`: the app version and the engine protocol version it speaks.

use crate::ipc::{self, CommandError};
use serde::{Deserialize, Serialize};

/// Params of `app_info`: none.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AppInfoParams {}

/// Result of `app_info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Version of this crate.
    pub version: &'static str,
    /// Protocol version of the engine channel this build speaks.
    pub protocol: u32,
}

/// The data `app_info` answers.
pub fn info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        protocol: gitraptor_api::PROTOCOL_VERSION,
    }
}

/// Answers the app and protocol versions.
///
/// # Errors
///
/// [`CommandError::InvalidParams`] when the body is not `{"params": {}}`.
#[tauri::command]
pub fn app_info(request: tauri::ipc::Request<'_>) -> Result<AppInfo, CommandError> {
    let AppInfoParams {} = ipc::params(&request)?;
    Ok(info())
}
