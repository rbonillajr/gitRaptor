//! Tauri backend of the GitRaptor desktop app. It is a client of the engine like the TUI: no Git
//! logic, a closed list of IPC commands and no plugins. It reads neither the environment nor the
//! command line.

pub mod commands;
pub mod ipc;
pub mod window;

/// Starts the app and blocks until the last window closes.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start (no webview available, for example): there is no
/// window to report the error in.
pub fn run() {
    let result = tauri::Builder::default()
        .invoke_handler(commands::handler())
        .setup(|app| {
            window::create(app)?;
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        panic!("the desktop app could not start: {e}");
    }
}
