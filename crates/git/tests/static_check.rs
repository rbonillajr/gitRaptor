//! ADR-GRP-009, Validación 5: process launching appears only in the invocation module of
//! `crates/git` (`src/invoke.rs`).

use std::path::{Path, PathBuf};

const AUTHORIZED: &[&str] = &["invoke.rs"];
const PATTERNS: &[&str] = &[
    "Command::new",
    "process::Command",
    "std::process",
    "CommandExt",
    "libc::exec",
    "posix_spawn",
];

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

/// Under `repo_intact::` so the single CI gate of INF-GRP-001 selects it.
mod repo_intact {
    use super::*;

    #[test]
    fn process_spawn_only_in_invoke_module() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        assert!(files.len() > 1);
        let mut offenders = Vec::new();
        for file in files {
            let name = file
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if AUTHORIZED.contains(&name.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for p in PATTERNS {
                if text.contains(p) {
                    offenders.push(format!("{name}: {p}"));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "process launch outside invoke.rs: {offenders:?}"
        );
    }
}

/// ADR-TMC-002 § 1 (Enmienda 2026-10-04) and Validación 1: gitoxide writes only in the store
/// writer of the Time Machine write layer (`src/tm_write/store/`).
const GIX_WRITE_PATTERNS: &[&str] = &[
    "write_blob",
    "write_object",
    "write_buf",
    "write_stream",
    "edit_tree",
    "new_commit",
    "commit_as",
    "edit_reference",
    ".reference(",
    "transaction(",
    "init_bare",
    "gix::init",
    "ThreadSafeRepository::init",
    "write_data_iter_to_stream",
    "index::File::write",
];

mod repo_intact_tm {
    use super::*;

    #[test]
    fn gix_writes_only_in_store_writer() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let store = src.join("tm_write").join("store");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let mut offenders = Vec::new();
        let mut store_files = 0;
        for file in files {
            if file.starts_with(&store) {
                store_files += 1;
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for p in GIX_WRITE_PATTERNS {
                if text.contains(p) {
                    offenders.push(format!("{}: {p}", file.display()));
                }
            }
        }
        assert!(store_files > 0, "store writer not found");
        assert!(
            offenders.is_empty(),
            "gitoxide write outside tm_write/store: {offenders:?}"
        );
    }

    #[test]
    fn reader_does_not_expose_the_repository() {
        let reader =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/reader.rs"))
                .unwrap();
        assert!(reader.contains("pub(crate) repo: gix::Repository"));
        assert!(!reader.contains("pub repo:"));
        assert!(!reader.contains("-> &gix::Repository"));
    }
}

/// ADR-TMC-002 § 2 and Validación 1, SEC-TMC-02, SEC-TMC-05: the write layer of the Time Machine
/// runs a closed list of plumbing commands, without porcelain or remote operations, only through
/// the write profile of `invoke.rs`; and its file system writes on the user's repository live in
/// `src/tm_write/`.
mod repo_intact_tm_write {
    use super::*;

    /// Plumbing commands the write profile may run (first word of each variant).
    const ALLOWED: &[&str] = &[
        "update-ref",
        "update-index",
        "pack-objects",
        "index-pack",
        "repack",
        "prune",
    ];
    /// Porcelain and remote commands that must never appear in the write profile.
    const FORBIDDEN: &[&str] = &[
        "push",
        "fetch",
        "pull",
        "clone",
        "remote",
        "ls-remote",
        "send-pack",
        "receive-pack",
        "upload-pack",
        "fetch-pack",
        "http-push",
        "checkout",
        "restore",
        "switch",
        "commit",
        "add",
        "stash",
        "reset",
        "merge",
        "rebase",
        "worktree",
        "gc",
        "filter-branch",
    ];

    fn write_subcommand_words() -> Vec<String> {
        let src =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/invoke.rs"))
                .unwrap()
                // A checkout with CRLF (Git for Windows without `.gitattributes`) is the same source.
                .replace("\r\n", "\n");
        let start = src
            .find("impl WriteSubcommand")
            .expect("WriteSubcommand impl");
        let body = &src[start..];
        let end = body.find("\n}\n").expect("end of impl");
        body[..end]
            .lines()
            .filter_map(|l| l.split_once("=> &[\""))
            .map(|(_, rest)| rest.split('"').next().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn write_profile_is_a_closed_plumbing_list() {
        let words = write_subcommand_words();
        assert!(words.len() >= ALLOWED.len(), "{words:?}");
        for w in &words {
            assert!(ALLOWED.contains(&w.as_str()), "not in the closed list: {w}");
            assert!(!FORBIDDEN.contains(&w.as_str()), "porcelain or remote: {w}");
        }
    }

    #[test]
    fn write_profile_neutralizes_configurable_code() {
        let src =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/invoke.rs"))
                .unwrap()
                // A checkout with CRLF (Git for Windows without `.gitattributes`) is the same source.
                .replace("\r\n", "\n");
        for needle in [
            "\"GIT_CONFIG_NOSYSTEM\", \"1\"",
            "\"GIT_CONFIG_GLOBAL\"",
            "\"core.hooksPath=\"",
            "\"gpg.program=\"",
            "\"protocol.allow=never\"",
            "\"--git-dir=\"",
            "\"--work-tree=\"",
            "\"core.fsmonitor=false\"",
        ] {
            assert!(src.contains(needle), "write profile lacks {needle}");
        }
    }

    #[test]
    fn only_the_write_layer_runs_the_write_profile() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let allowed = [
            src.join("invoke.rs"),
            src.join("tm_write").join("cli.rs"),
            // `repack` and `prune` of the store name their variant (ADR-TMC-007 § 4).
            src.join("tm_write").join("store").join("mod.rs"),
        ];
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| !allowed.contains(f))
            .filter(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                text.contains("run_write") || text.contains("WriteSubcommand")
            })
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "write profile used outside tm_write/cli.rs: {offenders:?}"
        );
        // And the write layer never takes the read profile's entry points.
        let mut tm = Vec::new();
        rust_files(&src.join("tm_write"), &mut tm);
        for f in tm {
            let text = std::fs::read_to_string(&f).unwrap();
            assert!(
                !text.contains("GitCli"),
                "{} uses the read CLI",
                f.display()
            );
        }
    }

    #[test]
    fn file_system_writes_on_repos_only_in_the_write_layer() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let tm = src.join("tm_write");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| !f.starts_with(&tm))
            .filter(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                FS_WRITE_PATTERNS.iter().any(|p| text.contains(p))
            })
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "file system writes outside tm_write: {offenders:?}"
        );
    }
}

/// Root-relative write calls of the applier (ADR-TMC-002 § 3, step 6).
const FS_WRITE_PATTERNS: &[&str] = &[
    "renameat",
    "unlinkat",
    "mkdirat",
    "symlinkat",
    "RenameFlags",
];
