//! US-GRD-012 end to end: an agent cannot commit a change to the Guardrails configuration
//! (`.gitraptor/`), an edit that was not committed changes nothing, and a lax configuration
//! committed in the agent's worktree or written in the local level relaxes nothing while the
//! attempt reaches the decision log. The real `raptor` is the daemon and the hook client, the
//! real `raptor-hook` dispatcher and plain Git run over temporary repos, a bare remote and a
//! temporary profile (NFR-01). The agent is `raptor-fake-agent`, a copy of this test binary that
//! the debug daemon knows as Claude Code: `git` runs under it (agent -> sh -> git -> hook), so
//! the daemon resolves the actor from the ancestry, never from a claim. No fixed waits.
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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

/// A team configuration that sets nothing the tests touch.
const TEAM: &str = r#"{"policies":{"protectedBranches":{"patterns":["release/*"]}}}"#;
/// A team configuration change the person commits.
const TEAM_CHANGED: &str =
    r#"{"policies":{"protectedBranches":{"patterns":["release/*","hotfix/*"]}}}"#;
/// A configuration that tries to turn the safe minimum off and to relax `commitAuthorship`.
const LAX: &str = r#"{"permissions":{"disableSafeMinimum":true,"allow":["force-push"]},
    "policies":{"commitAuthorship":{"mode":"flexible"}}}"#;
/// A local level that tries to allow force-push.
const LOCAL_LAX: &str = r#"{"permissions":{"allow":["force-push"]}}"#;

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

fn write_in(dir: &Path, rela: &str, content: &str) {
    let path = dir.join(rela);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// "demo" with `main` and the team configuration `settings` committed on it (the floor), a bare
/// remote with `main` and `feat-x`, and the repo protected by `raptor guard install`.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
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
        f.write("src/main.rs", "fn main() {}\n");
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
        let m = Self { f, outside };
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

    /// A shell line run in `dir` by the developer ("unattributed") or by the simulated Claude
    /// Code.
    fn sh(&self, dir: &Path, agent: bool, line: &str) -> Output {
        if !agent {
            return Command::new("/bin/sh")
                .arg("-c")
                .arg(line)
                .env_clear()
                .envs(self.env())
                .current_dir(dir)
                .output()
                .unwrap();
        }
        let fake = self.outside.path().join("bin").join(FAKE_AGENT);
        if !fake.exists() {
            copy_executable(&std::env::current_exe().unwrap(), &fake);
        }
        Command::new(fake)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(AGENT_CMD, line)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `git <args>` by the developer in the main worktree.
    fn human(&self, args: &str) -> Output {
        self.sh(&self.f.repo, false, &self.git_line(args))
    }

    /// `git <args>` run by the simulated Claude Code in the main worktree.
    fn agent(&self, args: &str) -> Output {
        self.sh(&self.f.repo, true, &self.git_line(args))
    }

    /// A shell line run by the simulated Claude Code in the main worktree.
    fn agent_sh(&self, line: &str) -> Output {
        self.sh(&self.f.repo, true, line)
    }

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    fn rev(&self, what: &str) -> String {
        let out = self.human(&format!("rev-parse {what}"));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
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

    /// Leaves `feat-x` of the worktree `wt` with a tip that is not a descendant of the remote's:
    /// the person commits (the configuration `lax` first, when given), pushes, and amends the
    /// tip, so a push of the branch is a force-push. Only the tip is new for the remote, and it
    /// touches nothing of `.gitraptor/`.
    fn diverge(&self, wt: &Path, lax: Option<&str>) {
        let person = |args: &str| self.ok(self.sh(wt, false, &self.git_line(args)));
        if let Some(lax) = lax {
            write_in(wt, ".gitraptor/settings.json", lax);
            person("add -- .gitraptor/settings.json");
            person("commit -q -m 'person: lax team config'");
        }
        write_in(wt, "person.txt", "p\n");
        person("add -- person.txt");
        person("commit -q -m 'person: change'");
        person("push -q origin feat-x");
        person("commit -q --amend --no-edit");
    }

    /// A JSON line printed by `raptor <args>` ("demo"), through the pty of `script`.
    fn json(&self, args: &[&str]) -> Value {
        let out = self.developer(args);
        assert!(out.status.success(), "{}", text(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The pty of `script` may echo control characters before the output.
        let line = stdout
            .lines()
            .find_map(|l| l.find('{').map(|i| &l[i..]))
            .unwrap_or_else(|| panic!("no JSON: {stdout}"));
        serde_json::from_str(line.trim()).unwrap()
    }

    /// `raptor guard log --json` of "demo".
    fn log(&self) -> Value {
        self.json(&["guard", "log", "--json", self.f.repo.to_str().unwrap()])
    }

    /// The id the daemon keys the repo by (the folder of its local level).
    fn repo_id(&self) -> String {
        let status = self.json(&["guard", "status", "--json", self.f.repo.to_str().unwrap()]);
        status["repo_id"].as_str().unwrap().to_owned()
    }

    /// Where the local level of the repo lives (ADR-GRP-008).
    fn local_settings_path(&self) -> std::path::PathBuf {
        self.dirs()
            .config
            .join("repos")
            .join(self.repo_id())
            .join("settings.local.json")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// The reasons of the log entries, as `(kind, actor, rule, level)`.
fn log_reasons(log: &Value) -> Vec<(String, String, String, String)> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| {
            e["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        text(&e["kind"]),
                        text(&e["actor"]),
                        text(&r["rule"]),
                        text(&r["level"]),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn has_rule(log: &Value, rule: &str) -> bool {
    log_reasons(log).iter().any(|(_, _, r, _)| r == rule)
}

/// A `config.relax-ignored` notice of `level` for the agent `claude-code`.
fn has_relax_notice(log: &Value, level: &str) -> bool {
    log_reasons(log).iter().any(|(kind, actor, rule, l)| {
        kind == "notice" && actor == "claude-code" && rule == "config.relax-ignored" && l == level
    })
}

/// US-GRD-012 escenario 1 (BR-AUTH-004): the commit of an agent that changes the team
/// configuration does not run, the reason names the protection, the log keeps it with the
/// agent's actor, and nothing is lost.
#[test]
fn an_agent_commit_relaxing_the_team_config_is_denied_and_logged() {
    let m = Machine::new(TEAM);
    let main = m.rev("main");

    let out = m.commit(
        true,
        &[(".gitraptor/settings.json", Some(LAX))],
        "commit -q",
    );
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("Guardrails configuration"),
        "{}",
        text(&out)
    );
    assert_eq!(m.rev("main"), main);
    // The change is still staged and in the working tree.
    assert!(
        m.state().1.contains("+++ b/.gitraptor/settings.json"),
        "{:?}",
        m.state()
    );
    assert_eq!(
        std::fs::read_to_string(m.f.repo.join(".gitraptor/settings.json")).unwrap(),
        LAX
    );

    let log = m.log();
    assert!(
        log_reasons(&log)
            .iter()
            .any(|(kind, actor, rule, _)| kind == "denial"
                && actor == "claude-code"
                && rule == "policy.config-protected"),
        "{log:#}"
    );
}

/// US-GRD-012 escenario 2 (Q-GRD-17): an edit that was not committed changes nothing: the
/// agent's force-push is still denied by the safe minimum and nothing is logged about the edit.
#[test]
fn an_uncommitted_relaxation_changes_nothing() {
    let m = Machine::new(TEAM);
    m.ok(m.human("switch -q feat-x"));
    m.diverge(&m.f.repo, None);
    // The edit stays in the working tree: it is never staged nor committed.
    m.f.write(".gitraptor/settings.json", LAX);

    let out = m.agent("push -q --force origin feat-x");
    assert!(!out.status.success(), "{}", text(&out));
    let log = m.log();
    assert!(has_rule(&log, "minimum.force-push"), "{log:#}");
    assert!(!has_rule(&log, "config.relax-ignored"), "{log:#}");
}

/// US-GRD-012 escenario 3 (Q-GRD-20): a lax configuration committed (by the person) in the
/// worktree of the agent relaxes nothing: the force-push is denied, and the attempt is in the log
/// as a notice of the `worktree` level with the agent's actor.
#[test]
fn a_lax_config_committed_in_the_agent_worktree_relaxes_nothing() {
    let m = Machine::new(TEAM);
    let wt = m.f.add_worktree("x", "feat-x");
    m.diverge(&wt, Some(LAX));

    let out = m.sh(&wt, true, &m.git_line("push -q --force origin feat-x"));
    assert!(!out.status.success(), "{}", text(&out));
    let log = m.log();
    assert!(has_rule(&log, "minimum.force-push"), "{log:#}");
    assert!(has_relax_notice(&log, "worktree"), "{log:#}");
}

/// US-GRD-012 escenario 4 (BR-AUTH-004): deleting the team configuration is a change too.
#[test]
fn an_agent_cannot_delete_the_team_config() {
    let m = Machine::new(TEAM);
    let out = m.commit(true, &[(".gitraptor/settings.json", None)], "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    let listed = m.human("ls-tree HEAD .gitraptor/settings.json");
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains(".gitraptor/settings.json"),
        "the configuration is still on the branch: {}",
        text(&listed)
    );
}

/// US-GRD-012 escenario 5: what an agent commits outside the configuration goes ahead.
#[test]
fn an_agent_commit_outside_the_config_goes_ahead() {
    let m = Machine::new(TEAM);
    let out = m.commit(
        true,
        &[("src/main.rs", Some("fn main() { }\n"))],
        "commit -q",
    );
    m.ok(out);
}

/// US-GRD-012 escenario 6: the person commits a change to the team configuration; it runs and
/// stays on the branch. The "conscious confirmation" is the person's own commit.
#[test]
fn the_developer_commits_a_team_config_change() {
    let m = Machine::new(TEAM);
    m.ok(m.commit(
        false,
        &[(".gitraptor/settings.json", Some(TEAM_CHANGED))],
        "commit -q",
    ));
    let shown = m.human("show HEAD:.gitraptor/settings.json");
    assert_eq!(String::from_utf8_lossy(&shown.stdout), TEAM_CHANGED);
}

/// The agent writes the local level itself to allow force-push: the effective rule does not
/// change (the force-push is still denied) and the attempt is in the log as a notice of the
/// `local` level with the agent's actor.
#[test]
fn an_agent_relaxing_the_local_config_changes_nothing_and_is_logged() {
    let m = Machine::new(TEAM);
    m.ok(m.human("switch -q feat-x"));
    m.diverge(&m.f.repo, None);
    let local = m.local_settings_path();
    let dir = local.parent().unwrap().to_str().unwrap().to_owned();
    let line = format!(
        "mkdir -p '{dir}' && printf '%s' '{LOCAL_LAX}' > '{}'",
        local.to_str().unwrap()
    );
    m.ok(m.agent_sh(&line));

    let out = m.agent("push -q --force origin feat-x");
    assert!(!out.status.success(), "{}", text(&out));
    let log = m.log();
    assert!(has_rule(&log, "minimum.force-push"), "{log:#}");
    assert!(has_relax_notice(&log, "local"), "{log:#}");
}

/// A relaxation is not the person's because of whose identity signs the commit: the actor the
/// hook sees decides. In a worktree whose committed configuration is lax, the agent commits with
/// the person's identity and no trailer: the authorship rule is the team's (the trailer is still
/// required), nothing is relaxed, and the attempt is a notice with the agent's actor.
#[test]
fn an_agent_commit_with_the_person_identity_and_no_trailer_does_not_relax() {
    let m = Machine::new(TEAM);
    let wt = m.f.add_worktree("x", "feat-x");
    m.diverge(&wt, Some(LAX));

    // The agent commits with the person's identity (the Git configuration) and no trailer: the
    // worktree's `flexible` is not the team's, so the commit is denied as before.
    write_in(&wt, "agent.txt", "a\n");
    m.ok(m.sh(&wt, false, &m.git_line("add -- agent.txt")));
    let out = m.sh(&wt, true, &m.git_line("commit -q -m 'agent: change'"));
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("trailer"), "{}", text(&out));
    m.ok(m.sh(&wt, false, &m.git_line("reset -q")));

    let out = m.sh(&wt, true, &m.git_line("push -q --force origin feat-x"));
    assert!(!out.status.success(), "{}", text(&out));
    let log = m.log();
    assert!(has_rule(&log, "authorship.trailer-required"), "{log:#}");
    assert!(has_relax_notice(&log, "worktree"), "{log:#}");
}
