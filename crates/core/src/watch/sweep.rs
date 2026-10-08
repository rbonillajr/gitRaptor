//! The metadata sweep of dormant repos (ADR-GRP-010, Enmienda 2026-10-07,
//! N3): one task for every dormant repo, which compares a fingerprint made
//! of file metadata only. It never spawns `git` and never opens a repo with
//! the read-only layer (ADR-GRP-009): it stats files, reads `HEAD` (at most
//! 4 KiB) and lists `.git/worktrees/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Most bytes of a `HEAD` read for the fingerprint.
const MAX_HEAD: u64 = 4 * 1024;

/// Most directories of `refs/heads` walked per repo: a repo with more
/// branch folders than this wakes on the files it did walk.
const MAX_REF_DIRS: usize = 4096;

/// Files of a worktree's Git directory whose size and mtime are compared.
const WORKTREE_FILES: &[&str] = &[
    "logs/HEAD",
    "index",
    "MERGE_HEAD",
    "ORIG_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];

/// Files of the common directory whose size and mtime are compared.
const REPO_FILES: &[&str] = &["packed-refs", "FETCH_HEAD"];

/// A repo's metadata at one moment. Two equal prints mean nothing the
/// sweep can see changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Print(BTreeMap<String, String>);

impl Print {
    /// Reads the print of a repo: its common directory and the private Git
    /// directory of each worktree, without following links.
    pub fn read(common: &Path, git_dirs: &[PathBuf]) -> Self {
        let mut out = BTreeMap::new();
        for file in REPO_FILES {
            out.insert(format!("repo:{file}"), stat(&common.join(file)));
        }
        let mut admin: Vec<String> = std::fs::read_dir(common.join("worktrees"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        admin.sort();
        out.insert("repo:worktrees".into(), admin.join("\n"));
        let mut dirs = vec![common.join("refs").join("heads")];
        let mut walked = 0;
        while let Some(dir) = dirs.pop() {
            walked += 1;
            if walked > MAX_REF_DIRS {
                break;
            }
            out.insert(format!("refs:{}", dir.display()), stat(&dir));
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    dirs.push(entry.path());
                }
            }
        }
        for git_dir in git_dirs {
            let key = git_dir.display();
            out.insert(format!("wt:{key}:HEAD"), head(&git_dir.join("HEAD")));
            for file in WORKTREE_FILES {
                out.insert(format!("wt:{key}:{file}"), stat(&git_dir.join(file)));
            }
        }
        Self(out)
    }
}

/// Size and mtime of a path, without following links; empty if absent.
fn stat(path: &Path) -> String {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}|{mtime}", meta.len())
        }
        Err(_) => String::new(),
    }
}

/// The first bytes of a regular `HEAD` file.
fn head(path: &Path) -> String {
    use std::io::Read;
    if !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        return String::new();
    }
    let mut buf = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(MAX_HEAD).read_to_end(&mut buf));
    match read {
        Ok(_) => String::from_utf8_lossy(&buf).into_owned(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "raptor-sweep-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(dir.join("refs/heads/feat")).unwrap();
        std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        dir
    }

    /// The print follows `HEAD`, the reflog, a new branch folder and a new
    /// linked worktree, and nothing else.
    #[test]
    fn the_print_moves_with_git_metadata_only() {
        let common = tmp();
        let dirs = vec![common.clone()];
        let before = Print::read(&common, &dirs);
        assert_eq!(before, Print::read(&common, &dirs));

        std::fs::write(common.join("HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        let head = Print::read(&common, &dirs);
        assert_ne!(before, head);

        std::fs::create_dir_all(common.join("logs")).unwrap();
        std::fs::write(common.join("logs/HEAD"), "a b c\n").unwrap();
        let log = Print::read(&common, &dirs);
        assert_ne!(head, log);

        std::fs::create_dir_all(common.join("worktrees/w1")).unwrap();
        let linked = Print::read(&common, &dirs);
        assert_ne!(log, linked);

        // A file outside the metadata does not move it.
        std::fs::write(common.join("description"), "x").unwrap();
        assert_eq!(linked, Print::read(&common, &dirs));
        let _ = std::fs::remove_dir_all(&common);
    }
}
