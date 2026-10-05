//! Shared fixtures: temporary repositories (never this repo, NFR-01) and the byte-level
//! fingerprint used to prove that a read leaves the repository untouched (ADR-GRP-009,
//! Validación 1).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use gitraptor_git::cli::{ConfigKey, GitCli, RefNamespace};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::{Invoker, ReaderOptions, RefName, RepoReader, SystemGit};

/// The Git of this machine, resolved by the layer itself.
pub fn system_git() -> SystemGit {
    match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default()) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => {
            panic!("tests need Git >= 2.38 on this machine: {diagnostics:?}")
        }
    }
}

/// A temporary home and a repository inside it.
pub struct Fixture {
    pub tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub repo: PathBuf,
    pub git: SystemGit,
}

impl Fixture {
    /// An empty repository with `main` as initial branch.
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Canonical paths: on macOS `/var` is a symlink to `/private/var`.
        let root = gitraptor_testkit::fixture::canonical_dir(tmp.path());
        let home = root.join("home");
        let repo = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = Test\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let fixture = Self {
            tmp,
            home,
            repo,
            git: system_git(),
        };
        fixture.git(&["init", "-q"]);
        fixture
    }

    /// A repository with one commit holding `a.txt` and `b.txt`.
    pub fn with_commit() -> Self {
        let f = Self::new();
        f.write("a.txt", "alpha\n");
        f.write("b.txt", "beta\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "initial"]);
        f
    }

    pub fn root(&self) -> PathBuf {
        self.repo.parent().unwrap().to_owned()
    }

    /// Run the fixture's own Git (test setup, not the layer under test) in the repo.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.repo, args)
    }

    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_command(dir, args).output().expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn git_command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(&self.git.path);
        c.args(args)
            .current_dir(dir)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "2026-10-04T10:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-10-04T10:00:00Z");
        c
    }

    pub fn write(&self, rela: &str, content: &str) {
        let p = self.repo.join(rela);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    /// Change the mtime of a tracked file without changing its size, so the index stat is dirty.
    pub fn dirty_stat(&self, rela: &str) {
        let p = self.repo.join(rela);
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(120))
            .unwrap();
    }

    /// An invoker whose children see only this fixture's home.
    pub fn invoker(&self) -> Invoker {
        Invoker::default().with_parent_env([
            ("HOME", self.home.as_os_str()),
            ("PATH", "/usr/bin:/bin".as_ref()),
        ])
    }

    /// Fingerprint of the whole fixture root: home, repo (with `.git`) and linked worktrees.
    pub fn fingerprint(&self) -> Fingerprint {
        fingerprint(&self.root())
    }
}

/// Subcommands of the CLI allowlist (ADR-GRP-009 § 3).
pub const ALLOWED: &[&str] = &[
    "version",
    "rev-parse",
    "for-each-ref",
    "worktree",
    "rev-list",
    "merge-base",
    "log",
    "config",
];

/// Fixed options before every subcommand (ADR-GRP-009 § 3).
pub const FIXED_PREFIX: &[&str] = &[
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=keep",
    "-c",
    "core.splitIndex=false",
    "-c",
    "gc.auto=0",
    "-c",
    "maintenance.auto=false",
    "-c",
    "log.showSignature=false",
    "-c",
    "credential.helper=",
    "-c",
    "color.ui=false",
    "-c",
    "core.pager=cat",
    "-c",
    "trace2.normalTarget=",
    "-c",
    "trace2.eventTarget=",
    "-c",
    "trace2.perfTarget=",
];

/// Path → (kind, size, content hash, mtime) of every file, directory and symlink.
pub type Fingerprint = BTreeMap<PathBuf, (char, u64, u64, Option<SystemTime>)>;

pub fn fingerprint(root: &Path) -> Fingerprint {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Fingerprint) {
    let meta = std::fs::symlink_metadata(dir).unwrap();
    out.insert(
        dir.strip_prefix(root).unwrap().to_owned(),
        ('d', 0, 0, meta.modified().ok()),
    );
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        let rel = path.strip_prefix(root).unwrap().to_owned();
        if meta.is_dir() {
            walk(root, &path, out);
        } else if meta.is_symlink() {
            let target = std::fs::read_link(&path).unwrap();
            out.insert(
                rel,
                ('l', 0, hash(target.as_os_str().as_encoded_bytes()), None),
            );
        } else {
            let content = std::fs::read(&path).unwrap();
            out.insert(rel, ('f', meta.len(), hash(&content), meta.modified().ok()));
        }
    }
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Assert two fingerprints are identical, listing every difference.
pub fn assert_unchanged(before: &Fingerprint, after: &Fingerprint, what: &str) {
    let mut diffs = Vec::new();
    for (path, b) in before {
        match after.get(path) {
            None => diffs.push(format!("removed {}", path.display())),
            Some(a) if a != b => diffs.push(format!("changed {}", path.display())),
            _ => {}
        }
    }
    for path in after.keys().filter(|p| !before.contains_key(*p)) {
        diffs.push(format!("created {}", path.display()));
    }
    assert!(diffs.is_empty(), "{what} modified the repo: {diffs:#?}");
}

/// Write an executable script owned by the current user with mode 0755.
///
/// A child shell writes it, so this process never holds a writable descriptor on it. On Linux,
/// a descriptor held here while another test thread forks is inherited by that child until its
/// `execve` closes it, and executing the script in that window fails with `ETXTBSY`.
#[cfg(unix)]
pub fn script(path: &Path, body: &str) {
    let status = Command::new("/bin/sh")
        .args([
            "-c",
            r#"printf '%s\n' '#!/bin/sh' "$2" > "$1" && chmod 0755 "$1""#,
            "sh",
        ])
        .arg(path)
        .arg(body)
        .env_clear()
        .status()
        .expect("run /bin/sh");
    assert!(status.success(), "writing {} failed", path.display());
}

/// Assert that `args` (an argv without the program) is a call of the allowlist: the fixed
/// options, an allowlisted subcommand, no `status`/`diff`, no config listing, no `%G*`.
pub fn assert_allowlisted(args: &[String]) {
    assert!(args.len() > FIXED_PREFIX.len(), "{args:?}");
    assert_eq!(&args[..FIXED_PREFIX.len()], FIXED_PREFIX, "{args:?}");
    let sub = &args[FIXED_PREFIX.len()];
    assert!(
        ALLOWED.contains(&sub.as_str()),
        "{sub} not allowlisted: {args:?}"
    );
    for arg in &args[FIXED_PREFIX.len()..] {
        assert!(
            !["status", "diff", "--list", "--get-regexp", "-l"].contains(&arg.as_str()),
            "{arg} in {args:?}"
        );
        assert!(!arg.contains("%G"), "{arg}");
    }
}

/// An invoker whose children see only `home` and `path_env`.
pub fn invoker_for(home: &Path, path_env: &std::ffi::OsStr) -> Invoker {
    Invoker::default().with_parent_env([("HOME", home.as_os_str()), ("PATH", path_env)])
}

/// Run every read of the layer, gitoxide and CLI, against `path`.
pub fn read_everything(f: &Fixture, path: &std::path::Path) {
    read_everything_with(&f.git, &f.invoker(), path);
}

/// [`read_everything`] for any fixture: the Git and invoker are given.
pub fn read_everything_with(git: &SystemGit, invoker: &Invoker, path: &Path) {
    let r = RepoReader::open(path, &ReaderOptions::default()).expect("open");
    let main = RefName::new("main").unwrap();
    let feature = RefName::new("feature").unwrap();
    r.head().unwrap();
    r.local_branches().unwrap();
    r.index_entry_count().unwrap();
    r.status().unwrap();
    r.worktrees().unwrap();
    r.in_progress();
    r.is_ignored("target", true).unwrap();
    r.remote_url(&RefName::new("origin").unwrap()).unwrap();
    r.resolve_ref(&main).unwrap();
    r.merge_base(&main, &feature).unwrap();
    r.ahead_behind(&main, &feature, 1000).unwrap();
    drop(r);

    let cli = GitCli::new(git, invoker, path).unwrap();
    cli.rev_parse_verify(&main).unwrap();
    cli.for_each_ref(RefNamespace::Heads).unwrap();
    cli.worktree_list().unwrap();
    cli.rev_list_left_right_count(&main, &feature).unwrap();
    cli.merge_base(&main, &feature).unwrap();
    cli.log(&main, 10).unwrap();
    cli.config_get(&ConfigKey::InitDefaultBranch).unwrap();
    cli.config_get(&ConfigKey::RemoteUrl(RefName::new("origin").unwrap()))
        .unwrap();
}

/// A repo with history on two branches, a remote, an ignore rule and pending changes.
pub fn busy_repo() -> Fixture {
    let f = Fixture::with_commit();
    f.git(&["branch", "feature"]);
    f.write(".gitignore", "target/\n");
    f.git(&["add", ".gitignore"]);
    f.git(&["commit", "-q", "-m", "ignore"]);
    f.git(&["checkout", "-q", "feature"]);
    f.write("c.txt", "gamma\n");
    f.git(&["add", "c.txt"]);
    f.git(&["commit", "-q", "-m", "feature"]);
    f.git(&["checkout", "-q", "main"]);
    f.git(&[
        "remote",
        "add",
        "origin",
        "https://user:token@example.com/o/r.git",
    ]);
    f.write("a.txt", "alpha changed\n");
    f.write("staged.txt", "staged\n");
    f.git(&["add", "staged.txt"]);
    f.write("untracked.txt", "u\n");
    std::fs::create_dir_all(f.repo.join("target")).unwrap();
    f.write("target/out.bin", "x");
    f.dirty_stat("b.txt");
    f
}
