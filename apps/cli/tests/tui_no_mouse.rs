//! The TUI never captures the mouse (ADR-CKP-003 § 10, US-CKP-005 scenario 5): the terminal's
//! own selection and scrolling stay with the user, and every action has a key.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read the TUI sources") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_tui_never_captures_the_mouse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let mut files = Vec::new();
    sources(&root, &mut files);
    assert!(!files.is_empty(), "no TUI sources found under {root:?}");
    for file in files {
        let text = fs::read_to_string(&file).expect("read a TUI source");
        // This file's own needle is built so the scan never matches itself.
        assert!(
            !text.contains(&["Enable", "MouseCapture"].concat()),
            "{} captures the mouse",
            file.display()
        );
    }
}
