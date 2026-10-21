//! US-GRD-007 end to end (DS-US-GRD-007 § 6, PR-A): per-operation permissions with the real
//! `raptor` as daemon and hook, the real `raptor-hook` dispatcher and plain Git, over temporary
//! repos, a bare remote and a temporary profile (NFR-01). The agent is `raptor-fake-agent`, a copy
//! of this test binary the debug daemon knows as Claude Code (agent -> sh -> git -> hook), so the
//! daemon resolves the actor from the ancestry, never from a claim; the person runs `git` with no
//! agent above it. The team configuration is committed on `main` without confirming it (a `deny`
//! already hardens, Q-GRD-23). Every agent commit carries the `agents-commit` trailer, so only the
//! permissions can deny. No fixed waits. Messages are checked by stable fragments of the English
//! templates (the harness forces `LANG=en_US.UTF-8`) and by rule codes in `raptor guard log`.
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{copy_executable, git_from_path};
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The command the simulated agent runs with `/bin/sh`.
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
/// The trailer `agents-commit` (the default) asks of an agent's commit.
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";

// Without debug assertions `raptor` ignores GITRAPTOR_PROFILE_DIR and would use the real profile
// (NFR-01): refuse to build.
#[cfg(not(debug_assertions))]
compile_error!(
    "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
);

const TEAM: &str = "team configuration";
const NEEDS_HUMAN: &str = "human confirmation";

/// Entry point of the simulated Claude Code: when this binary runs as `raptor-fake-agent` with
/// [`AGENT_CMD`], it runs that command and exits with its status. As a normal test, nothing.
#[test]
fn fake_agent_entry() {
    let Some(cmd) = std::env::var_os(AGENT_CMD) else {
        return;
    };
    let status = Command::new("/bin/sh").arg("-c").arg(cmd).status().unwrap();
    std::process::exit(status.code().unwrap_or(2));
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// `raptor <args>` from the developer's own terminal: a pty through `script`.
fn developer_command(args: &[&str]) -> Command {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/script");
        c.args(["-q", "/dev/null", RAPTOR]).args(args);
        c
    } else {
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let line = std::iter::once(RAPTOR)
            .chain(args.iter().copied())
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ");
        let mut c = Command::new("script");
        c.args(["-q", "-e", "-c", &line, "/dev/null"]);
        c
    }
}

/// "demo" with `main`, `feat-x` and the team configuration `settings` committed on `main` (the
/// floor), a bare remote with both, `prepare` applied (plain Git, no hooks yet) and the repo
/// protected by `raptor guard install`.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    remote: PathBuf,
}

impl Machine {
    fn new(settings: &str) -> Self {
        Self::build(settings, |_| {})
    }

    fn build(settings: &str, prepare: impl FnOnce(&Fixture)) -> Self {
        let f = Fixture::with_commit(&git_from_path());
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("notes.txt", "notes\n");
        f.write(".gitraptor/settings.json", settings);
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "seed and team settings"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.git(&["push", "-q", "origin", "feat-x"]);
        f.git(&["switch", "-q", "main"]);
        prepare(&f);
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self { f, outside, remote };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["guard", "install", "--yes", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn developer(&self, args: &[&str]) -> Output {
        developer_command(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn git_path(&self) -> String {
        self.f.git.to_str().unwrap().to_owned()
    }

    fn git_line(&self, args: &str) -> String {
        format!("'{}' {args}", self.git_path())
    }

    /// `git <args>` by the developer: no agent in the ancestry.
    fn human(&self, args: &str) -> Output {
        self.human_sh(&self.git_line(args))
    }

    fn human_sh(&self, line: &str) -> Output {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(line)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.repo)
            .output()
            .unwrap()
    }

    /// `git <args>` run by the simulated Claude Code.
    fn agent(&self, args: &str) -> Output {
        self.agent_sh(&self.git_line(args))
    }

    /// A shell line run by the simulated Claude Code.
    fn agent_sh(&self, line: &str) -> Output {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            copy_executable(&std::env::current_exe().unwrap(), &agent);
        }
        Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(AGENT_CMD, line)
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `git <args>` by the agent or by the person.
    fn by(&self, agent: bool, args: &str) -> Output {
        if agent {
            self.agent(args)
        } else {
            self.human(args)
        }
    }

    /// A shell line by the agent or by the person.
    fn sh_by(&self, agent: bool, line: &str) -> Output {
        if agent {
            self.agent_sh(line)
        } else {
            self.human_sh(line)
        }
    }

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    fn denied(&self, out: &Output) {
        assert!(!out.status.success(), "{}", text(out));
    }

    fn rev(&self, what: &str) -> String {
        let out = self.human(&format!("rev-parse {what}"));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn remote_rev(&self, name: &str) -> Option<String> {
        let out = Command::new(self.git_path())
            .arg("-C")
            .arg(&self.remote)
            .args(["rev-parse", "--verify", "-q", name])
            .env_clear()
            .envs(self.env())
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// The message file of a commit, with the `agents-commit` trailer.
    fn message(&self) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let msg = self.outside.path().join(format!("msg{n}"));
        std::fs::write(&msg, format!("feat: change {n}\n\n{CLAUDE}\n")).unwrap();
        msg
    }

    /// Stages `files`, then runs the `git` command `command` with `-F <message with the
    /// trailer>` by the agent or by the person.
    fn commit(&self, agent: bool, files: &[(&str, &str)], command: &str) -> Output {
        for (path, content) in files {
            self.f.write(path, content);
            self.ok(self.human(&format!("add -- '{path}'")));
        }
        let args = format!("{command} -F '{}'", self.message().to_str().unwrap());
        self.by(agent, &args)
    }

    /// The same commit, by plumbing: `commit-tree` + `update-ref` of `branch`.
    fn plumbing_commit(&self, agent: bool, branch: &str) -> Output {
        let g = self.git_path();
        let msg = self.message();
        let line = format!(
            "G='{g}'; T=$($G write-tree) && C=$($G commit-tree $T -p {branch} -F '{}') && \
             $G update-ref refs/heads/{branch} $C",
            msg.to_str().unwrap()
        );
        self.sh_by(agent, &line)
    }

    /// `raptor guard log --json` of "demo".
    fn log(&self) -> Value {
        let out = self.developer(&["guard", "log", "--json", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The pty of `script` may echo control characters before the output.
        let line = stdout
            .lines()
            .find_map(|l| l.find('{').map(|i| &l[i..]))
            .unwrap_or_else(|| panic!("no JSON: {stdout}"));
        serde_json::from_str(line.trim()).unwrap()
    }

    /// `(rule, level, cause)` of every reason of every entry of the log.
    fn logged_reasons(&self) -> Vec<(String, String, Option<String>)> {
        let log = self.log();
        log["entries"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|e| e["reasons"].as_array().unwrap().iter())
            .map(|r| {
                (
                    r["rule"].as_str().unwrap().to_owned(),
                    r["level"].as_str().unwrap().to_owned(),
                    r["cause"].as_str().map(str::to_owned),
                )
            })
            .collect()
    }

    /// Leaves an interrupted rebase or merge as the person would, to try the next command.
    fn settle(&self) {
        let git = self.f.repo.join(".git");
        for state in ["rebase-merge", "rebase-apply"] {
            let _ = std::fs::remove_dir_all(git.join(state));
        }
        for file in ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE"] {
            let _ = std::fs::remove_file(git.join(file));
        }
        let _ = self.human("switch -q -f feat-x");
        let _ = self.human("reset -q --hard");
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "daemon did not stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// A commit on `branch` with plain Git (before the install: no hooks).
fn seed_commit(f: &Fixture, branch: &str, file: &str) {
    f.git(&["switch", "-q", branch]);
    f.write(file, "seed\n");
    f.git(&["add", file]);
    f.git(&["commit", "-q", "-m", &format!("seed {file}")]);
    f.git(&["switch", "-q", "main"]);
}

/// A branch `side`, pushed and then rewritten, so pushing it needs a force.
fn diverged_side(f: &Fixture) {
    f.git(&["switch", "-q", "-c", "side"]);
    f.git(&["push", "-q", "origin", "side"]);
    f.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "--amend",
        "-m",
        "side rewritten",
    ]);
    f.git(&["switch", "-q", "main"]);
}

const DENY_COMMIT: &str = r#"{"permissions":{"deny":["commit"]}}"#;

/// US-GRD-007 escenario 1 (commit): neither `git commit`, nor `--no-verify`, nor `commit-tree` +
/// `update-ref` moves the branch, for the agent and for the person; the reason names the
/// permission and the team configuration.
#[test]
fn deny_commit_blocks_commit_and_its_second_line() {
    let m = Machine::new(DENY_COMMIT);
    m.ok(m.human("switch -q feat-x"));
    let tip = m.rev("feat-x");
    for agent in [true, false] {
        let out = m.commit(agent, &[("src.txt", "x\n")], "commit -q");
        m.denied(&out);
        let shown = text(&out);
        assert!(shown.contains(TEAM), "agent={agent}: {shown}");
        assert!(shown.contains("commit"), "agent={agent}: {shown}");
        assert_eq!(m.rev("feat-x"), tip);
        // The change is still staged and in the working tree.
        assert!(m.f.repo.join("src.txt").is_file());
        m.ok(m.human("reset -q --hard"));
        m.ok(m.human("clean -fdq"));

        // `--no-verify` skips the hooks of the commit, not the update of the branch.
        let out = m.commit(agent, &[("src.txt", "y\n")], "commit -q --no-verify");
        m.denied(&out);
        assert!(text(&out).contains(TEAM), "agent={agent}: {}", text(&out));
        assert_eq!(m.rev("feat-x"), tip);
        m.ok(m.human("reset -q --hard"));
        m.ok(m.human("clean -fdq"));

        // Plumbing.
        m.f.write("src.txt", "z\n");
        m.ok(m.human("add src.txt"));
        let out = m.plumbing_commit(agent, "feat-x");
        m.denied(&out);
        assert_eq!(m.rev("feat-x"), tip);
        m.ok(m.human("reset -q --hard"));
        m.ok(m.human("clean -fdq"));
    }
}

/// US-GRD-007 escenario 1 (push): the push does not reach the remote, for both.
#[test]
fn deny_push_blocks_push() {
    let m = Machine::build(r#"{"permissions":{"deny":["push"]}}"#, |f| {
        seed_commit(f, "feat-x", "ahead.txt")
    });
    let remote_tip = m.remote_rev("refs/heads/feat-x").unwrap();
    assert_ne!(m.rev("feat-x"), remote_tip);
    for agent in [true, false] {
        let out = m.by(agent, "push -q origin feat-x");
        m.denied(&out);
        assert!(text(&out).contains(TEAM), "agent={agent}: {}", text(&out));
        assert_eq!(m.remote_rev("refs/heads/feat-x").unwrap(), remote_tip);
        let out = m.by(agent, "push -q origin feat-x:refs/heads/feat-new");
        m.denied(&out);
        assert!(m.remote_rev("refs/heads/feat-new").is_none());
    }
}

/// US-GRD-007 escenario 1 (force-push): the forced push does not run and the reason names the
/// permission and the level. The safe minimum also denies a force-push, so the permission shows
/// in `raptor guard log` with its level.
#[test]
fn deny_force_push_names_the_permission_and_the_team() {
    let m = Machine::build(r#"{"permissions":{"deny":["force-push"]}}"#, diverged_side);
    let remote_tip = m.remote_rev("refs/heads/side").unwrap();
    m.ok(m.human("switch -q side"));
    for agent in [true, false] {
        let out = m.by(agent, "push -q --force origin side");
        m.denied(&out);
        assert!(text(&out).contains(TEAM), "agent={agent}: {}", text(&out));
        assert_eq!(m.remote_rev("refs/heads/side").unwrap(), remote_tip);
    }
    let reasons = m.logged_reasons();
    assert!(
        reasons
            .iter()
            .any(|(rule, level, _)| rule == "permission.force-push" && level == "floor"),
        "{reasons:?}"
    );
    // While the minimum is active, it is named too.
    assert!(
        reasons
            .iter()
            .any(|(rule, ..)| rule == "minimum.force-push"),
        "{reasons:?}"
    );
}

/// US-GRD-007 escenario 1 (borrar rama): `branch -D`, `update-ref -d`, `branch -m` and the remote
/// deletion do not run, for both.
#[test]
fn deny_branch_delete_blocks_local_and_remote() {
    let m = Machine::build(r#"{"permissions":{"deny":["branch-delete"]}}"#, |f| {
        f.git(&["branch", "tmp"]);
    });
    let tmp = m.rev("tmp");
    let feat = m.remote_rev("refs/heads/feat-x").unwrap();
    for agent in [true, false] {
        let out = m.by(agent, "branch -D tmp");
        m.denied(&out);
        assert!(text(&out).contains(TEAM), "agent={agent}: {}", text(&out));
        assert_eq!(m.rev("tmp"), tmp);

        m.denied(&m.by(agent, "update-ref -d refs/heads/tmp"));
        assert_eq!(m.rev("tmp"), tmp);

        m.denied(&m.by(agent, "branch -m tmp tmp-renamed"));
        assert_eq!(m.rev("tmp"), tmp);
        assert!(
            m.human("rev-parse --verify -q tmp-renamed")
                .stdout
                .is_empty(),
            "the renamed branch must not exist"
        );

        m.denied(&m.by(agent, "push -q origin :feat-x"));
        assert_eq!(m.remote_rev("refs/heads/feat-x").unwrap(), feat);
    }
}

/// US-GRD-007 escenario 1 (rebase): `rebase`, `rebase --no-verify` and `pull --rebase` do not
/// move the branch, for both.
#[test]
fn deny_rebase_blocks_rebase_and_pull_rebase() {
    let m = Machine::build(r#"{"permissions":{"deny":["rebase"]}}"#, |f| {
        // `main` moves ahead (also on the remote) while `feat-x` has a commit of its own.
        seed_commit(f, "feat-x", "feat.txt");
        seed_commit(f, "main", "main.txt");
        f.git(&["push", "-q", "origin", "main"]);
    });
    m.ok(m.human("switch -q feat-x"));
    let tip = m.rev("feat-x");
    for agent in [true, false] {
        let out = m.by(agent, "rebase main");
        m.denied(&out);
        assert!(text(&out).contains(TEAM), "agent={agent}: {}", text(&out));
        m.settle();
        assert_eq!(m.rev("feat-x"), tip);

        // The pre-rebase hook is skipped; the update of the branch is not.
        m.denied(&m.by(agent, "rebase --no-verify main"));
        m.settle();
        assert_eq!(m.rev("feat-x"), tip);

        m.denied(&m.by(agent, "pull --rebase origin main"));
        m.settle();
        assert_eq!(m.rev("feat-x"), tip);
    }
}

/// US-GRD-007 escenario 1 (crear worktree): `worktree add -b` creates nothing; without a new
/// branch it goes through (declared, D9).
#[test]
fn deny_worktree_add_blocks_a_worktree_with_a_new_branch() {
    let m = Machine::new(r#"{"permissions":{"deny":["worktree-add"]}}"#);
    for (n, agent) in [true, false].into_iter().enumerate() {
        let path = m.outside.path().join(format!("wt-new-{n}"));
        let branch = format!("wt-branch-{n}");
        let out = m.by(
            agent,
            &format!("worktree add -b {branch} '{}'", path.to_str().unwrap()),
        );
        m.denied(&out);
        assert!(text(&out).contains(TEAM), "agent={agent}: {}", text(&out));
        assert!(!path.exists(), "no worktree was created");
        assert!(
            m.human(&format!("rev-parse --verify -q {branch}"))
                .stdout
                .is_empty()
        );

        // Without a new branch the permission is not evaluated (declared).
        let path = m.outside.path().join(format!("wt-detached-{n}"));
        m.ok(m.by(
            agent,
            &format!("worktree add --detach '{}'", path.to_str().unwrap()),
        ));
        assert!(path.exists());
    }
}

/// US-GRD-007 D8: with `merge` denied the branch does not move, the reason says how to go back,
/// and `git merge --abort` / `git reset --merge` leave the working tree recoverable.
#[test]
fn merge_deny_keeps_the_branch_and_leaves_a_recoverable_tree() {
    let m = Machine::build(r#"{"permissions":{"deny":["merge"]}}"#, |f| {
        seed_commit(f, "feat-x", "feat.txt");
        seed_commit(f, "main", "main.txt");
    });
    let main = m.rev("main");
    for (agent, abort) in [(true, "merge --abort"), (false, "reset --merge")] {
        // An unrelated local change that recovering must not touch.
        m.f.write("notes.txt", "my local edit\n");
        let msg = m.message();
        let out = m.by(
            agent,
            &format!("merge --no-ff -F '{}' feat-x", msg.to_str().unwrap()),
        );
        m.denied(&out);
        assert!(
            text(&out).contains("merge --abort"),
            "the reason says how to go back: {}",
            text(&out)
        );
        assert_eq!(m.rev("main"), main, "the branch does not move");

        m.ok(m.human(abort));
        assert_eq!(m.rev("main"), main);
        assert_eq!(
            std::fs::read_to_string(m.f.repo.join("notes.txt")).unwrap(),
            "my local edit\n",
            "the unrelated change survives"
        );
        assert!(!m.f.repo.join("feat.txt").exists(), "the merge is undone");
        let status = String::from_utf8_lossy(&m.human("status --porcelain").stdout).into_owned();
        assert_eq!(status.trim(), "M notes.txt", "{status}");
        m.ok(m.human("checkout -q -- notes.txt"));
    }
}

/// US-GRD-007 D9: with `commit` denied and `merge` allowed, a `--no-verify` commit on a detached
/// `HEAD` does not reach a branch by `merge --ff-only`, `branch -f`, `reset --soft` or `switch -C`.
#[test]
fn a_detached_commit_cannot_reach_a_branch_unevaluated() {
    let m = Machine::new(DENY_COMMIT);
    for agent in [true, false] {
        m.ok(m.human("switch -q feat-x"));
        let tip = m.rev("feat-x");
        m.ok(m.human("switch -q --detach"));
        // The detached commit moves no branch: it is not evaluated when it is made.
        m.ok(m.commit(agent, &[], "commit -q --no-verify --allow-empty"));
        let detached = m.rev("HEAD");
        assert_ne!(detached, tip);

        let attempts = [
            ("feat-x", format!("merge --ff-only {detached}")),
            ("main", format!("branch -f feat-x {detached}")),
            ("feat-x", format!("reset --soft {detached}")),
            ("main", format!("switch -q -C feat-x {detached}")),
        ];
        for (on, command) in attempts {
            m.ok(m.human(&format!("switch -q {on}")));
            let out = m.by(agent, &command);
            m.denied(&out);
            assert_eq!(m.rev("feat-x"), tip, "`git {command}` moved feat-x");
            m.settle();
        }
        let reasons = m.logged_reasons();
        assert!(
            reasons.iter().any(|(rule, ..)| rule == "permission.commit"),
            "{reasons:?}"
        );
    }
}

/// US-GRD-007 D9: with `commit` denied, a fast-forward to commits that already exist and a pull
/// of commits from the remote go through, while a new commit is denied.
#[test]
fn a_fast_forward_of_existing_commits_is_not_a_commit() {
    let m = Machine::build(DENY_COMMIT, |f| {
        // `ahead` is a local branch with a commit `main` lacks.
        f.git(&["switch", "-q", "-c", "ahead"]);
        f.write("ahead.txt", "ahead\n");
        f.git(&["add", "ahead.txt"]);
        f.git(&["commit", "-q", "-m", "ahead"]);
        f.git(&["switch", "-q", "main"]);
        // The remote's `main` has a commit pushed from another clone.
        let other = f.root.join("other");
        f.git_in(
            &f.root,
            &[
                "clone",
                "-q",
                f.root.join("remote.git").to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        std::fs::write(other.join("remote.txt"), "remote\n").unwrap();
        f.git_in(&other, &["add", "remote.txt"]);
        f.git_in(&other, &["commit", "-q", "-m", "from the remote"]);
        f.git_in(&other, &["push", "-q", "origin", "main"]);
    });
    let start = m.rev("main");
    let ahead = m.rev("ahead");
    for agent in [true, false] {
        // A fast-forward to a local branch that already exists.
        m.ok(m.by(agent, "merge -q --ff-only ahead"));
        assert_eq!(m.rev("main"), ahead);
        m.ok(m.human(&format!("reset -q --hard {start}")));

        // A pull of commits that come from the remote.
        m.ok(m.by(agent, "pull -q --ff-only origin main"));
        assert_ne!(m.rev("main"), start);
        m.ok(m.human(&format!("reset -q --hard {start}")));
    }
    // A new commit is still denied.
    for agent in [true, false] {
        let out = m.commit(agent, &[("new.txt", "n\n")], "commit -q");
        m.denied(&out);
        assert_eq!(m.rev("main"), start);
        m.ok(m.human("reset -q --hard"));
        m.ok(m.human("clean -fdq"));
    }
}

/// US-GRD-007 escenario 2: `push` allowed lets `feat-x` through, while another restricted
/// operation of the same configuration (`commit`) is denied.
#[test]
fn allow_lets_push_through() {
    let m = Machine::build(
        r#"{"permissions":{"allow":["push"],"deny":["commit"]}}"#,
        |f| {
            seed_commit(f, "feat-x", "ahead.txt");
            f.git(&["branch", "feat-y", "feat-x"]);
        },
    );
    m.ok(m.agent("push -q origin feat-x"));
    assert_eq!(
        m.remote_rev("refs/heads/feat-x").unwrap(),
        m.rev("feat-x"),
        "the agent's push ran"
    );
    m.ok(m.human("push -q origin feat-y"));
    assert_eq!(m.remote_rev("refs/heads/feat-y").unwrap(), m.rev("feat-y"));
    // The restricted operation next to it is denied.
    m.ok(m.human("switch -q feat-x"));
    let out = m.commit(true, &[("src.txt", "x\n")], "commit -q");
    m.denied(&out);
    assert!(text(&out).contains(TEAM), "{}", text(&out));
}

const ASK_DELETE: &str = r#"{"permissions":{"ask":["branch-delete"]}}"#;

/// US-GRD-007 escenario 3 (Q-GRD-46): `branch-delete` in `ask`, an agent does not delete the
/// branch and the reason says it needs human confirmation.
#[test]
fn ask_is_denied_as_requiring_human_confirmation() {
    let m = Machine::build(ASK_DELETE, |f| {
        f.git(&["branch", "tmp"]);
    });
    let tmp = m.rev("tmp");
    let out = m.agent("branch -D tmp");
    m.denied(&out);
    assert!(text(&out).contains(NEEDS_HUMAN), "{}", text(&out));
    assert_eq!(m.rev("tmp"), tmp);
}

/// US-GRD-007 D19: the person deletes the branch with the notice, `guard log` does not count it as
/// a denial, and the agent is denied.
#[test]
fn ask_lets_the_developer_through() {
    let m = Machine::build(ASK_DELETE, |f| {
        f.git(&["branch", "tmp"]);
        f.git(&["branch", "tmp-agent"]);
    });
    // The agent is denied (the control: this is a repo where `ask` is evaluated at all).
    let out = m.agent("branch -D tmp-agent");
    m.denied(&out);
    assert!(text(&out).contains(NEEDS_HUMAN), "{}", text(&out));
    assert!(!m.human("rev-parse --verify -q tmp-agent").stdout.is_empty());

    // The person passes, with the notice of D19 (d).
    let out = m.ok(m.human("branch -D tmp"));
    assert!(
        text(&out).contains("confirmation"),
        "the person is told the ask did not apply: {}",
        text(&out)
    );
    assert!(m.human("rev-parse --verify -q tmp").stdout.is_empty());

    // Only the agent's denial is in the log and in the KPI.
    let log = m.log();
    let denials: Vec<&Value> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "denial")
        .collect();
    assert_eq!(denials.len(), 1, "{log:#}");
    assert_eq!(log["summary"]["blocked"], 1, "{log:#}");
}

/// US-GRD-007 D19: without the daemon (degraded mode), `ask` denies the person too, with
/// `ask-unavailable`. (A daemon without `guard.permissions` is covered in `crates/core`.)
#[test]
fn ask_without_the_daemon_denies_everyone() {
    // `commit` and not `branch-delete`: without the daemon a branch deletion is already refused
    // for another reason (no recovery point can be saved), which would hide the one under test.
    let m = Machine::new(r#"{"permissions":{"ask":["commit"]}}"#);
    m.ok(m.human("switch -q feat-x"));
    m.stop();
    let tip = m.rev("feat-x");
    for agent in [false, true] {
        let out = m.commit(agent, &[("src.txt", "x\n")], "commit -q");
        m.denied(&out);
        assert!(
            text(&out).contains(NEEDS_HUMAN),
            "agent={agent}: {}",
            text(&out)
        );
        assert_eq!(m.rev("feat-x"), tip);
        m.ok(m.human("reset -q --hard"));
        m.ok(m.human("clean -fdq"));
    }
}

/// US-GRD-007 escenario 6: hardening `push` in `feat-x` without committing changes nothing; once
/// committed, the push from `feat-x` is denied without reinstalling.
#[test]
fn a_hardening_applies_when_committed_not_when_edited() {
    let m = Machine::new(r#"{"permissions":{"allow":["push"]}}"#);
    m.ok(m.human("switch -q feat-x"));
    m.ok(m.commit(false, &[("one.txt", "1\n")], "commit -q"));

    // Edited, not committed: the push still goes.
    m.f.write(
        ".gitraptor/settings.json",
        r#"{"permissions":{"deny":["push"]}}"#,
    );
    m.ok(m.human("push -q origin feat-x"));
    assert_eq!(m.remote_rev("refs/heads/feat-x").unwrap(), m.rev("feat-x"));

    // Committed: the next push is denied, with no new install.
    m.ok(m.commit(false, &[], "commit -q -a"));
    let committed = m.rev("feat-x");
    let out = m.human("push -q origin feat-x");
    m.denied(&out);
    assert!(
        text(&out).contains("on this branch"),
        "the reason names the branch level: {}",
        text(&out)
    );
    assert_ne!(m.remote_rev("refs/heads/feat-x").unwrap(), committed);
}

/// US-GRD-007 D17: every denial of a permission (also the agent's `ask`) is in `raptor guard log`
/// with its rule, level and cause, and counts in the KPI.
#[test]
fn permission_denials_reach_the_decision_log() {
    let m = Machine::build(
        r#"{"permissions":{"deny":["commit","push"],"ask":["branch-delete"]}}"#,
        |f| {
            f.git(&["branch", "tmp"]);
        },
    );
    m.ok(m.human("switch -q feat-x"));
    m.denied(&m.commit(true, &[("src.txt", "x\n")], "commit -q"));
    m.ok(m.human("reset -q --hard"));
    m.denied(&m.agent("branch -D tmp"));
    m.denied(&m.human("push -q origin feat-x:refs/heads/feat-new"));

    let reasons = m.logged_reasons();
    for rule in ["permission.commit", "permission.push"] {
        assert!(
            reasons
                .iter()
                .any(|(r, level, cause)| r == rule && level == "floor" && cause.is_none()),
            "{rule}: {reasons:?}"
        );
    }
    assert!(
        reasons
            .iter()
            .any(|(r, level, cause)| r == "permission.branch-delete"
                && level == "floor"
                && cause.as_deref() == Some("ask-unavailable")),
        "{reasons:?}"
    );
    let log = m.log();
    assert!(log["summary"]["blocked"].as_u64().unwrap() >= 3, "{log:#}");
}

/// US-GRD-007 D12: without the daemon the team's `deny` of the floor and of the worktree applies;
/// its `allow` and `disableSafeMinimum` do not; the minimum holds.
#[test]
fn degraded_mode_applies_the_team_permissions_only_to_harden() {
    let floor =
        r#"{"permissions":{"deny":["commit"],"allow":["force-push"],"disableSafeMinimum":true}}"#;
    let m = Machine::build(floor, |f| {
        // The worktree level: `feat-x` carries a `deny` of `push` of its own, and has a commit to
        // push.
        f.git(&["switch", "-q", "feat-x"]);
        f.write(
            ".gitraptor/settings.json",
            r#"{"permissions":{"deny":["push","commit"],"allow":["force-push"]}}"#,
        );
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "worktree hardening"]);
        f.git(&["switch", "-q", "main"]);
        diverged_side(f);
    });
    m.stop();

    // The floor's deny of `commit` applies, for the person.
    let out = m.commit(false, &[("src.txt", "x\n")], "commit -q");
    m.denied(&out);
    assert!(text(&out).contains(TEAM), "{}", text(&out));
    m.ok(m.human("reset -q --hard"));
    m.ok(m.human("clean -fdq"));

    // The worktree's deny of `push` applies from `feat-x`.
    m.ok(m.human("switch -q feat-x"));
    let remote_feat = m.remote_rev("refs/heads/feat-x").unwrap();
    let out = m.human("push -q origin feat-x");
    m.denied(&out);
    assert_eq!(m.remote_rev("refs/heads/feat-x").unwrap(), remote_feat);

    // The floor's `allow` of force-push and its `disableSafeMinimum` are not read: the minimum holds.
    m.ok(m.human("switch -q side"));
    let remote_side = m.remote_rev("refs/heads/side").unwrap();
    for agent in [false, true] {
        let out = m.by(agent, "push -q --force origin side");
        m.denied(&out);
        assert_eq!(m.remote_rev("refs/heads/side").unwrap(), remote_side);
    }
}
