//! TS-GRP-005 T006 (RES-06, ADR-GRP-016): no thread of `crates/core/src` is created without
//! declaring its class. A `thread::Builder::new()` or `thread::spawn` outside `work_class/`
//! must be in the list below, with the reason it keeps the class it inherits.
//!
//! T010 (CLS2): the prior-snapshot path is no longer excepted: `raptor-conn`, `raptor-prior`,
//! `raptor-tm-capture`, `raptor-tm-seed` and the hashing threads of a capture
//! (`std::thread::scope`) declare their class too. A `thread::scope` counts as a thread
//! creation and must call `set_current(` within `SCOPE_DECLARES_WITHIN` lines of it. D11: a
//! file that takes the store writer (`.writer(`) never declares `Utility` or `Background`.
//!
//! Test code (`#[cfg(test)]` modules, `*tests*.rs` files) is out of scope.

use std::path::{Path, PathBuf};

/// (file under `crates/core/src`, thread name, reason).
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    // Inherited `Default` (DS-TS-GRP-005, "Clase por hilo").
    (
        "watch/mod.rs",
        "raptor-watch-worktree",
        "observer: Default; Utility only inside Task::periodic",
    ),
    (
        "watch/mod.rs",
        "raptor-watch-repo",
        "observer: Default; Utility only inside Task::periodic",
    ),
    (
        "watch/mod.rs",
        "raptor-sentinel",
        "inherited Default: wakes repos",
    ),
    (
        "watch/exclusions.rs",
        "raptor-watch-exclusions",
        "inherited Default",
    ),
    (
        "daemon/modules/predict.rs",
        "raptor-predict",
        "inherited Default; TS-CKP-001 decides",
    ),
    ("detect/mod.rs", "raptor-sessions", "inherited Default"),
    ("detect/mod.rs", "raptor-s3", "inherited Default"),
    (
        "channel/server.rs",
        "raptor-accept",
        "inherited Default: channel",
    ),
    (
        "channel/conn.rs",
        "raptor-conn-out",
        "inherited Default: event stream",
    ),
    ("daemon/shutdown.rs", "raptor-signals", "inherited Default"),
    ("daemon/mod.rs", "raptor-stop-deadline", "inherited Default"),
    ("executor/git.rs", "raptor-time-limit", "inherited Default"),
    // Client side: outside the daemon.
    (
        "client.rs",
        "raptor-daemon-reaper",
        "client side, out of scope",
    ),
];

/// Lines after a `thread::scope(` where each scoped thread must declare its class.
const SCOPE_DECLARES_WITHIN: usize = 12;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap();
    name == "tests.rs"
        || name.contains("_tests")
        || name.contains("tests_")
        || rel.contains("/tests/")
}

/// The thread creations of non-test code: (line number, thread name).
fn thread_creations(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        if line.contains("thread::Builder::new()") {
            let name = lines[i..(i + 5).min(lines.len())]
                .iter()
                .find_map(|l| {
                    let rest = l.split(".name(").nth(1)?;
                    let rest = rest.strip_prefix('"')?;
                    Some(rest.split('"').next()?.to_string())
                })
                .unwrap_or_else(|| "<unnamed>".into());
            found.push((i + 1, name));
        } else if line.contains("thread::spawn(") {
            found.push((i + 1, "<unnamed>".into()));
        } else if line.contains("thread::scope(") {
            let end = (i + 1 + SCOPE_DECLARES_WITHIN).min(lines.len());
            let declared = lines[i..end].iter().any(|l| l.contains("set_current("));
            if !declared {
                found.push((i + 1, "<scope>".into()));
            }
        }
    }
    found
}

#[test]
fn every_daemon_thread_declares_its_class() {
    for (_, _, reason) in EXCEPTIONS {
        assert!(!reason.is_empty(), "every exception states its reason");
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();

    let mut undeclared = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("work_class/") || is_test_file(&rel) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (line, name) in thread_creations(&text) {
            // Scoped threads are checked only on the Time Machine path (D18); other scopes
            // (`daemon::reconcile_all`) are out of this guard.
            if name == "<scope>" && !rel.starts_with("timemachine/") {
                continue;
            }
            let allowed = EXCEPTIONS.iter().any(|(f, n, _)| *f == rel && *n == name);
            if !allowed {
                undeclared.push(format!("{rel}:{line} `{name}`"));
            }
        }
    }
    assert!(
        undeclared.is_empty(),
        "threads created with thread::Builder::new()/thread::spawn outside work_class/ \
         without a declared class (use work_class::spawn or add a reasoned exception):\n{}",
        undeclared.join("\n")
    );
}

/// D11 (inversion of priority): no section that holds the store writer runs below `Default`.
/// A file that calls `.writer(` must not name `WorkClass::Utility` or `WorkClass::Background`.
#[test]
fn a_file_that_takes_the_writer_never_declares_a_lower_class() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    let mut offenders = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_file(&rel) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let code: String = text
            .lines()
            .take_while(|l| !l.trim_start().starts_with("#[cfg(test)]"))
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        if code.contains(".writer(")
            && (code.contains("WorkClass::Utility") || code.contains("WorkClass::Background"))
        {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "files that take the store writer must not declare Utility or Background (D11, D18): {offenders:?}"
    );
}
