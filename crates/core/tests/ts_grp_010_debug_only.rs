//! TS-GRP-010 (D3, D3-bis, D4): the test paths of the per-requester profile do not exist in a
//! release build. A fitness check over the source: every one of them sits behind
//! `#[cfg(debug_assertions)]` (or `cfg!(debug_assertions)`), so a release daemon can never keep a
//! daemon child, a process without a terminal or the real Claude Code out of the `mcp` profile.

use std::path::Path;

fn source(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Whether the item or statement that `line` belongs to is gated: `#[cfg(debug_assertions)]`
/// between it and the end of the previous item or statement.
fn gated(lines: &[&str], line: usize) -> bool {
    lines[..=line]
        .iter()
        .rev()
        .skip(1)
        .map(|l| l.trim())
        .take_while(|l| !l.ends_with([';', '{', '}', ',']))
        .any(|l| l == "#[cfg(debug_assertions)]")
}

/// Every line of `file` that contains `needle` (and is not a comment) is gated.
fn assert_gated(file: &str, needle: &str, at_least: usize) {
    let text = source(file);
    let lines: Vec<&str> = text.lines().collect();
    let hits: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains(needle) && !l.trim_start().starts_with("//"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        hits.len() >= at_least,
        "{file}: expected {needle} at least {at_least} times, found {}",
        hits.len()
    );
    for i in hits {
        assert!(
            gated(&lines, i),
            "{file}:{}: {needle} must sit behind #[cfg(debug_assertions)]: {}",
            i + 1,
            lines[i]
        );
    }
}

#[test]
fn the_daemon_children_knob_is_debug_only() {
    // The field and its default.
    assert_gated("src/channel/mod.rs", "test_daemon_children_as_self", 2);
    // Its only read in the handshake.
    assert_gated("src/channel/conn.rs", "test_daemon_children_as_self", 1);
}

#[test]
fn the_ttyless_test_mode_is_debug_only() {
    assert_gated("src/channel/requester_profile.rs", "pub fn test_ttyless", 1);
    assert_gated("src/channel/conn.rs", "test_ttyless(", 1);
}

#[test]
fn the_agent_executables_override_is_read_only_in_debug_builds() {
    let env = source("src/daemon/env.rs");
    let body = env
        .split("pub(crate) fn agent_executables_override()")
        .nth(1)
        .expect("agent_executables_override exists");
    let body = &body[..body.find("\n}\n").expect("end of the function")];
    assert!(
        body.contains("if cfg!(debug_assertions)") && body.contains("} else {\n        None"),
        "GITRAPTOR_AGENT_EXECUTABLES must be read only in debug builds: {body}"
    );
}
