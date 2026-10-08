//! US-GRD-008 end to end (DS-US-GRD-008 § 6): protected branches and forbidden paths with the
//! real `raptor` as daemon and hook, the real `raptor-hook` dispatcher and plain Git, over
//! temporary repos, a bare remote and a temporary profile (NFR-01). The agent is
//! `raptor-fake-agent`, a copy of this test binary the debug daemon knows as Claude Code: `git`
//! runs under it (agent → sh → git → hook), so the daemon resolves the actor from the ancestry,
//! never from a claim. No fixed waits.
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

use std::ffi::OsString;
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
/// The trailer `agents-commit` (the default) asks of an agent's commit: every commit here carries
/// it, so only the policies under test can deny.
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";

const PROTECT_MAIN: &str = r#"{"policies":{"protectedBranches":{"patterns":["main","release/*"]}}}"#;
const PROTECT_MAIN_EVERYONE: &str =
    r#"{"policies":{"protectedBranches":{"patterns":["main"],"appliesTo":"everyone"}}}"#;
const FORBID_SECRETS: &str = r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#;
const BOTH: &str = r#"{"policies":{
    "protectedBranches":{"patterns":["main"]},
    "forbiddenPaths":{"patterns":["secrets/"]}}}"#;

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

/// "demo" with `main`, `secrets/a.txt` and `secrets/old.txt`, a bare remote with `main` and
/// `feat-x`, the team configuration `settings` committed on `main` (the floor), and the repo
/// protected by `raptor guard install`.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    remote: std::path::PathBuf,
}

impl Machine {
    fn new(settings: &str) -> Self {
        #[cfg(not(debug_assertions))]
        panic!(
            "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
        );
        let f = Fixture::with_commit(&git_from_path());
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        f.write("secrets/a.txt", "a\n");
        f.write("secrets/old.txt", "old\n");
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

    /// `git <args>` by the developer: no agent in the ancestry ("unattributed").
    fn human(&self, args: &str) -> Output {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(self.git_line(args))
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

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
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

    /// Stages `files` (a path with `None` content is removed), then runs the `git commit`
    /// `command` under the agent (or the person) with `-F <message with the trailer>`.
    fn commit(&self, agent: bool, files: &[(&str, Option<&str>)], command: &str) -> Output {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (path, content) in files {
            match content {
                Some(content) => self.f.write(path, content),
                None => {
                    self.ok(self.human(&format!("rm -q -- '{path}'")));
                    continue;
                }
            }
            self.ok(self.human(&format!("add -- '{path}'")));
        }
        let msg = self.outside.path().join(format!("msg{n}"));
        std::fs::write(&msg, format!("feat: change {n}\n\n{CLAUDE}\n")).unwrap();
        let args = format!("{command} -F '{}'", msg.to_str().unwrap());
        if agent {
            self.agent(&args)
        } else {
            self.human(&args)
        }
    }

    /// The branches, the index and the working tree: what a denial must leave as it was.
    fn state(&self) -> (String, String, String) {
        let text = |args: &str| String::from_utf8_lossy(&self.human(args).stdout).into_owned();
        (
            text("for-each-ref refs/heads"),
            text("diff --cached"),
            text("status --porcelain"),
        )
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

    fn profile_settings(&self, json: &str) {
        std::fs::write(self.dirs().config.join("settings.json"), json).unwrap();
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

/// US-GRD-008 escenario 1: an agent cannot commit, push or delete the protected branch, with
/// Git directly (also with `--no-verify`, by the second line), the reason names the branch, and
/// nothing is lost.
#[test]
fn protected_branch_blocks_an_agent() {
    let m = Machine::new(PROTECT_MAIN);
    let main = m.rev("main");

    // commit
    let out = m.commit(true, &[("src.txt", Some("x\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("protected"), "{shown}");
    assert!(shown.contains("«main»"), "{shown}");
    assert_eq!(m.rev("main"), main);
    // The change is still staged and in the working tree.
    assert!(m.state().1.contains("+++ b/src.txt"), "{:?}", m.state());
    assert!(m.f.repo.join("src.txt").is_file());

    // The same with `--no-verify`: the hooks of the commit are skipped, the ref update is not.
    let out = m.commit(true, &[("src2.txt", Some("y\n"))], "commit -q --no-verify");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«main»"), "{}", text(&out));
    assert_eq!(m.rev("main"), main);

    // push (the commit is the person's: the agent only pushes)
    m.ok(m.human("reset -q"));
    m.ok(m.human("switch -q -c side"));
    let out = m.agent("push -q origin side:main");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«main»"), "{}", text(&out));
    let remote_main = m.remote_rev("refs/heads/main");
    assert_eq!(remote_main.as_deref(), Some(main.as_str()));

    // delete: locally and on the remote
    let out = m.agent("branch -D main");
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(m.rev("main"), main);
    let out = m.agent("push -q origin :main");
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(
        m.remote_rev("refs/heads/main").as_deref(),
        Some(main.as_str())
    );

    // force-push (the permission of US-GRD-007 also covers it; the policy names the branch)
    let out = m.agent("push -q --force origin side:main");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«main»"), "{}", text(&out));

    // creating a branch that matches a protected pattern changes it too
    let out = m.agent("branch release/1.0");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«release/1.0»"), "{}", text(&out));
    assert!(m.human("rev-parse --verify -q release/1.0").stdout.is_empty());
    // …and the person creates it
    m.ok(m.human("branch release/1.0"));
}

/// US-GRD-008 escenario 2 and BR-VAL-003 (Q-GRD-1): the person is not an agent, and the other
/// branches are free for the agent: commit and push.
#[test]
fn the_person_and_other_branches_are_free() {
    let m = Machine::new(PROTECT_MAIN);

    let out = m.commit(false, &[("person.txt", Some("p\n"))], "commit -q");
    m.ok(out);
    m.ok(m.human("push -q origin main"));

    m.ok(m.human("switch -q feat-x"));
    m.ok(m.commit(true, &[("agent.txt", Some("a\n"))], "commit -q"));
    m.ok(m.agent("push -q origin feat-x"));
    assert_eq!(
        m.remote_rev("refs/heads/feat-x").as_deref(),
        Some(m.rev("feat-x").as_str())
    );
}

/// `appliesTo: everyone` is the policy saying otherwise: the person is held too.
#[test]
fn everyone_applies_to_the_person_too() {
    let m = Machine::new(PROTECT_MAIN_EVERYONE);
    let out = m.commit(false, &[("person.txt", Some("p\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«main»"), "{}", text(&out));
    // The other branches stay free.
    m.ok(m.human("reset -q"));
    m.ok(m.human("switch -q feat-x"));
    m.ok(m.commit(false, &[("person.txt", Some("p\n"))], "commit -q"));
}

/// US-GRD-008 escenarios 3 and 4: a commit that modifies, creates or deletes a forbidden path is
/// denied (also with `--no-verify` and with plumbing), the reason names the path, and the
/// changes stay in the working tree.
#[test]
fn forbidden_path_blocks_an_agent_commit() {
    let m = Machine::new(FORBID_SECRETS);
    m.ok(m.human("switch -q feat-x"));
    let tip = m.rev("feat-x");

    // modify
    let out = m.commit(true, &[("secrets/a.txt", Some("changed\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("forbidden"), "{}", text(&out));
    assert!(text(&out).contains("«secrets/a.txt»"), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
    assert_eq!(
        std::fs::read_to_string(m.f.repo.join("secrets/a.txt")).unwrap(),
        "changed\n"
    );
    assert!(m.state().1.contains("secrets/a.txt"), "{:?}", m.state());
    m.ok(m.human("reset -q --hard"));

    // create
    let out = m.commit(true, &[("secrets/new.txt", Some("n\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«secrets/new.txt»"), "{}", text(&out));
    m.ok(m.human("reset -q --hard"));
    m.ok(m.human("clean -fdq"));

    // delete
    let out = m.commit(true, &[("secrets/old.txt", None)], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«secrets/old.txt»"), "{}", text(&out));
    m.ok(m.human("reset -q --hard"));

    // `--no-verify`
    let out = m.commit(
        true,
        &[("secrets/a.txt", Some("again\n"))],
        "commit -q --no-verify",
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
    m.ok(m.human("reset -q --hard"));

    // plumbing: `commit-tree` and `update-ref`
    m.f.write("secrets/a.txt", "plumbing\n");
    let g = m.git_path();
    let line = format!(
        "G='{g}'; $G add secrets/a.txt && T=$($G write-tree) && \
         C=$($G commit-tree $T -p HEAD -m 'plumbing') && $G update-ref refs/heads/feat-x $C"
    );
    let out = m.agent_sh(&line);
    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
    m.ok(m.human("reset -q --hard"));

    // The person commits the same path.
    m.ok(m.commit(false, &[("secrets/a.txt", Some("person\n"))], "commit -q"));
    // The agent changes a path that is not forbidden.
    m.ok(m.commit(true, &[("src.txt", Some("fine\n"))], "commit -q"));
}

/// A forbidden path does not leave through `push` either, even when the commit was made on a
/// detached `HEAD` (no branch moved, so only the push can see it).
#[test]
fn forbidden_path_does_not_leave_by_push() {
    let m = Machine::new(FORBID_SECRETS);
    m.ok(m.human("switch -q --detach"));
    let out = m.commit(true, &[("secrets/a.txt", Some("leak\n"))], "commit -q");
    assert!(
        out.status.success(),
        "the detached commit moves no branch: {}",
        text(&out)
    );
    let out = m.agent("push -q origin HEAD:refs/heads/leak");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«secrets/a.txt»"), "{}", text(&out));
    assert!(m.remote_rev("refs/heads/leak").is_none());
    // The person pushes it (the policy governs agents).
    m.ok(m.human("push -q origin HEAD:refs/heads/leak"));
}

/// US-GRD-008 escenario 5 (BR-CALC-001): two broken rules are named together.
#[test]
fn two_broken_rules_are_named_together() {
    let m = Machine::new(BOTH);
    let out = m.commit(true, &[("secrets/a.txt", Some("both\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("«main»"), "{shown}");
    assert!(shown.contains("«secrets/a.txt»"), "{shown}");
}

/// US-GRD-005: each denial is in `raptor guard log` with its rule.
#[test]
fn denials_reach_the_decision_log() {
    let m = Machine::new(BOTH);
    let out = m.commit(true, &[("src.txt", Some("x\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    m.ok(m.human("reset -q"));
    m.ok(m.human("switch -q feat-x"));
    let out = m.commit(true, &[("secrets/a.txt", Some("x\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));

    let log = m.log();
    let rules: Vec<String> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["reasons"].as_array().unwrap().iter())
        .map(|r| r["rule"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        rules.iter().any(|r| r == "policy.protected-branch"),
        "{log:#}"
    );
    assert!(
        rules.iter().any(|r| r == "policy.forbidden-path"),
        "{log:#}"
    );
    // The log never keeps what was inside the file.
    let shown = log.to_string();
    assert!(!shown.contains("x\\n"), "{shown}");
}

/// BR-CONS-001: the levels only harden. A personal level adds its patterns to the team's; it
/// cannot take any away, and the safe minimum is untouched.
#[test]
fn levels_only_harden() {
    let m = Machine::new(PROTECT_MAIN);
    m.profile_settings(
        r#"{"policies":{"protectedBranches":{"patterns":[]},"forbiddenPaths":{"patterns":["src.txt"]}}}"#,
    );
    // The team's `main` still holds although the profile lists nothing.
    let out = m.commit(true, &[("other.txt", Some("x\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("team configuration"), "{}", text(&out));
    m.ok(m.human("reset -q"));
    // The profile adds a forbidden path of its own.
    m.ok(m.human("switch -q feat-x"));
    let out = m.commit(true, &[("src.txt", Some("x\n"))], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("«src.txt»"), "{}", text(&out));
    m.ok(m.human("reset -q"));
    // The minimum keeps holding: deleting the base is denied whatever the policies say.
    let out = m.agent("push -q origin :main");
    assert!(!out.status.success(), "{}", text(&out));
}

/// ADR-GRD-003 § 4: without the daemon the layer is degraded: the actor is "unattributed" and
/// the policies are not read, so neither rule applies; the minimum still does.
#[test]
fn degraded_mode_applies_no_policy_rule() {
    let m = Machine::new(BOTH);
    m.stop();
    m.ok(m.commit(true, &[("secrets/a.txt", Some("x\n"))], "commit -q"));
    let out = m.agent("branch -D main");
    assert!(
        !out.status.success(),
        "the safe minimum holds: {}",
        text(&out)
    );
}
