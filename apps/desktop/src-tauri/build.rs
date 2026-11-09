//! Builds the Tauri context with the app manifest of `commands.json`, the single list of IPC
//! commands: each one then needs its `allow-<command>` permission in the capability.

use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=commands.json");
    let text = fs::read_to_string("commands.json")
        .unwrap_or_else(|e| panic!("cannot read commands.json: {e}"));
    let list: Vec<String> = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("commands.json must be an array of strings: {e}"));
    // The manifest wants `&'static` names; the build script ends right after, so leaking is free.
    let names: Vec<&'static str> = list
        .into_iter()
        .map(|n| &*Box::leak(n.into_boxed_str()))
        .collect();
    let names: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(names));
    if let Err(e) = tauri_build::try_build(attributes) {
        panic!("tauri-build failed: {e:#}");
    }
}
