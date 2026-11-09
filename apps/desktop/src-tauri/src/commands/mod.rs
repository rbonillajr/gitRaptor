//! The closed list of IPC commands. It must match `commands.json` (a test checks it), and each
//! command needs its `allow-<command>` permission in `capabilities/main.json`.
//!
//! Never add a generic command that receives a channel method name.

pub mod app_info;
pub mod engine_retry;
pub mod engine_watch;
pub mod fleet_open;

/// The invoke handler with every command of the list.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        app_info::app_info,
        engine_watch::engine_watch,
        engine_retry::engine_retry,
        fleet_open::fleet_open
    ]
}
