//! Temporary machine for one scenario (NFR-01: never this repo, never the real profile):
//!
//! ```text
//! <root>/                      marked as a testkit root (guard)
//!   home/                      HOME of every Git the fixture or the engine launches
//!     .gitconfig .config/git/ .gnupg/ .claude/
//!   repo/                      the observed repository, `.git` included
//!   other-repo/                another repository of the machine
//!   profile/{data,config,state}  the engine profile (ADR-GRP-006 § 1)
//! ```
//!
//! Each top-level folder is a fingerprint [`Scope`] with its own name, so two fixtures built by
//! the same code can be compared in a control run. Linked worktrees created under the root
//! become scopes too. The system Git config, wherever the resolved Git keeps it, is a read-only
//! [`Scope::system`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::exceptions::Exceptions;
use crate::fingerprint::{Scope, Snapshot};
use crate::guard;

/// `~/.gitconfig` of every fixture home. Automatic maintenance is off: a commit or fetch of the
/// fixture would otherwise start `git maintenance run --auto` (Git >= 2.29) or `gc --auto`, in the
/// background by default (`maintenance.autoDetach`, Git >= 2.47), and its lock or pack lands in a
/// fingerprint at random (`objects/maintenance.lock`, `gc.pid`). A test that provokes `gc --auto`
/// on purpose turns it back on in the repo config, which wins over this file.
pub const HOME_GITCONFIG: &str = "[user]\n\tname = Test\n\temail = test@example.com\n\
[init]\n\tdefaultBranch = main\n\
[maintenance]\n\tauto = false\n\tautoDetach = false\n\
[gc]\n\tauto = 0\n\tautoPackLimit = 0\n\tautoDetach = false\n";

/// The first absolute `git` in `PATH`, for the testkit's own tests. Crates under test resolve
/// Git with their own code and pass it to [`Fixture::new`].
pub fn git_from_path() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(format!("git{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
        .expect("tests need git in PATH")
}

/// `path` canonicalized, in the form Git accepts. On Windows `canonicalize` returns a verbatim
/// path (`\\?\C:\…`) and Git cannot read its configuration under a `HOME` of that form ("unknown
/// error occurred while reading the configuration files"); the plain drive form names the same
/// directory. Other OSes never return that prefix and get the canonical path unchanged.
pub fn canonical_dir(path: &Path) -> PathBuf {
    without_verbatim_drive(&path.canonicalize().expect("canonical path"))
}

/// `\\?\C:\…` becomes `C:\…`; any other path (a verbatim UNC path included) is kept as is.
pub fn without_verbatim_drive(path: &Path) -> PathBuf {
    let Some(rest) = path.to_str().and_then(|t| t.strip_prefix(r"\\?\")) else {
        return path.to_owned();
    };
    let b = rest.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        PathBuf::from(rest)
    } else {
        path.to_owned()
    }
}

/// mtime of the `n`-th file written by [`Fixture::write`]: 2026-09-21 plus `n` seconds.
pub fn fixed_mtime(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_790_000_000 + n)
}

pub struct Fixture {
    _tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub home: PathBuf,
    pub repo: PathBuf,
    pub other_repo: PathBuf,
    pub profile: PathBuf,
    /// Git used to build the fixture (setup and user/agent actions, not the code under test).
    pub git: PathBuf,
    /// System config file of `git`, if it has one.
    pub system_config: Option<PathBuf>,
    writes: AtomicU64,
}

impl Fixture {
    /// An empty repository with `main` as initial branch, and the rest of the fake machine.
    pub fn new(git: &Path) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Canonical paths: on macOS `/var` is a symlink to `/private/var`.
        let root = canonical_dir(tmp.path());
        guard::mark(&root);
        let home = root.join("home");
        let repo = root.join("repo");
        let other_repo = root.join("other-repo");
        let profile = root.join("profile");
        for dir in [
            home.join(".gnupg"),
            home.join(".claude"),
            home.join(".config/git"),
            repo.clone(),
            other_repo.clone(),
            profile.join("data"),
            profile.join("config"),
            profile.join("state"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(home.join(".gitconfig"), HOME_GITCONFIG).unwrap();
        std::fs::write(home.join(".config/git/ignore"), "*.swp\n").unwrap();
        std::fs::write(home.join(".gnupg/pubring.kbx"), "keyring").unwrap();
        std::fs::write(home.join(".claude/settings.json"), "{}\n").unwrap();
        std::fs::write(profile.join("config/config.toml"), "# profile config\n").unwrap();
        let fixture = Self {
            _tmp: tmp,
            root,
            home,
            repo,
            other_repo,
            profile,
            git: git.to_owned(),
            system_config: None,
            writes: AtomicU64::new(0),
        };
        fixture.git(&["init", "-q"]);
        fixture.git_in(&fixture.other_repo, &["init", "-q"]);
        let system_config = fixture.find_system_config();
        Self {
            system_config,
            ..fixture
        }
    }

    /// A repository with one commit holding `a.txt` and `b.txt`.
    pub fn with_commit(git: &Path) -> Self {
        let f = Self::new(git);
        f.write("a.txt", "alpha\n");
        f.write("b.txt", "beta\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", "initial"]);
        f
    }

    /// History on two branches, a remote with a token, an ignore rule, staged, unstaged and
    /// untracked changes, an ignored folder and a file with a dirty stat.
    pub fn busy(git: &Path) -> Self {
        let f = Self::with_commit(git);
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
        f.write("target/out.bin", "x");
        f.dirty_stat("b.txt");
        f
    }

    /// Run the fixture's Git in the repo; panics on failure.
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

    /// A deterministic Git command: fixture home, no system config, fixed dates.
    pub fn git_command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(&self.git);
        c.args(args).current_dir(dir);
        if cfg!(unix) {
            c.env_clear().env("PATH", "/usr/bin:/bin");
        } else {
            c.env("USERPROFILE", &self.home);
        }
        c.env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "2026-10-04T10:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-10-04T10:00:00Z");
        c
    }

    /// Write a file of the repo with an mtime in the past, one second later on every write. Two
    /// fixtures built by the same code are then identical; Git never sees a "racy" index entry
    /// (which makes it rehash the file and freshen an existing object at random); and a rewrite
    /// with the same size still changes the stat, so `git add` sees it.
    pub fn write(&self, rela: &str, content: &str) {
        let p = self.repo.join(rela);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, content).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(fixed_mtime(self.writes.fetch_add(1, Ordering::Relaxed)))
            .unwrap();
    }

    /// Change the mtime of a tracked file without changing its size, so the index stat is dirty.
    pub fn dirty_stat(&self, rela: &str) {
        let f = std::fs::File::options()
            .write(true)
            .open(self.repo.join(rela))
            .unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(120))
            .unwrap();
    }

    /// A linked worktree at `<root>/wt-<name>` on `branch`; it becomes its own scope.
    pub fn add_worktree(&self, name: &str, branch: &str) -> PathBuf {
        let wt = self.root.join(format!("wt-{name}"));
        self.git(&["worktree", "add", "-q", wt.to_str().unwrap(), branch]);
        wt
    }

    /// Every top-level folder of the root, plus the system Git config.
    pub fn scopes(&self) -> Vec<Scope> {
        let mut scopes: Vec<Scope> = std::fs::read_dir(&self.root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name() != guard::MARKER)
            .map(|e| Scope::new(e.file_name().to_string_lossy(), e.path()))
            .collect();
        scopes.sort_by(|a, b| a.label.cmp(&b.label));
        if let Some(sys) = &self.system_config {
            scopes.push(Scope::system("system-gitconfig", sys));
        }
        scopes
    }

    /// Snapshot of every scope, keeping what `exceptions` need to compare.
    pub fn snapshot(&self, exceptions: &Exceptions) -> Snapshot {
        Snapshot::take(&self.scopes(), &exceptions.keep_content())
    }

    /// Snapshot with no content kept.
    pub fn fingerprint(&self) -> Snapshot {
        Snapshot::take(&self.scopes(), &BTreeSet::new())
    }

    /// Where the fixture's Git keeps its system config (Homebrew, Command Line Tools, Git for
    /// Windows and distro packages differ), asked to Git itself.
    fn find_system_config(&self) -> Option<PathBuf> {
        let out = Command::new(&self.git)
            .args(["config", "--system", "--show-origin", "--list"])
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let first = text.lines().next()?;
        let path = first.strip_prefix("file:")?.split('\t').next()?;
        let path = PathBuf::from(path);
        path.is_file().then_some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_verbatim_drive_prefix_is_removed() {
        let plain = |p: &str| without_verbatim_drive(Path::new(p));
        assert_eq!(
            plain(r"\\?\C:\Users\dev\tmp"),
            Path::new(r"C:\Users\dev\tmp")
        );
        assert_eq!(
            plain(r"\\?\UNC\server\share"),
            Path::new(r"\\?\UNC\server\share")
        );
        assert_eq!(plain(r"\\?\Volume{x}\dir"), Path::new(r"\\?\Volume{x}\dir"));
        assert_eq!(plain(r"C:\Users\dev"), Path::new(r"C:\Users\dev"));
        assert_eq!(plain("/private/var/tmp"), Path::new("/private/var/tmp"));
    }
}
