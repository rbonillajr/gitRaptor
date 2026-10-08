//! US-GRD-005 end to end (DS-US-GRD-005 T006): what Guardrails blocked shows up in
//! `raptor guard log` with its rule and its actor. The real `raptor` as daemon and hook, the real
//! dispatcher and plain Git, over temporary repos and a temporary profile (NFR-01). The agent is
//! `raptor-fake-agent`, a copy of this test binary the debug daemon knows as Claude Code. No
//! fixed waits: the log entry reaches the daemon loop before the hook gets its answer, and the
//! query goes through the same loop afterwards.
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

use std::ffi::OsString;
use std::process::{Command, Output, Stdio};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{copy_executable, git_from_path};
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";
const HUMAN_DENY: &str = r#"{"policies":{"commitAuthorship":{"mode":"human-author"}}}"#;
const HUMAN_WARN: &str =
    r#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#;

/// Entry point of the simulated Claude Code (see `guard_us_grd_018.rs`).
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
}

impl Machine {
    /// "demo" with `main` and `feat-x` pushed to a bare remote, protected; `settings`, when
    /// given, is the team configuration committed on `main` first (the floor).
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
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.write("x.txt", "x\n");
        f.git(&["add", "x.txt"]);
        f.git(&["commit", "-q", "-m", "feat-x"]);
        f.git(&["push", "-q", "origin", "feat-x"]);
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self { f, outside };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()], "en_US.UTF-8");
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(
            &["guard", "install", "--yes", m.f.repo.to_str().unwrap()],
            "en_US.UTF-8",
        );
        assert!(out.status.success(), "{}", text(&out));
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self, lang: &str) -> Vec<(&'static str, OsString)> {
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
            ("LANG", lang.into()),
        ]
    }

    fn developer(&self, args: &[&str], lang: &str) -> Output {
        developer_command(args)
            .env_clear()
            .envs(self.env(lang))
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
            .envs(self.env("en_US.UTF-8"))
            .current_dir(&self.f.repo)
            .output()
            .unwrap()
    }

    /// `git <args>` run by the simulated Claude Code.
    fn agent(&self, args: &str) -> Output {
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
            .envs(self.env("en_US.UTF-8"))
            .env(AGENT_CMD, self.git_line(args))
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A commit of a new file with `message` (written to a file outside the repo).
    fn commit_with(&self, agent: bool, message: &str, command: &str) -> Output {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("f{n}.txt");
        std::fs::write(self.f.repo.join(&name), format!("{n}\n")).unwrap();
        let msg = self.outside.path().join(format!("msg{n}"));
        std::fs::write(&msg, message).unwrap();
        let add = self.human(&format!("add {name}"));
        assert!(add.status.success(), "{}", text(&add));
        let args = format!("{command} -F '{}'", msg.to_str().unwrap());
        if agent {
            self.agent(&args)
        } else {
            self.human(&args)
        }
    }

    /// `raptor guard log --json` of "demo".
    fn log(&self) -> Value {
        let out = self.developer(
            &["guard", "log", "--json", self.f.repo.to_str().unwrap()],
            "en_US.UTF-8",
        );
        assert!(out.status.success(), "{}", text(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The pty of `script` may echo control characters before the output.
        let line = stdout
            .lines()
            .find_map(|l| l.find('{').map(|i| &l[i..]))
            .unwrap_or_else(|| panic!("no JSON: {stdout}"));
        serde_json::from_str(line.trim()).unwrap()
    }

    fn log_text(&self, lang: &str) -> String {
        let out = self.developer(&["guard", "log", self.f.repo.to_str().unwrap()], lang);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// Whether any file under `dir` contains `needle`.
fn profile_contains(dir: &std::path::Path, needle: &[u8]) -> bool {
    // The daemon creates and removes temporary files while the test scans the profile: an entry
    // that vanished between the listing and the read held nothing that can matter.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        if meta.is_dir() {
            profile_contains(&path, needle)
        } else if meta.is_file() {
            std::fs::read(&path)
                .unwrap_or_default()
                .windows(needle.len())
                .any(|w| w == needle)
        } else {
            false
        }
    })
}

/// The one entry whose operation kind and first rule are these.
fn find<'a>(log: &'a Value, operation: &str, rule: &str) -> &'a Value {
    log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["operation"]["kind"] == operation && e["reasons"][0]["rule"] == rule)
        .unwrap_or_else(|| panic!("no {operation} / {rule} in {log:#}"))
}

/// The dogfooding finding of 2026-10-07, reproduced on a repo protected with `raptor guard
/// install` ("hooks only"): an agent's commit without its trailer, the same commit with
/// `--no-verify` (denied by the second line), the agent deleting `main` and the person
/// force-pushing `main` were blocked and showed up nowhere. Each one is now in the log with its
/// rule and its actor, the commit message is not stored, and the repo is untouched
/// (US-GRD-005 escenarios 1 y 4).
#[test]
fn the_dogfooding_blocks_are_logged() {
    let m = Machine::new(None);
    let out = m.developer(
        &["guard", "status", m.f.repo.to_str().unwrap()],
        "en_US.UTF-8",
    );
    assert!(text(&out).contains("hooks"), "{}", text(&out));

    let secret = "feat: secret-subject-q7Zr";
    let out = m.commit_with(true, &format!("{secret}\n"), "commit -q");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(m.human("reset -q").status.success());

    let out = m.commit_with(true, "feat: skipped hooks\n", "commit -q --no-verify");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(m.human("reset -q").status.success());

    let out = m.agent("branch -D main");
    assert!(!out.status.success(), "{}", text(&out));

    assert!(m.human("switch -q main").status.success());
    assert!(
        m.human("commit -q --amend -m 'main rewritten'")
            .status
            .success()
    );
    let out = m.human("push -f origin main");
    assert!(!out.status.success(), "{}", text(&out));

    let log = m.log();
    assert_eq!(log["summary"]["blocked"], 4, "{log:#}");

    let commit = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["operation"]["kind"] == "commit" && e["operation"]["stage"] != "second-line")
        .unwrap_or_else(|| panic!("no commit-msg denial in {log:#}"));
    assert_eq!(commit["reasons"][0]["rule"], "authorship.trailer-required");
    assert_eq!(commit["kind"], "denial");
    assert_eq!(commit["actor"], "claude-code");
    assert_eq!(commit["layer"], "hooks");
    assert_eq!(commit["origin"], "daemon");
    assert_eq!(commit["branch"]["untrusted"], "feat-x");
    assert!(commit["worktree"]["untrusted"].is_string(), "{commit:#}");
    assert_eq!(commit["authorship"]["agentTrailer"], false);

    let second = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["operation"]["stage"] == "second-line")
        .unwrap_or_else(|| panic!("no second-line denial in {log:#}"));
    assert_eq!(second["reasons"][0]["rule"], "authorship.trailer-required");
    assert_eq!(second["actor"], "claude-code");

    let delete = find(&log, "ref-transaction", "minimum.base-branch-delete");
    assert_eq!(delete["actor"], "claude-code");
    assert_eq!(delete["operation"]["refs"][0]["name"]["untrusted"], "main");
    assert_eq!(delete["operation"]["refs"][0]["change"], "delete");

    let push = find(&log, "push", "minimum.force-push");
    assert!(push["actor"].is_null(), "the person: unattributed");
    assert_eq!(push["operation"]["remote"]["untrusted"], "origin");
    assert_eq!(push["operation"]["refs"][0]["name"]["untrusted"], "main");
    assert_eq!(push["operation"]["refs"][0]["change"], "force");

    // Privacy (M-06, D7): the message is nowhere in the profile.
    assert!(
        !profile_contains(&m.f.profile, b"secret-subject-q7Zr"),
        "the commit message reached the profile"
    );
    // Escenario 5: nothing of the log is in the repo.
    let status = String::from_utf8(m.human("status --porcelain --ignored").stdout).unwrap();
    assert!(!status.contains("sqlite"), "{status}");

    // In both languages, with the rule and the actor.
    let en = m.log_text("en_US.UTF-8");
    assert!(en.contains("4 blocked actions in the last 7 days"), "{en}");
    assert!(en.contains("claude-code"), "{en}");
    assert!(en.contains("unattributed"), "{en}");
    assert!(
        en.contains("author not available: the commit was never created"),
        "{en}"
    );
    let es = m.log_text("es_ES.UTF-8");
    assert!(
        es.contains("4 acciones bloqueadas en los últimos 7 días"),
        "{es}"
    );
    assert!(es.contains("sin atribuir"), "{es}");
    assert!(
        es.contains("autor no disponible: el commit no llegó a crearse"),
        "{es}"
    );

    // `raptor guard status` points to the log.
    let out = m.developer(
        &["guard", "status", m.f.repo.to_str().unwrap()],
        "en_US.UTF-8",
    );
    assert!(
        text(&out).contains("4 blocked actions in the last 7 days · raptor guard log"),
        "{}",
        text(&out)
    );
}

/// A burst of distinct denials (more than the 100 new rows a minute): rows over the cap are
/// aggregated, never dropped, so the count in `raptor guard log` and `raptor guard status` is
/// the real one (ADR-GRD-006 § 2, Validación 6).
#[test]
fn a_burst_of_denials_keeps_its_count() {
    let m = Machine::new(None);
    const N: usize = 120;
    for i in 0..N {
        // A different branch in `HEAD` makes each denial a distinct entry.
        assert!(
            m.human(&format!("symbolic-ref HEAD refs/heads/burst-{i}"))
                .status
                .success()
        );
        let out = m.human("branch -D main");
        assert!(!out.status.success(), "{}", text(&out));
    }
    let log = m.log();
    assert_eq!(log["summary"]["blocked"], N, "{log:#}");
    // Whether some of them went over the cap depends on the machine's speed; the cap itself is
    // tested in the store (`crates/core/tests/guard_log.rs`).
    let out = m.developer(
        &["guard", "status", m.f.repo.to_str().unwrap()],
        "en_US.UTF-8",
    );
    assert!(
        text(&out).contains(&format!("{N} blocked actions in the last 7 days")),
        "{}",
        text(&out)
    );
}

/// With the engine down the hook decides alone and nothing is logged (the spool is deferred):
/// the log says so for that period instead of showing a silent 0.
#[test]
fn a_period_with_the_engine_down_is_not_a_silent_zero() {
    let m = Machine::new(None);
    let out = m.developer(&["daemon", "stop", "--yes"], "en_US.UTF-8");
    assert!(out.status.success(), "{}", text(&out));
    let start = std::time::Instant::now();
    while running_pid(&m.dirs().state).unwrap().is_some() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "daemon did not stop"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = m.human("branch -D main");
    assert!(
        !out.status.success(),
        "degraded mode still denies: {}",
        text(&out)
    );
    let log = m.log();
    assert_eq!(log["summary"]["blocked"], 0, "{log:#}");
    assert!(
        !log["unloggedPeriods"].as_array().unwrap().is_empty(),
        "{log:#}"
    );
    let en = m.log_text("en_US.UTF-8");
    assert!(en.contains("the engine was not running"), "{en}");
    let es = m.log_text("es_ES.UTF-8");
    assert!(es.contains("el motor no estaba en marcha"), "{es}");
}

/// `--no-verify` skips the commit hooks; the second line denies and logs it (DS-US-GRD-018 D6).
#[test]
fn a_no_verify_denied_by_the_second_line_is_logged() {
    let m = Machine::new(Some(HUMAN_DENY));
    let out = m.commit_with(
        true,
        &format!("feat: skipped hooks\n\n{CLAUDE}\n"),
        "commit -q --no-verify",
    );
    assert!(!out.status.success(), "{}", text(&out));
    let log = m.log();
    assert_eq!(log["summary"]["blocked"], 1, "{log:#}");
    let e = find(&log, "commit", "authorship.human-author");
    assert_eq!(e["operation"]["stage"], "second-line");
    assert_eq!(e["actor"], "claude-code");
    assert_eq!(e["authorship"]["agentTrailer"], true);
    assert_eq!(e["authorship"]["coauthors"][0], "claude-code");
}

/// Escenario 3 (de los permitidos): 10 allowed commits leave no entry.
#[test]
fn allowed_operations_leave_no_entries() {
    let m = Machine::new(None);
    for i in 0..10 {
        let out = m.commit_with(false, &format!("chore: {i}\n"), "commit -q");
        assert!(out.status.success(), "{}", text(&out));
    }
    let log = m.log();
    assert_eq!(log["summary"]["blocked"], 0);
    assert_eq!(log["entries"].as_array().unwrap().len(), 0, "{log:#}");
}

/// A warning with the hooks is one entry, never one per hook (ADR-GRD-003 § 6), and does not
/// count as blocked (BR-AUTH-005).
#[test]
fn a_warning_is_logged_once_outside_the_kpi() {
    let m = Machine::new(Some(HUMAN_WARN));
    let out = m.commit_with(true, &format!("feat: warned\n\n{CLAUDE}\n"), "commit -q");
    assert!(out.status.success(), "{}", text(&out));
    let log = m.log();
    assert_eq!(log["summary"]["blocked"], 0, "{log:#}");
    assert_eq!(log["summary"]["notices"], 1, "{log:#}");
    let entries = log["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{log:#}");
    assert_eq!(entries[0]["kind"], "notice");
    assert_eq!(entries[0]["count"], 1);
    let en = m.log_text("en_US.UTF-8");
    assert!(en.contains("author: see raptor events"), "{en}");
    // `flexible` relaxes only from a confirmed floor, which no command confirms yet
    // (US-GRD-014): its entry is covered in `crates/core/tests/guard_evaluate.rs`.
}
