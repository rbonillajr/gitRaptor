//! Static fitness checks of the channel client in `crates/api` (INF-CKP-001, Entrega 2b;
//! ADR-CKP-003, Enmienda 2026-10-08). They read the sources: no build of anything else.
//!
//! - The contract crate stays light: the client sits behind the `client` feature, its
//!   OS crates are optional, and it never depends on the engine, `directories` or the
//!   autostart, nor launches a process (the launcher lives in `crates/core`, behind
//!   [`Launch`](gitraptor_api::client::Launch)).
//! - The security checks of the channel's client side (private folder, peer of the
//!   socket, pipe identity) exist once: `crates/core` delegates to this crate and does not
//!   re-implement them.

use std::path::Path;

const API: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(API).join(rel))
        .unwrap()
        .replace("\r\n", "\n")
}

/// The source without its unit tests and without comments.
fn code(rel: &str) -> String {
    let text = read(rel);
    let text = match text.find("#[cfg(test)]\nmod tests") {
        Some(i) => text[..i].to_owned(),
        None => text,
    };
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A `[section]` of a manifest (up to the next section).
fn section<'a>(manifest: &'a str, name: &str) -> Vec<&'a str> {
    manifest
        .split("\n[")
        .filter(|s| s.starts_with(name))
        .collect()
}

const CLIENT_FILES: &[&str] = &[
    "src/client/mod.rs",
    "src/client/transport.rs",
    "src/client/peer.rs",
];

#[test]
fn the_client_is_behind_a_feature_with_optional_os_crates() {
    let manifest = read("Cargo.toml");
    let features = section(&manifest, "features]").join("\n");
    assert!(features.contains("client = ["), "no `client` feature");
    for dep in ["nix", "gitraptor-winsys"] {
        let line = manifest
            .lines()
            .find(|l| l.starts_with(&format!("{dep} ")))
            .unwrap_or_else(|| panic!("{dep} is not a dependency"));
        assert!(line.contains("optional = true"), "{dep} is not optional");
    }
    let lib = code("src/lib.rs");
    assert!(
        lib.contains("#[cfg(feature = \"client\")]\npub mod client;"),
        "the module is not gated by the feature"
    );
    // The workspace lints (`forbid(unsafe_code)`) still apply to this crate.
    assert!(
        section(&manifest, "lints]")
            .join("")
            .contains("workspace = true")
    );
}

#[test]
fn the_client_never_reaches_the_engine_nor_launches_processes() {
    let manifest = read("Cargo.toml");
    for krate in [
        "gitraptor-core",
        "gitraptor-git",
        "gitraptor-policy",
        "directories",
    ] {
        assert!(!manifest.contains(krate), "crates/api depends on {krate}");
    }
    for file in CLIENT_FILES {
        let code = code(file);
        for pattern in ["process::Command", "Command::new", "autostart", "unsafe"] {
            assert!(!code.contains(pattern), "{file} contains {pattern}");
        }
    }
}

#[test]
fn core_delegates_the_client_side_checks_instead_of_reimplementing_them() {
    let core = |rel: &str| {
        let text = std::fs::read_to_string(Path::new(API).join("../core/src").join(rel))
            .unwrap()
            .replace("\r\n", "\n");
        text.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let transport = core("channel/transport.rs");
    for owned_here in [
        "fn connect(",
        "fn socket_path(",
        "fn pipe_name(",
        "fn runs_as_this_user(",
        "fn in_dir<",
        "MAX_SOCKET_PATH: usize",
    ] {
        assert!(
            !transport.contains(owned_here),
            "crates/core/src/channel/transport.rs re-implements {owned_here}"
        );
    }
    let fsperm = core("profile/fsperm.rs");
    assert!(
        !fsperm.contains("expected 700"),
        "crates/core/src/profile/fsperm.rs re-implements the private-folder check"
    );
    let peer = core("channel/peer.rs");
    for owned_here in ["LocalPeerCred", "socket_peercred", "peer_pid()"] {
        assert!(
            !peer.contains(owned_here),
            "crates/core/src/channel/peer.rs re-implements {owned_here}"
        );
    }
    let client = core("client.rs");
    for owned_here in ["fn greet(", "fn read_frame(", "fn accept_capabilities("] {
        assert!(
            !client.contains(owned_here),
            "crates/core/src/client.rs re-implements {owned_here}"
        );
    }
}
