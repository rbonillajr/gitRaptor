//! Helpers for the profile integration tests. Every test uses a temporary
//! profile and temporary repos; nothing touches the real profile or this
//! repo (NFR-01).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gitraptor_core::profile::{
    Agent, AgentKind, NewEvent, Origin, Profile, ProfileDirs, Timestamp, WriteOp,
};

pub struct TempProfile {
    pub root: tempfile::TempDir,
}

impl TempProfile {
    pub fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    pub fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(self.root.path().join("profile"))
    }

    pub fn open(&self) -> Profile {
        Profile::open(self.dirs()).unwrap().0
    }
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git must be installed");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// `git init` in `parent/name`, optionally with one commit.
pub fn init_repo(parent: &Path, name: &str, with_commit: bool) -> PathBuf {
    let repo = parent.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    if with_commit {
        std::fs::write(repo.join("README"), "hello\n").unwrap();
        git(&repo, &["add", "README"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
    }
    repo
}

/// Absolute Git common directory of a repo or worktree, as `crates/git`
/// will resolve it for the daemon.
pub fn common_dir(worktree: &Path) -> PathBuf {
    PathBuf::from(git(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ))
}

pub fn agent() -> Agent {
    Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
    }
}

pub fn ts(ms: i64) -> Timestamp {
    Timestamp {
        utc_ms: ms,
        offset_s: 3600,
    }
}

/// A batch that registers a worktree, starts a session and appends `n`
/// events to it.
pub fn sample_batch(worktree: &Path, session: &str, n: usize) -> Vec<WriteOp> {
    let mut ops = vec![
        WriteOp::UpsertWorktree {
            path: worktree.to_path_buf(),
            admin_name: None,
            seen_ms: 1,
        },
        WriteOp::StartSession {
            session_id: session.to_owned(),
            worktree: worktree.to_path_buf(),
            agent: agent(),
            origin: Origin::Detected,
            detection_key: Some("pid:1".into()),
            started_ms: 1,
        },
    ];
    for i in 0..n {
        ops.push(event(
            worktree,
            Some(session),
            &format!("{{\"ref\":\"refs/heads/b{i}\"}}"),
        ));
    }
    ops
}

pub fn event(worktree: &Path, session: Option<&str>, metadata: &str) -> WriteOp {
    WriteOp::AppendEvent(NewEvent {
        worktree: worktree.to_path_buf(),
        kind: "ref-updated".into(),
        metadata: metadata.into(),
        observed: ts(2),
        session_id: session.map(str::to_owned),
        evidence: Some("S3".into()),
        gap_id: None,
    })
}

/// Every regular file under `dir`, recursively.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}
