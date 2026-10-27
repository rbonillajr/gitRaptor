//! Static fitness checks of the TUI (ADR-CKP-003 § 5 and Validation V5,
//! INF-CKP-001). They read the sources: no build of anything else.
//!
//! - The TUI modules (`tui`, `model`, `client`, `present`, `queue`) never
//!   import the engine (`gitraptor_core`), the Git layer (`gitraptor_git`)
//!   nor the policy layer (`gitraptor_policy`), with no exception: the
//!   channel client lives in `crates/api` (INF-CKP-001, Entrega 2b), and the
//!   engine's launcher is injected by the binary (ADR-CKP-003, Enmienda
//!   2026-10-08).
//! - `apps/cli` does not depend on the Git or the policy layer outside its
//!   tests.
//! - No TUI module launches processes (the editor launcher, `tui::editor`,
//!   is the only one allowed, DEP-CKP-12).
//! - The model and the view never hold contract text raw: no `Untrusted`
//!   type outside `present` and `client` (SEC-12, by type).

use std::path::{Path, PathBuf};

const SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");

/// Modules of the TUI, relative to `src`.
const TUI_MODULES: &[&str] = &[
    "tui", "model.rs", "client", "present", "queue.rs", "paths.rs",
];

const FORBIDDEN_CRATES: &[&str] = &["gitraptor_core", "gitraptor_git", "gitraptor_policy"];

fn rust_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}

/// The source without its unit tests (`#[cfg(test)] mod tests` at the end)
/// and without comments.
fn code(path: &Path) -> String {
    // CRLF on a Windows checkout.
    let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let text = match text.find("#[cfg(test)]\nmod tests") {
        Some(i) => &text[..i],
        None => &text[..],
    };
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn tui_files() -> Vec<PathBuf> {
    let files: Vec<PathBuf> = TUI_MODULES
        .iter()
        .flat_map(|m| rust_files(&Path::new(SRC).join(m)))
        .collect();
    assert!(files.len() >= 10, "the scan found too few files: {files:?}");
    files
}

#[test]
fn the_tui_does_not_import_the_engine_git_or_policy() {
    let mut offenders = Vec::new();
    for file in tui_files() {
        let code = code(&file);
        for krate in FORBIDDEN_CRATES {
            if code.contains(krate) {
                offenders.push(format!("{} imports {krate}", file.display()));
            }
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn every_module_of_the_library_is_covered() {
    let lib = code(&Path::new(SRC).join("lib.rs"));
    let modules: Vec<&str> = lib
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub mod "))
        .map(|m| m.trim_end_matches(';'))
        .collect();
    assert!(!modules.is_empty(), "no module found in lib.rs");
    for module in &modules {
        let covered = TUI_MODULES
            .iter()
            .any(|m| m.trim_end_matches(".rs") == *module);
        assert!(
            covered,
            "module `{module}` is not covered by the boundary check"
        );
    }
    // The former exception is gone for good.
    assert!(
        !Path::new(SRC).join("link.rs").exists(),
        "src/link.rs is back"
    );
}

/// The engine's launcher (on-demand start, autostart, clean environment) is
/// built by the binary only and reaches the TUI as a `Launch` trait object.
#[test]
fn only_the_binary_builds_the_launcher() {
    for file in tui_files() {
        let code = code(&file);
        for name in ["InstalledLauncher", "ClientOptions", "ensure_daemon("] {
            assert!(
                !code.contains(name),
                "{} uses {name} of the engine",
                file.display()
            );
        }
    }
    let tui = code(&Path::new(SRC).join("commands").join("tui.rs"));
    assert!(
        tui.contains(".launcher()"),
        "the binary no longer injects it"
    );
}

#[test]
fn apps_cli_does_not_depend_on_git_or_policy() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let deps = manifest
        .split("\n[")
        .find(|section| section.starts_with("dependencies]"))
        .expect("a [dependencies] section");
    for krate in ["gitraptor-git", "gitraptor-policy"] {
        assert!(!deps.contains(krate), "apps/cli depends on {krate}");
    }
}

#[test]
fn the_tui_launches_no_process() {
    for file in tui_files() {
        if file.ends_with(Path::new("tui").join("editor.rs")) {
            continue;
        }
        let code = code(&file);
        assert!(
            !code.contains("process::Command") && !code.contains("Command::new"),
            "{} launches a process",
            file.display()
        );
    }
}

#[test]
fn the_model_and_the_view_never_hold_untrusted_text() {
    for file in tui_files() {
        let in_ingest = file
            .components()
            .any(|c| c.as_os_str() == "present" || c.as_os_str() == "client");
        // A `*_tests.rs` file is a `#[cfg(test)]` module: it builds engine fixtures, never ships.
        let test_only = file
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with("_tests.rs"));
        if in_ingest || test_only {
            continue;
        }
        let code = code(&file);
        assert!(
            !code.contains("Untrusted"),
            "{} handles untrusted text outside the ingest",
            file.display()
        );
    }
}

/// The source before its first `#[cfg(test)]`, without comments.
fn production(path: &Path) -> String {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()))
        .replace("\r\n", "\n");
    let text = match text.find("#[cfg(test)]") {
        Some(i) => &text[..i],
        None => &text[..],
    };
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// US-CKP-005 (D3, ADR-CKP-003): every text of the view reaches the screen through `Say`
/// (which folds it to ASCII when the symbol set asks): `.render(` appears once in the
/// production part of `view.rs`, inside `impl Say`, and never in `help.rs`.
#[test]
fn the_view_translates_only_through_say() {
    let tui = Path::new(SRC).join("tui");
    let view = production(&tui.join("view.rs"));
    let calls: Vec<usize> = view.match_indices(".render(").map(|(i, _)| i).collect();
    assert_eq!(
        calls.len(),
        1,
        "`.render(` must appear exactly once in view.rs (inside `impl Say`), found {}",
        calls.len()
    );
    let say = view
        .find("impl Say")
        .expect("view.rs has no `impl Say` (the single point of translation)");
    assert!(
        calls[0] > say,
        "the only `.render(` of view.rs is outside `impl Say`"
    );
    let help = production(&tui.join("help.rs"));
    assert!(
        !help.contains(".render("),
        "help.rs translates by itself: it must go through Say"
    );
}
