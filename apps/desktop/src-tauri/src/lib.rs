//! Tauri backend of the GitRaptor desktop app. It is a client of the engine like the TUI: no Git
//! logic, a closed list of IPC commands and one plugin (the native dialog, never reachable from the webview). It reads neither the environment nor the
//! command line.

pub mod commands;
pub mod engine;
pub mod ipc;
pub mod window;

/// Starts the app and blocks until the last window closes.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start (no webview available, for example): there is no
/// window to report the error in.
pub fn run() {
    let hub = engine::EngineHub::new(
        Box::new(engine::launcher::BundleConnector::new()),
        engine::launcher::bundled_raptor(),
    );
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(hub)
        .invoke_handler(commands::handler())
        .setup(|app| {
            window::create(app)?;
            Ok(())
        })
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                tauri::Manager::state::<engine::EngineHub>(handle).shutdown();
            }
        }),
        Err(e) => panic!("the desktop app could not start: {e}"),
    }
}
