//! The main window, created in code so it carries the navigation and new-window handlers: the CSP
//! does not stop a top-level navigation (`location.href`) nor `window.open`.

use tauri::webview::NewWindowResponse;
use tauri::{App, Url, WebviewUrl, WebviewWindowBuilder};

/// Label of the only window; the capability `main` is bound to it.
pub const MAIN: &str = "main";

/// Dev server of `tauri.dev.conf.json` (`http://localhost:1420`), only reachable in debug builds.
const DEV_HOST: &str = "localhost";
const DEV_PORT: u16 = 1420;

/// Creates the main window.
///
/// # Errors
///
/// Returns the Tauri error when the webview cannot be built.
pub fn create(app: &App) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("GitRaptor")
        .inner_size(1440.0, 900.0)
        .min_inner_size(720.0, 560.0)
        .content_protected(true)
        .on_navigation(is_allowed_navigation)
        .on_new_window(|_, _| NewWindowResponse::Deny);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build()?;
    Ok(())
}

/// Whether the webview may navigate to `url`: only the origins that serve `frontendDist`
/// (`tauri://localhost` and `http://tauri.localhost`, any path) and, in debug builds, the dev
/// server `http://localhost:1420`. Everything else is denied.
pub fn is_allowed_navigation(url: &Url) -> bool {
    let host = url.host_str();
    let port = url.port();
    match url.scheme() {
        "tauri" => host == Some("localhost") && port.is_none(),
        "http" => {
            (host == Some("tauri.localhost") && port.is_none())
                || (cfg!(debug_assertions) && host == Some(DEV_HOST) && port == Some(DEV_PORT))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed_navigation;
    use tauri::Url;

    fn allowed(url: &str) -> bool {
        match Url::parse(url) {
            Ok(u) => is_allowed_navigation(&u),
            Err(e) => panic!("{url} does not parse: {e}"),
        }
    }

    #[test]
    fn frontend_dist_origins_with_any_path_are_allowed() {
        for url in [
            "tauri://localhost",
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "http://tauri.localhost/any/path?q=1",
        ] {
            assert!(allowed(url), "{url}");
        }
    }

    #[test]
    fn dev_server_only_in_debug() {
        assert_eq!(allowed("http://localhost:1420/"), cfg!(debug_assertions));
    }

    #[test]
    fn other_schemes_hosts_and_ports_are_denied() {
        for url in [
            "https://example.com",
            "https://tauri.localhost/",
            "http://tauri.localhost:8080/",
            "tauri://localhost:1/",
            "http://localhost:1421/",
            "http://localhost/",
            "http://tauri.localhost.evil.com/",
            "http://localhost.evil.com:1420/",
            "tauri://evil/",
            "file:///etc/passwd",
            "about:blank",
            "data:text/html,<h1>x</h1>",
            "javascript:alert(1)",
            "ws://localhost:1420/",
        ] {
            assert!(!allowed(url), "{url}");
        }
    }
}
