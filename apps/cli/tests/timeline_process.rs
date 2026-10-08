//! US-TMC-006 end to end (DS-US-TMC-006 § 9.4): `raptor timeline`, the real
//! binary as daemon and as client, over a temporary machine (temporary repo,
//! home and profile, never this repo nor the real profile, NFR-01). The
//! developer runs under a pty (`script`), so the engine takes them for the
//! developer.
//!
//! Two kinds of authorship, as in US-GRP-007 / US-GRP-009: Claude Code is a
//! copy of this test binary named `raptor-fake-agent` (the only agent
//! executable the debug daemon knows) that runs each line of its input with
//! `/bin/sh`, so its `git` descends from it; a registered agent is a
//! registration (`raptor agent register`) and a commit with no process of
//! its own.
//!
//! No fixed waits: each state is awaited with a deadline.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_CLAUDE: &str = "raptor-fake-agent";
const AGENT_MODE: &str = "RAPTOR_FAKE_AGENT_MODE";
const DONE: &str = "<<raptor-fake-agent-done>>";
const DEADLINE: Duration = Duration::from_secs(20);

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of the simulated Claude Code (see the module docs). As a
/// normal test it does nothing.
#[test]
fn fake_agent_entry() {
    if std::env::var_os(AGENT_MODE).is_none() {
        return;
    }
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let status = Command::new("/bin/sh").arg("-c").arg(&line).status();
        println!("{DONE} {}", status.map(|s| s.success()).unwrap_or(false));
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

struct FakeAgent {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeAgent {
    /// Runs `command` in its shell and waits for it; whether it succeeded.
    fn run(&mut self, command: &str) -> bool {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake agent died"
            );
            if let Some((_, ok)) = line.split_once(DONE) {
                return ok.trim() == "true";
            }
        }
    }
}

impl Drop for FakeAgent {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    worktree: PathBuf,
}

impl Machine {
    /// "demo": `main` with `login.txt`, branch `feat-login` and its worktree
    /// "feat-login" (`<root>/wt-feat-login`); not yet observed.
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-login"]);
        let worktree = f
            .add_worktree("feat-login", "feat-login")
            .canonicalize()
            .unwrap();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("bin")).unwrap();
        // Hooks used per command (`git -c core.hooksPath=…`): the repo's
        // hooks are never touched. The slow one keeps `git` alive a moment
        // so the engine's sample sees it and attributes the commit.
        let hooks = outside.path().join("slow-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let hook = hooks.join("reference-transaction");
        std::fs::write(
            &hook,
            "#!/bin/sh\n[ \"$1\" = committed ] && sleep 0.5\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            f,
            outside,
            worktree,
        }
    }

    /// `new`, observed.
    fn observed() -> Self {
        let m = Self::new();
        let out = m.developer(
            "en_US.UTF-8",
            &m.f.root,
            &["repo", "add", m.f.repo.to_str().unwrap()],
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
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_CLAUDE.into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", lang.into()),
        ]
    }

    /// The developer, from their own terminal in `cwd`, in `lang`.
    fn developer(&self, lang: &str, cwd: &Path, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `raptor timeline <args>` from the feat-login worktree; the output
    /// when it succeeds, a panic with what it said when it does not.
    fn timeline(&self, lang: &str, args: &[&str]) -> String {
        let mut argv = vec!["timeline"];
        argv.extend_from_slice(args);
        let out = self.developer(lang, &self.worktree, &argv);
        assert!(
            out.status.success(),
            "raptor {argv:?} failed ({}): {}",
            out.status,
            text(&out)
        );
        text(&out)
    }

    /// `raptor timeline --json <args>`, parsed.
    fn timeline_json(&self, args: &[&str]) -> Value {
        let mut argv = vec!["--json"];
        argv.extend_from_slice(args);
        let out = self.timeline("en_US.UTF-8", &argv);
        let start = out.find('{').unwrap_or_else(|| panic!("no JSON: {out}"));
        serde_json::from_str(out[start..].trim())
            .unwrap_or_else(|e| panic!("not JSON ({e}): {out}"))
    }

    fn register(&self, agent: &str) {
        let out = self.developer(
            "en_US.UTF-8",
            &self.f.root,
            &[
                "agent",
                "register",
                agent,
                "--worktree",
                self.worktree.to_str().unwrap(),
            ],
        );
        assert!(out.status.success(), "{}", text(&out));
    }

    fn launch_claude(&self) -> FakeAgent {
        let agent = self.outside.path().join("bin").join(FAKE_CLAUDE);
        std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        let mut child = Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env("en_US.UTF-8"))
            .env(AGENT_MODE, "attached")
            .env("HOME", &self.f.home)
            .current_dir(&self.worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        FakeAgent {
            child,
            stdin,
            stdout,
        }
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        let snapshot: Value = self
            .client()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        snapshot["repos"][0]["repo_id"].as_str().unwrap().to_owned()
    }

    /// Waits until the history has the commit at `HEAD` of the worktree,
    /// with an actor that `ok` accepts; the event.
    fn event_when(&self, ok: impl Fn(&Value) -> bool) -> Value {
        let head = self.f.git_in(&self.worktree, &["rev-parse", "HEAD"]);
        let head = head.trim();
        let start = Instant::now();
        loop {
            let page: Value = self
                .client()
                .call(
                    methods::EVENTS_HISTORY,
                    json!({ "repo_id": self.repo_id() }),
                )
                .unwrap();
            let events = page["events"].as_array().cloned().unwrap_or_default();
            if let Some(e) = events
                .iter()
                .find(|e| e["details"]["new_commit"].as_str() == Some(head) && ok(e))
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no commit {head} as expected: {events:#?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A commit of the developer (no agent process) that adds `file`.
    fn commit(&self, file: &str, message: &str) -> Value {
        std::fs::write(self.worktree.join(file), format!("{message}\n")).unwrap();
        self.f.git_in(&self.worktree, &["add", file]);
        self.f
            .git_in(&self.worktree, &["commit", "-q", "-m", message]);
        self.event_when(|_| true)
    }

    /// Waits until the engine has a detected Claude Code session in the
    /// worktree (`raptor status --json`).
    fn claude_session_detected(&self) {
        let start = Instant::now();
        loop {
            let out = self.developer("en_US.UTF-8", &self.f.root, &["status", "--json"]);
            let text = text(&out);
            let seen = text.find('{').is_some_and(|i| {
                serde_json::from_str::<Value>(text[i..].trim()).is_ok_and(|v| {
                    v["repos"][0]["worktrees"].as_array().is_some_and(|ws| {
                        ws.iter().any(|w| {
                            w["path"] == self.worktree.to_str().unwrap()
                                && w["sessions"].as_array().is_some_and(|s| {
                                    s.iter().any(|s| {
                                        s["agent"] == "claude-code" && s["origin"] == "detected"
                                    })
                                })
                        })
                    })
                })
            });
            if seen {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "no detected session: {text}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer("en_US.UTF-8", &self.f.root, &["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(start.elapsed() < Duration::from_secs(10));
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

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// Whether `text` has a clock time (`HH:MM`).
fn has_clock(text: &str) -> bool {
    let b = text.as_bytes();
    b.windows(5).any(|w| {
        w[0].is_ascii_digit()
            && w[1].is_ascii_digit()
            && w[2] == b':'
            && w[3].is_ascii_digit()
            && w[4].is_ascii_digit()
    })
}

fn entry_of(timeline: &Value, seq: i64) -> &Value {
    let id = format!("event:{seq}");
    timeline["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("no entries: {timeline:#}"))
        .iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no entry {id}: {timeline:#}"))
}

// ----- Scenarios ----------------------------------------------------------------

/// Escenario 2: a commit made while an agent is registered shows that agent,
/// "registered".
#[test]
fn a_registered_agent_commit_is_shown_as_registered() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::observed();
    m.register("Codex");
    let event = m.commit("codex.txt", "by codex");
    assert_eq!(event["actor"]["name"]["untrusted"], "Codex", "{event:#}");

    let shown = m.timeline("en_US.UTF-8", &[]);
    assert!(shown.contains("Codex"), "{shown}");
    assert!(shown.contains("registered"), "{shown}");
    assert!(shown.contains("codex.txt"), "{shown}");

    let timeline = m.timeline_json(&[]);
    let entry = entry_of(&timeline, event["seq"].as_i64().unwrap());
    assert_eq!(entry["actor"]["actor"], "agent", "{entry:#}");
    assert_eq!(entry["actor"]["name"]["untrusted"], "Codex", "{entry:#}");
    assert_eq!(entry["actor"]["origin"], "registered", "{entry:#}");
    m.stop();
}

/// Escenario 1: a commit by a detected Claude Code shows when, where, which
/// files and who ("Claude Code, detected").
#[test]
fn a_detected_agent_commit_shows_when_where_which_files_and_who() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::observed();
    let mut claude = m.launch_claude();
    m.claude_session_detected();
    let hooks = m.outside.path().join("slow-hooks");
    assert!(claude.run(&format!(
        "printf 'x\\n' > feature.rs && git add feature.rs && \
         git -c core.hooksPath={} commit -q -m feature",
        hooks.display()
    )));
    let event = m.event_when(|e| e["actor"]["kind"] == "claude-code");
    assert_eq!(event["actor"]["origin"], "detected", "{event:#}");

    let shown = m.timeline("en_US.UTF-8", &[]);
    // Who.
    assert!(shown.contains("Claude Code"), "{shown}");
    assert!(shown.contains("detected"), "{shown}");
    // Where.
    assert!(shown.contains("feat-login"), "{shown}");
    // Which files.
    assert!(shown.contains("feature.rs"), "{shown}");
    // When.
    assert!(has_clock(&shown), "{shown}");

    let timeline = m.timeline_json(&[]);
    let entry = entry_of(&timeline, event["seq"].as_i64().unwrap());
    assert_eq!(entry["actor"]["kind"], "claude-code", "{entry:#}");
    assert_eq!(entry["files"]["paths"][0]["untrusted"], "feature.rs");
    drop(claude);
    m.stop();
}

/// Escenario 3: what nobody is attributed to says "no agent" / "sin agente",
/// never "human" / "humano", in both languages.
#[test]
fn unattributed_changes_say_no_agent_in_es_and_en() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::observed();
    let event = m.commit("mine.txt", "by nobody");
    assert_eq!(event["actor"]["actor"], "unattributed", "{event:#}");

    let en = m.timeline("en_US.UTF-8", &[]);
    assert!(en.contains("no agent"), "{en}");
    assert!(en.contains("mine.txt"), "{en}");
    let es = m.timeline("es_ES.UTF-8", &[]);
    assert!(es.contains("sin agente"), "{es}");
    for shown in [&en, &es] {
        let lower = shown.to_lowercase();
        assert!(!lower.contains("human"), "{shown}");
    }
    // The wire and `--json` say `unattributed`.
    let timeline = m.timeline_json(&[]);
    let entry = entry_of(&timeline, event["seq"].as_i64().unwrap());
    assert_eq!(entry["actor"]["actor"], "unattributed", "{entry:#}");
    m.stop();
}

/// Escenario 5: a repo with no activity says so, in both languages, and does
/// not claim the timeline is incomplete.
#[test]
fn a_new_repo_says_there_is_no_activity_yet() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::observed();
    let es = m.timeline("es_ES.UTF-8", &[]);
    assert!(es.contains("aún no hay actividad"), "{es}");
    assert!(!es.contains("incompleto"), "{es}");
    let en = m.timeline("en_US.UTF-8", &[]);
    assert!(en.contains("no activity yet"), "{en}");
    assert!(!en.contains("incomplete"), "{en}");
    let timeline = m.timeline_json(&[]);
    assert_eq!(timeline["entries"], json!([]), "{timeline:#}");
    assert_eq!(timeline["unavailable"], json!([]), "{timeline:#}");
    m.stop();
}

/// D3: `--agent unattributed` keeps what nobody was attributed to; another
/// value matches the declared name, exact.
#[test]
fn agent_unattributed_filters_what_nobody_attributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::observed();
    m.register("Codex");
    let by_codex = m.commit("by_codex.txt", "codex");
    // A second agent makes the worktree shared: the next commit has no
    // evidence of who made it.
    m.register("Cursor");
    let by_nobody = m.commit("by_nobody.txt", "nobody");
    assert_eq!(by_nobody["actor"]["actor"], "unattributed", "{by_nobody:#}");

    let timeline = m.timeline_json(&["--agent", "unattributed"]);
    let entries = timeline["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "{timeline:#}");
    for e in entries {
        assert_eq!(e["actor"]["actor"], "unattributed", "{e:#}");
    }
    entry_of(&timeline, by_nobody["seq"].as_i64().unwrap());
    let codex_id = format!("event:{}", by_codex["seq"]);
    assert!(entries.iter().all(|e| e["id"] != codex_id.as_str()));

    let codex = m.timeline_json(&["--agent", "Codex"]);
    entry_of(&codex, by_codex["seq"].as_i64().unwrap());
    let nobody_id = format!("event:{}", by_nobody["seq"]);
    assert!(
        codex["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["id"] != nobody_id.as_str())
    );
    m.stop();
}

/// Privacy: a path with ANSI and line breaks is printed sanitized, so it
/// cannot rewrite the screen or fake a line of the timeline.
#[test]
fn paths_with_control_characters_are_printed_sanitized() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::observed();
    let name = "evil\u{1b}[31mred\nforged line.txt";
    let event = m.commit(name, "hostile path");

    let shown = m.timeline("en_US.UTF-8", &[]);
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
    assert!(!shown.contains("red\nforged"), "{shown:?}");
    assert!(shown.contains('\u{FFFD}'), "{shown:?}");
    // The wire keeps the raw text; only the screen is sanitized.
    let timeline = m.timeline_json(&[]);
    let entry = entry_of(&timeline, event["seq"].as_i64().unwrap());
    assert_eq!(entry["files"]["paths"][0]["untrusted"], name, "{entry:#}");
    m.stop();
}
