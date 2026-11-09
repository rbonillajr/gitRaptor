//! The closed list of IPC commands. It must match `commands.json` (a test checks it), and each
//! command needs its `allow-<command>` permission in `capabilities/main.json`.
//!
//! Never add a generic command that receives a channel method name.

pub mod app_info;
pub mod engine_retry;
pub mod engine_watch;

/// The invoke handler with every command of the list.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![app_info::app_info]
}
