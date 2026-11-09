//! D3-WIRING (DS-US-DSK-003 T005, B-3, W137): the app connects to the engine only through
//! `BundleConnector`, which checks who serves the socket. `lib.rs` registers the hub with it and
//! shuts it down on exit; `SocketConnector` (no B-3) stays in `engine/link.rs`, for tests.

use std::path::{Path, PathBuf};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    out
}

/// Source text without whitespace, so the check does not depend on how rustfmt wraps lines.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn the_app_connects_only_through_the_bundle_connector() {
    let lib = std::fs::read_to_string(Path::new(ROOT).join("src/lib.rs")).unwrap();
    let lib = squeezed(&lib);
    assert!(
        lib.contains("EngineHub::new(Box::new(")
            && lib.contains("BundleConnector::new()")
            && lib.contains("bundled_raptor()"),
        "D3-WIRING: lib.rs must register EngineHub::new(Box::new(...BundleConnector::new()), ...bundled_raptor())"
    );
    assert!(
        lib.contains("RunEvent::Exit") && lib.contains(".shutdown()"),
        "D3-WIRING: lib.rs must call shutdown() on RunEvent::Exit"
    );

    let src = Path::new(ROOT).join("src");
    let link = src.join("engine/link.rs");
    let offenders: Vec<String> = rust_files(&src)
        .into_iter()
        .filter(|f| *f != link)
        .filter(|f| {
            std::fs::read_to_string(f)
                .unwrap()
                .contains("SocketConnector")
        })
        .map(|f| f.strip_prefix(ROOT).unwrap().display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "D3-WIRING: SocketConnector (no B-3) must not appear outside engine/link.rs: {offenders:?}"
    );
}
