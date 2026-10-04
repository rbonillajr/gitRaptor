//! Temporary repositories for the team-level tests (never this repo, NFR-01).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::{Invoker, ReaderOptions, RepoReader};

pub struct Repo {
    _tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub home: PathBuf,
    pub path: PathBuf,
    git: PathBuf,
}

impl Repo {
    /// A repository with `main` and one commit without team settings.
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let path = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = Test\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let git = match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default())
        {
            Resolution::Found { git, .. } => git.path,
            Resolution::NotFound { diagnostics } => panic!("tests need Git: {diagnostics:?}"),
        };
        let repo = Self {
            _tmp: tmp,
            root,
            home,
            path,
            git,
        };
        repo.git(&["init", "-q"]);
        repo.write(&repo.path, "README", "demo\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "initial"]);
        repo
    }

    pub fn command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(&self.git);
        c.args(args)
            .current_dir(dir)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.command(dir, args).output().expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.path, args)
    }

    pub fn write(&self, dir: &Path, rela: &str, content: &str) {
        let p = dir.join(rela);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// Commit `content` as the team settings in `dir` and return the commit.
    pub fn commit_settings_in(&self, dir: &Path, content: &str) -> String {
        self.write(dir, ".gitraptor/settings.json", content);
        self.git_in(dir, &["add", "."]);
        self.git_in(dir, &["commit", "-q", "-m", "settings"]);
        self.git_in(dir, &["rev-parse", "HEAD"])
    }

    pub fn commit_settings(&self, content: &str) -> String {
        self.commit_settings_in(&self.path, content)
    }

    /// Blob id of the team settings in `commit`.
    pub fn settings_blob(&self, commit: &str) -> String {
        self.git(&["rev-parse", &format!("{commit}:.gitraptor/settings.json")])
    }

    /// Write `content` as a loose blob and return its id.
    pub fn hash_blob(&self, content: &[u8]) -> String {
        let mut child = self
            .command(&self.path, &["hash-object", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(content).unwrap();
        let out = child.wait_with_output().unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// A commit whose only tree entry is the team settings with `mode` and object `id`.
    pub fn commit_entry(&self, mode: &str, id: &str) -> String {
        let mut child = self
            .command(&self.path, &["mktree"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let kind = if mode == "160000" { "commit" } else { "blob" };
        writeln!(
            child.stdin.take().unwrap(),
            "{mode} {kind} {id}\tsettings.json"
        )
        .unwrap();
        let dir = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        let mut child = self
            .command(&self.path, &["mktree"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "040000 tree {}\t.gitraptor",
            dir.trim()
        )
        .unwrap();
        let root = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        self.git(&["commit-tree", root.trim(), "-m", "entry"])
    }

    /// Simulate a fetched copy of the main branch: `origin` with `refs/remotes/origin/main`.
    pub fn set_origin_main(&self, commit: &str) {
        if self.git(&["remote"]).lines().all(|r| r != "origin") {
            self.git(&["remote", "add", "origin", "/nonexistent/origin.git"]);
        }
        self.git(&["update-ref", "refs/remotes/origin/main", commit]);
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// A linked worktree on a new branch at `start`.
    pub fn worktree(&self, name: &str, start: &str) -> PathBuf {
        let dir = self.root.join(name);
        self.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            name,
            dir.to_str().unwrap(),
            start,
        ]);
        dir
    }

    pub fn reader(&self) -> RepoReader {
        self.reader_at(&self.path)
    }

    pub fn reader_at(&self, dir: &Path) -> RepoReader {
        RepoReader::open(dir, &ReaderOptions::default()).unwrap()
    }

    /// Path → (size, content hash, mtime) of everything under the fixture root.
    pub fn fingerprint(&self) -> BTreeMap<PathBuf, (u64, u64, Option<std::time::SystemTime>)> {
        let mut out = BTreeMap::new();
        walk(&self.root, &mut out);
        out
    }
}

fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, (u64, u64, Option<std::time::SystemTime>)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let meta = std::fs::symlink_metadata(entry.path()).unwrap();
        let mtime = meta.modified().ok();
        if meta.is_dir() {
            out.insert(entry.path(), (0, 0, mtime));
            walk(&entry.path(), out);
        } else {
            let bytes = std::fs::read(entry.path()).unwrap_or_default();
            let mut h = DefaultHasher::new();
            bytes.hash(&mut h);
            out.insert(entry.path(), (meta.len(), h.finish(), mtime));
        }
    }
}
