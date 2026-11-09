//! WIN1 (TS-DSK-001 T001, M-3): the window navigates only to the origins of `frontendDist`
//! (`tauri://localhost`, `http://tauri.localhost`) and, in debug builds, `http://localhost:1420`.

use gitraptor_desktop::window::is_allowed_navigation;
use tauri::Url;

fn allowed(url: &str) -> bool {
    is_allowed_navigation(&Url::parse(url).unwrap())
}

#[test]
fn win1_frontend_dist_origins_are_allowed() {
    assert!(
        allowed("tauri://localhost/index.html"),
        "WIN1: tauri://localhost must be allowed"
    );
    assert!(
        allowed("tauri://localhost"),
        "WIN1: tauri://localhost root must be allowed"
    );
    assert!(
        allowed("http://tauri.localhost/"),
        "WIN1: http://tauri.localhost must be allowed"
    );
    assert!(
        allowed("http://tauri.localhost/any/path?q=1"),
        "WIN1: any path on tauri.localhost"
    );
}

#[test]
fn win1_dev_server_is_allowed_only_in_debug() {
    assert_eq!(
        allowed("http://localhost:1420/"),
        cfg!(debug_assertions),
        "WIN1: http://localhost:1420 is allowed exactly when debug_assertions is on"
    );
}

#[test]
fn win1_everything_else_is_rejected() {
    for url in [
        "https://example.com",
        "https://tauri.localhost/",
        "http://localhost:1421/",
        "http://localhost/",
        "http://localhost.evil.com:1420/",
        "http://tauri.localhost.evil.com/",
        "tauri://evil/",
        "file:///etc/passwd",
        "about:blank",
        "data:text/html,<h1>x</h1>",
        "javascript:alert(1)",
        "ws://localhost:1420/",
    ] {
        assert!(!allowed(url), "WIN1: {url} must be rejected");
    }
}
