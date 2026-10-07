//! US-GRD-018 end to end (DS-US-GRD-018 § 7): the commit authorship policy with the real
//! `raptor` as daemon and hook, the real `raptor-hook` dispatcher and plain Git, over temporary
//! repos and a temporary profile (NFR-01). The agent is `raptor-fake-agent`, a copy of this test
//! binary the debug daemon knows as Claude Code: `git commit` runs under it (agent → sh → git →
//! hook), so the daemon resolves the actor from the ancestry, never from a claim. No fixed waits.
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
use gitraptor_testkit::fixture::git_from_path;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The command the simulated agent runs with `/bin/sh`.
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";

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

struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    lang: &'static str,
}

impl Machine {
    /// "demo" with `main`; `settings`, when given, is the team configuration committed on `main`
    /// before protecting it (the floor).
    fn new(settings: Option<&str>) -> Self {
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
        if let Some(settings) = settings {
            f.write(".gitraptor/settings.json", settings);
            f.git(&["add", ".gitraptor/settings.json"]);
            f.git(&["commit", "-q", "-m", "team settings"]);
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self {
            f,
            outside,
            lang: "en_US.UTF-8",
        };
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
            ("LANG", self.lang.into()),
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

    fn git_line(&self, args: &str) -> String {
        format!("'{}' {args}", self.f.git.to_str().unwrap())
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
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
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
            .env(AGENT_CMD, self.git_line(args))
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A commit of a new file, with `message` (written to a file: trailers keep their lines).
    fn commit_by(&self, agent: bool, message: &str) -> Output {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("f{n}.txt");
        std::fs::write(self.f.repo.join(&name), format!("{n}\n")).unwrap();
        let msg = self.outside.path().join(format!("msg{n}"));
        std::fs::write(&msg, message).unwrap();
        let add = self.human(&format!("add {name}"));
        assert!(add.status.success(), "{}", text(&add));
        let args = format!("commit -q -F '{}'", msg.to_str().unwrap());
        if agent {
            self.agent(&args)
        } else {
            self.human(&args)
        }
    }

    fn head_message(&self) -> String {
        let out = self.human("log -1 --format=%B");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn head_author(&self) -> String {
        let out = self.human("log -1 --format=%an<%ae>");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
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

const HUMAN_DENY: &str = r#"{"policies":{"commitAuthorship":{"mode":"human-author"}}}"#;
const HUMAN_WARN: &str =
    r#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#;
const FLEXIBLE: &str = r#"{"policies":{"commitAuthorship":{"mode":"flexible"}}}"#;

/// US-GRD-018 escenario 1 (sin política: `agents-commit`): with its trailer the agent's commit
/// goes in with the person's identity and the trailer intact; without it, it does not, and the
/// reason names the policy and the example.
#[test]
fn agents_commit_needs_the_agents_trailer() {
    let m = Machine::new(None);
    let author_before = m.head_author();

    let out = m.commit_by(true, "feat: by the agent\n");
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("\"agents-commit\""), "{shown}");
    assert!(
        shown.contains("«Co-Authored-By: Claude <noreply@anthropic.com>»"),
        "{shown}"
    );
    m.human("reset -q");

    let out = m.commit_by(true, &format!("feat: by the agent\n\n{CLAUDE}\n"));
    assert!(out.status.success(), "{}", text(&out));
    assert!(m.head_message().contains(CLAUDE));
    // GitRaptor never rewrites the author: it is the person's Git identity.
    assert_eq!(m.head_author(), author_before);

    // A trailer of an unknown co-author does not count.
    let out = m.commit_by(true, "feat: x\n\nCo-Authored-By: Ana <ana@x.com>\n");
    assert!(!out.status.success(), "{}", text(&out));
}

/// BR-EDGE-004 (adjustment 1 of the coordinator): a commit with no agent in the ancestry is
/// never blocked by the authorship policy, with or without a trailer, under the default.
#[test]
fn an_unattributed_commit_needs_no_trailer() {
    let m = Machine::new(None);
    let out = m.commit_by(false, "feat: by the person\n");
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("GitRaptor"), "{}", text(&out));
}

/// `human-author` with `deny` blocks the agent even with its trailer; with `warn` the commit
/// goes ahead with a warning on stderr (exit 0), in English and in Spanish. The person is
/// never blocked, and a Claude trailer on the person's commit does not make an actor.
#[test]
fn human_author_blocks_or_warns() {
    let m = Machine::new(Some(HUMAN_DENY));
    let out = m.commit_by(true, &format!("feat: x\n\n{CLAUDE}\n"));
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("\"human-author\" policy, team configuration"));
    m.human("reset -q");
    let out = m.commit_by(false, &format!("feat: x\n\n{CLAUDE}\n"));
    assert!(out.status.success(), "{}", text(&out));

    let mut m = Machine::new(Some(HUMAN_WARN));
    let out = m.commit_by(true, &format!("feat: x\n\n{CLAUDE}\n"));
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("warning: in this repo commits are made by the person"),
        "{}",
        text(&out)
    );
    // With `warn` the commit still meets `agents-commit`.
    let out = m.commit_by(true, "feat: no trailer\n");
    assert!(!out.status.success(), "{}", text(&out));
    m.human("reset -q");

    m.lang = "es_ES.UTF-8";
    let out = m.commit_by(true, &format!("feat: y\n\n{CLAUDE}\n"));
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("aviso: en este repo los commits los hace la persona"),
        "{}",
        text(&out)
    );
    let out = m.commit_by(false, "feat: z\n");
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("aviso"), "{}", text(&out));
}

/// A personal level never relaxes the team's `human-author` (Q-GRD-20); the reason names the
/// team configuration.
#[test]
fn a_personal_level_does_not_relax_the_team_policy() {
    let m = Machine::new(Some(HUMAN_DENY));
    m.profile_settings(FLEXIBLE);
    let out = m.commit_by(true, &format!("feat: x\n\n{CLAUDE}\n"));
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("team configuration"), "{}", text(&out));
}

/// D2: `flexible` outside a confirmed floor is ignored and `agents-commit` rules; a personal
/// level may harden.
#[test]
fn only_the_floor_relaxes_to_flexible() {
    // The profile asks for flexible: ignored.
    let m = Machine::new(None);
    m.profile_settings(FLEXIBLE);
    let out = m.commit_by(true, "feat: x\n");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("\"agents-commit\""));
    m.human("reset -q");
    // The profile hardens to human-author.
    m.profile_settings(HUMAN_DENY);
    let out = m.commit_by(true, &format!("feat: x\n\n{CLAUDE}\n"));
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("personal configuration"),
        "{}",
        text(&out)
    );

    // A floor with team settings at install stays unconfirmed (Q-GRD-23): its `flexible`
    // does not relax until the developer confirms it (US-GRD-014). The confirmed floor that
    // relaxes is covered in `crates/core/tests/guard_evaluate.rs`.
    let m = Machine::new(Some(FLEXIBLE));
    let out = m.commit_by(true, "feat: x\n");
    assert!(!out.status.success(), "{}", text(&out));
}

/// D3/ADR-GRD-003 § 4: in degraded mode the actor is unattributed, so no authorship rule
/// denies.
#[test]
fn degraded_mode_does_not_apply_authorship_rules() {
    let m = Machine::new(Some(HUMAN_DENY));
    m.stop();
    let out = m.commit_by(true, "feat: x\n");
    assert!(out.status.success(), "{}", text(&out));
}

/// D7: a message the client does not read (over 64 KiB) denies an agent with `message-unreadable`.
#[test]
fn an_unreadable_message_denies_the_agent() {
    let m = Machine::new(None);
    std::fs::write(m.f.repo.join("u.txt"), "u\n").unwrap();
    assert!(m.human("add u.txt").status.success());
    // Larger than 64 KiB: never read, even with the trailer at the end.
    let big = format!("feat: x\n\n{}\n\n{CLAUDE}\n", "a".repeat(70 * 1024));
    let msg = m.outside.path().join("big");
    std::fs::write(&msg, big).unwrap();
    let out = m.agent(&format!("commit -q -F '{}'", msg.to_str().unwrap()));
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("could not be read"), "{}", text(&out));
}

/// Adjustment 2 of the coordinator: a repo protected with template 1 (no commit dispatchers)
/// keeps working unchanged with the new engine: commits are not evaluated and the safe minimum
/// still holds.
#[test]
fn a_template_1_install_keeps_working() {
    let m = Machine::new(None);
    let folder = m.f.repo.join(".git/gitraptor");
    let conf = std::fs::read_to_string(folder.join("dispatch.conf")).unwrap();
    std::fs::write(
        folder.join("dispatch.conf"),
        conf.replace("template\t2\n", "template\t1\n"),
    )
    .unwrap();
    for hook in ["pre-commit", "commit-msg"] {
        std::fs::remove_file(folder.join("hooks").join(hook)).unwrap();
    }
    // No commit dispatcher: the agent's commit is not evaluated.
    let out = m.commit_by(true, "feat: x\n");
    assert!(out.status.success(), "{}", text(&out));
    // The minimum still applies through the template-1 dispatchers.
    assert!(m.human("switch -q -c side").status.success());
    let out = m.agent("branch -D main");
    assert!(!out.status.success(), "{}", text(&out));
}

/// D9: `--amend` of the person's commit by the agent needs the trailer; a merge commit by the
/// agent is evaluated; a fast-forward, a rebase and a cherry-pick are not.
#[test]
fn amend_merge_rebase_and_cherry_pick() {
    let m = Machine::new(None);
    let out = m.commit_by(false, "feat: by the person\n");
    assert!(out.status.success(), "{}", text(&out));
    let out = m.agent("commit -q --amend -m 'feat: amended'");
    assert!(!out.status.success(), "{}", text(&out));
    let msg = format!("feat: amended\n\n{CLAUDE}");
    let out = m.agent(&format!("commit -q --amend -m \"{msg}\""));
    assert!(out.status.success(), "{}", text(&out));

    // A side branch by the person, then a merge commit by the agent.
    assert!(m.human("switch -q -c side").status.success());
    let out = m.commit_by(false, "feat: side\n");
    assert!(out.status.success(), "{}", text(&out));
    assert!(m.human("switch -q main").status.success());
    let out = m.commit_by(false, "feat: main moves\n");
    assert!(out.status.success(), "{}", text(&out));
    let out = m.agent("merge -q --no-ff side -m 'merge side'");
    assert!(!out.status.success(), "{}", text(&out));
    let _ = m.human("merge --abort");

    // Cherry-pick and rebase by the agent keep the original commits: not evaluated.
    let side = String::from_utf8(m.human("rev-parse side").stdout).unwrap();
    let out = m.agent(&format!("cherry-pick {}", side.trim()));
    assert!(out.status.success(), "{}", text(&out));
    assert!(m.human("switch -q side").status.success());
    let out = m.agent("rebase -q main");
    assert!(out.status.success(), "{}", text(&out));
    // A fast-forward creates no commit.
    assert!(m.human("switch -q main").status.success());
    assert!(m.human("switch -q -c ff").status.success());
    let out = m.commit_by(false, "feat: ff\n");
    assert!(out.status.success(), "{}", text(&out));
    assert!(m.human("switch -q main").status.success());
    let out = m.agent("merge -q --ff-only ff");
    assert!(out.status.success(), "{}", text(&out));
}
