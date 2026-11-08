//! US-GRP-008 end to end: the work the developer does from their editor is
//! never attributed to Claude Code (BR-EDGE-004). Each scenario of the Dev
//! Spec (§ 9.4) with the real `raptor` binary as daemon and as client, over a
//! temporary machine built by the "intact repo" harness (INF-GRP-001):
//! temporary repo and profile, never this repo nor the real profile (NFR-01).
//! The developer runs reserved commands under a pty (`script`).
//!
//! The processes are simulated with copies of this test binary (a copy of a
//! system binary is killed by macOS), launched in a worktree and outside
//! `GITRAPTOR_AGENT_EXECUTABLES` unless said:
//! - the editors `cursor`, `code` and `codex`: they write a file and end;
//! - Claude Code, `raptor-fake-agent`: it runs each line of its input with
//!   `/bin/sh`, so its `git`s descend from it.
//!
//! The harness never reads the environment or the processes of the session
//! that runs it. Waits are to a state with a deadline, never a fixed time.
//! The entry of the copies is the first lines of
//! `repo_intact_editor_scenarios_do_not_modify_it`.
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
use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, Snapshot, TrailerCheck,
};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, UntrustedName, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The test the copies of this binary run, and what they do there.
const ENTRY: &str = "repo_intact_editor_scenarios_do_not_modify_it";
const FAKE_PROC: &str = "RAPTOR_FAKE_PROC";
const FAKE_SCRIPT: &str = "RAPTOR_FAKE_SCRIPT";
/// Printed by the simulated Claude Code after each command.
const DONE: &str = "<<raptor-fake-claude-done>>";

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// What a copy of this binary does when it is not a test: `claude` runs each
/// line of its input with `/bin/sh` and prints [`DONE`] after each, until its
/// input ends; `editor` runs [`FAKE_SCRIPT`] once and ends. Returns `false`
/// for a normal test run.
fn run_as_fake_process() -> bool {
    let Some(kind) = std::env::var_os(FAKE_PROC) else {
        return false;
    };
    if kind == "editor" {
        let script = std::env::var(FAKE_SCRIPT).unwrap();
        let status = Command::new("/bin/sh").arg("-c").arg(script).status();
        std::process::exit(if status.is_ok_and(|s| s.success()) {
            0
        } else {
            1
        });
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

/// A running simulated Claude Code.
struct FakeClaude {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeClaude {
    /// Runs `command` in its shell and waits for it.
    fn run(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "({command}) < /dev/null").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake claude died"
            );
            // libtest may print its header on the same line.
            if let Some(ok) = line.split_once(DONE).map(|(_, ok)| ok) {
                assert_eq!(ok.trim(), "true", "{command} failed");
                return;
            }
        }
    }

    /// Normal close: its input ends and it exits.
    fn close(mut self) {
        self.stdin.take();
        self.child.wait().unwrap();
    }
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The temporary machine, plus what lives outside the fixture's root: the
/// folders the simulated processes are installed in and a slow hook.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    feat_pagos: PathBuf,
    feat_login: PathBuf,
}

impl Machine {
    fn new(f: Fixture, feat_pagos: PathBuf, feat_login: PathBuf) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("bin")).unwrap();
        // A hook folder used per command (`git -c core.hooksPath=…`): the
        // repo's hooks are never touched.
        let hooks = outside.path().join("slow-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let post_commit = hooks.join("post-commit");
        std::fs::write(&post_commit, "#!/bin/sh\nsleep 1\n").unwrap();
        std::fs::set_permissions(&post_commit, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            f,
            outside,
            feat_pagos,
            feat_login,
        }
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn slow_hooks(&self) -> PathBuf {
        self.outside.path().join("slow-hooks")
    }

    /// A file of the folder outside the fixture, to signal between processes.
    fn signal(&self, name: &str) -> PathBuf {
        self.outside.path().join(name)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn raptor(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    /// The developer, from their own terminal.
    fn developer(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn add(&self) {
        let out = self.developer(&["repo", "add", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }

    /// A copy of this test binary named `name`, outside the fixture.
    fn install(&self, name: &str) -> PathBuf {
        let exe = self.outside.path().join("bin").join(name);
        if !exe.exists() {
            gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &exe);
        }
        exe
    }

    fn fake(&self, name: &str, kind: &str, worktree: &Path) -> Command {
        let mut cmd = Command::new(self.install(name));
        cmd.args([ENTRY, "--exact", "--nocapture", "--test-threads=1"])
            .env_clear()
            .envs(self.env())
            .env(FAKE_PROC, kind)
            .env("HOME", &self.f.home)
            .current_dir(worktree);
        cmd
    }

    /// An editor of the developer, `cursor`, `code` or `codex`, runs `script`
    /// with its working folder in `worktree` and ends.
    fn editor(&self, name: &str, worktree: &Path, script: &str) {
        let status = self
            .fake(name, "editor", worktree)
            .env(FAKE_SCRIPT, script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{name} {script}: {status}");
    }

    /// The developer launches Claude Code in `worktree`.
    fn launch_claude(&self, worktree: &Path) -> FakeClaude {
        let mut child = self
            .fake(FAKE_AGENT, "claude", worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        FakeClaude {
            child,
            stdin,
            stdout,
        }
    }

    fn status(&self) -> Value {
        let out = self.raptor(&["status", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        let snapshot: Snapshot = self
            .client()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        snapshot.repos[0].repo_id.clone()
    }

    /// The worktree `status --json` shows at `worktree`, once `ok` accepts it.
    fn worktree_when(&self, worktree: &Path, ok: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let status = self.status();
            let found = status["repos"][0]["worktrees"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["path"] == worktree.to_str().unwrap())
                .cloned();
            if let Some(w) = found.as_ref().filter(|w| ok(w)) {
                return w.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{}: {found:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The worktree once `file` is among its changes.
    fn with_change(&self, worktree: &Path, file: &str) -> Value {
        self.worktree_when(worktree, |w| {
            w["changes"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c["path"] == file))
        })
    }

    /// `raptor sessions --all --json`.
    fn all_sessions(&self) -> Vec<Value> {
        let out = self.raptor(&["sessions", "--all", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        value["sessions"].as_array().unwrap().clone()
    }

    fn history(&self) -> Vec<GitEventView> {
        let page: EventsHistoryResult = self
            .client()
            .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
            .unwrap();
        page.events
    }

    /// Waits until the history has an event of `kind` in `worktree`.
    fn event(&self, kind: GitEventKind, worktree: &Path) -> GitEventView {
        let start = Instant::now();
        loop {
            let history = self.history();
            if let Some(e) = history
                .iter()
                .find(|e| e.kind == kind && Path::new(e.worktree.raw()) == worktree)
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no {kind:?} in {}: {history:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until `path` exists.
    fn wait_for(&self, path: &Path, what: &str) {
        let start = Instant::now();
        while !path.exists() || std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "{what} did not happen"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Stops the engine and returns what the store holds for the repo: the
    /// sessions and the attribution records of all of them.
    fn stored(&self, repo_id: &str) -> (Vec<String>, usize) {
        self.stop();
        let (profile, _) = Profile::open(self.dirs()).unwrap();
        let (store, _) = profile.open_store(repo_id).unwrap();
        let sessions: Vec<String> = store
            .sessions_with_state()
            .unwrap()
            .into_iter()
            .map(|s| format!("{s:?}"))
            .collect();
        let records = store
            .events_for_worktree(&self.feat_pagos)
            .unwrap()
            .iter()
            .chain(store.events_for_worktree(&self.feat_login).unwrap().iter())
            .filter(|e| e.session_id.is_some())
            .count();
        (sessions, records)
    }

    /// The commit event of `worktree` in the store, after the engine stopped.
    fn stored_commit_session(&self, repo_id: &str, worktree: &Path) -> Option<String> {
        self.stop();
        let (profile, _) = Profile::open(self.dirs()).unwrap();
        let (store, _) = profile.open_store(repo_id).unwrap();
        store
            .events_for_worktree(worktree)
            .unwrap()
            .into_iter()
            .find(|e| e.kind == "commit")
            .expect("a stored commit event")
            .session_id
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
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

/// "demo": `main` with `login.txt`, `pagos.txt` and `README.md`, and the
/// worktrees "feat-pagos" and "feat-login" (`<root>/wt-<name>`).
fn demo() -> Machine {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.write("pagos.txt", "pay\n");
    f.write("README.md", "# demo\n");
    f.git(&["add", "login.txt", "pagos.txt", "README.md"]);
    f.git(&["commit", "-q", "-m", "demo"]);
    f.git(&["branch", "feat-pagos"]);
    f.git(&["branch", "feat-login"]);
    let pagos = f
        .add_worktree("feat-pagos", "feat-pagos")
        .canonicalize()
        .unwrap();
    let login = f
        .add_worktree("feat-login", "feat-login")
        .canonicalize()
        .unwrap();
    Machine::new(f, pagos, login)
}

/// `demo` observed.
fn observed() -> Machine {
    let m = demo();
    m.add();
    m
}

fn claude_code_detected() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

/// One detected Claude Code session, in `state`.
fn is_claude(s: &Value, state: &str) -> bool {
    s["agent"] == "claude-code" && s["origin"] == "detected" && s["state"] == state
}

fn has_session(w: &Value, ok: impl Fn(&Value) -> bool) -> bool {
    w["sessions"].as_array().is_some_and(|s| s.iter().any(&ok))
}

fn engine_profile() -> Exceptions {
    Exceptions::engine_profile("profile")
        .with(Exception::Subtree {
            scope: "profile".into(),
            prefix: "run".into(),
        })
        .with(Exception::DirTimes {
            scope: "profile".into(),
            path: PathBuf::new(),
        })
}

/// The value of `key` in the CLI catalog of `lang` (any of its files).
fn catalog(lang: &str, key: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("i18n")
        .join(lang);
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let content = std::fs::read_to_string(entry.path()).unwrap();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once(" = ")
                && k == key
            {
                return v.to_owned();
            }
        }
    }
    panic!("the {lang} catalog has no key {key}");
}

/// The line `raptor status` shows for one modified, not staged file: the
/// catalog's `status.changes-actor` with `status.changes` and the text of
/// `actor.unattributed` (not a literal).
fn expected_changes_line(lang: &str) -> String {
    let changes = catalog(lang, "status.changes")
        .replace("{total}", "1")
        .replace("{staged}", "0")
        .replace("{unstaged}", "1")
        .replace("{untracked}", "0");
    catalog(lang, "status.changes-actor")
        .replace("{changes}", &changes)
        .replace("{actor}", &catalog(lang, "actor.unattributed"))
}

fn assert_text_shows_the_actor(m: &Machine) {
    for (lang, locale) in [("en", "en_US.UTF-8"), ("es", "es_ES.UTF-8")] {
        let out = m.raptor(&["status"], &[("LANG", locale)]);
        let shown = text(&out);
        let expected = expected_changes_line(lang);
        assert!(
            shown.lines().any(|l| l.contains(&expected)),
            "the {lang} text must show `{expected}` (status.changes-actor, actor.unattributed):\n{shown}"
        );
    }
}

fn assert_unattributed_change(w: &Value) {
    assert_eq!(
        w["changes_actor"],
        json!({"actor": "unattributed"}),
        "the changes must figure as unattributed (changes_actor): {w:#}"
    );
}

/// Escenario 1: a change made from `editor` in "feat-pagos" is unattributed.
fn a_change_without_an_identified_agent(editor: &str) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let pagos = m.feat_pagos.clone();
    // Dado ningún agente identificado en "feat-pagos".
    assert!(m.all_sessions().is_empty());

    // Cuando el desarrollador modifica "pagos.txt" desde su editor.
    m.editor(editor, &pagos, "echo edited >> pagos.txt");
    let w = m.with_change(&pagos, "pagos.txt");

    // Entonces el cambio figura como sin atribuir, y ninguna sesión.
    assert_eq!(w["sessions"], json!([]), "{w:#}");
    assert_unattributed_change(&w);
    assert_text_shows_the_actor(&m);
    let all = m.all_sessions();
    assert!(all.is_empty(), "{all:#?}");
    if editor == "codex" {
        assert!(!all.iter().any(|s| s["agent"] == "other"), "{all:#?}");
    }
    let repo_id = m.repo_id();
    let (sessions, records) = m.stored(&repo_id);
    assert!(
        sessions.is_empty(),
        "an editor made a session row: {sessions:#?}"
    );
    assert_eq!(records, 0, "an editor made an attribution record");
}

/// Cursor del desarrollador.
#[test]
fn a_change_without_an_identified_agent_is_unattributed_cursor() {
    a_change_without_an_identified_agent("cursor");
}

/// Otro editor del desarrollador.
#[test]
fn a_change_without_an_identified_agent_is_unattributed_other_editor() {
    a_change_without_an_identified_agent("code");
}

/// Un agente sin soporte completo que no se registró, como Codex.
#[test]
fn a_change_without_an_identified_agent_is_unattributed_unregistered_codex() {
    a_change_without_an_identified_agent("codex");
}

/// Escenario 2: a doubtful change next to Claude Code is not Claude Code's.
#[test]
fn a_doubtful_change_next_to_claude_code_is_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let login = m.feat_login.clone();
    let _claude = m.launch_claude(&login);
    m.worktree_when(&login, |w| has_session(w, |s| is_claude(s, "active")));

    m.editor("cursor", &login, "echo edited >> README.md");
    let w = m.with_change(&login, "README.md");
    let sessions = w["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{w:#}");
    assert_ne!(sessions[0]["shared"], json!(true), "{w:#}");
    assert_unattributed_change(&w);
    assert_ne!(w["changes_actor"]["actor"], "claude-code", "{w:#}");
}

/// Escenario 3: a developer commit with no visible `git` while a `git` of
/// Claude Code (read only) is alive is unattributed.
#[test]
fn a_developer_commit_next_to_claude_code_without_a_visible_git_is_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let login = m.feat_login.clone();
    let mut claude = m.launch_claude(&login);
    m.worktree_when(&login, |w| has_session(w, |s| is_claude(s, "active")));
    // A `git` of Claude Code that only reads and stays alive for a few
    // seconds; its first answer says it is running.
    let (answered, ended) = (m.signal("git-answered"), m.signal("git-ended"));
    claude.run(&format!(
        "( (echo HEAD; sleep 4) | git cat-file --batch-check > {} 2>&1; echo ended > {} ) > /dev/null 2>&1 &",
        answered.display(),
        ended.display()
    ));
    m.wait_for(&answered, "the read-only git of Claude Code starting");

    // The developer's editor writes the commit with no `git` the engine sees:
    // a `git` run from outside the repo with `--git-dir` and `--work-tree`.
    std::fs::write(login.join("login.txt"), "user\ndeveloper\n").unwrap();
    let admin = admin_dir(&login);
    let out =
        m.f.git_command(
            m.outside.path(),
            &[
                "--git-dir",
                admin.to_str().unwrap(),
                "--work-tree",
                login.to_str().unwrap(),
                "commit",
                "-q",
                "-a",
                "-m",
                "developer",
            ],
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let e = m.event(GitEventKind::Commit, &login);
    assert_eq!(
        e.actor,
        Actor::Unattributed,
        "a commit the developer made must not be Claude Code's: {e:#?}"
    );
    let repo_id = m.repo_id();
    // Its `git` ends before the next scenario's machine is built.
    m.wait_for(&ended, "the read-only git of Claude Code ending");
    assert_eq!(m.stored_commit_session(&repo_id, &login), None);
}

/// Escenario 3: when S3 sees no `git` of the session at all.
#[test]
fn a_developer_commit_after_claude_codes_git_ended_is_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let login = m.feat_login.clone();
    let mut claude = m.launch_claude(&login);
    m.worktree_when(&login, |w| has_session(w, |s| is_claude(s, "active")));
    // A `git` of Claude Code that already ended.
    claude.run("git status -s > /dev/null");

    std::fs::write(login.join("login.txt"), "user\ndeveloper\n").unwrap();
    m.f.git_in(&login, &["commit", "-qam", "developer"]);
    let e = m.event(GitEventKind::Commit, &login);
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");
    // A hint is not an attribution: its trailer is never confirmed.
    if let Some(hint) = &e.inferred {
        assert_eq!(hint.trailer, Some(TrailerCheck::Unconfirmed), "{e:#?}");
    }
    let repo_id = m.repo_id();
    assert_eq!(m.stored_commit_session(&repo_id, &login), None);
}

/// Q52: no signal attributes a file change, not even the agent's own.
#[test]
fn an_agents_own_uncommitted_edit_is_unattributed_too() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let login = m.feat_login.clone();
    let mut claude = m.launch_claude(&login);
    m.worktree_when(&login, |w| has_session(w, |s| is_claude(s, "active")));

    claude.run("printf 'user\\nedited by claude\\n' > login.txt");
    let w = m.with_change(&login, "login.txt");
    assert_unattributed_change(&w);
}

/// Q53: a registered other agent does not take the uncommitted changes; its
/// commits are still its own (US-GRP-009 D8).
#[test]
fn a_registered_other_agent_does_not_take_the_uncommitted_changes() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let pagos = m.feat_pagos.clone();
    let out = m.developer(&[
        "agent",
        "register",
        "Codex",
        "--worktree",
        pagos.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let is_codex = |s: &Value| {
        s["agent"] == "other"
            && s["agent_name"] == "Codex"
            && s["origin"] == "registered"
            && s["state"] == "active"
    };
    m.worktree_when(&pagos, |w| has_session(w, is_codex));

    m.editor("cursor", &pagos, "echo edited >> pagos.txt");
    let w = m.with_change(&pagos, "pagos.txt");
    assert_unattributed_change(&w);

    m.f.git_in(&pagos, &["commit", "-qam", "pagos"]);
    let e = m.event(GitEventKind::Commit, &pagos);
    assert_eq!(
        e.actor,
        Actor::Agent {
            kind: AgentKind::Other,
            name: Some(UntrustedName::new("Codex")),
            origin: AgentOrigin::Registered,
        },
        "{e:#?}"
    );
}

/// The commit with a slow hook of US-GRP-007 stays Claude Code's, backed by
/// S3, after the rule that a `git` of the session counts only if it writes.
#[test]
fn a_claude_code_commit_is_still_attributed_by_s3() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = observed();
    let login = m.feat_login.clone();
    let mut claude = m.launch_claude(&login);
    m.worktree_when(&login, |w| has_session(w, |s| is_claude(s, "active")));
    claude.run(&format!(
        "printf 'user\\npassword\\n' > login.txt && git add login.txt && \
         git -c core.hooksPath={} commit -q -m 'add password'",
        m.slow_hooks().display()
    ));
    let e = m.event(GitEventKind::Commit, &login);
    assert_eq!(e.actor, claude_code_detected(), "{e:#?}");
    let repo_id = m.repo_id();
    claude.close();
    m.stop();
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let (store, _) = profile.open_store(&repo_id).unwrap();
    let commit = store
        .events_for_worktree(&login)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "commit")
        .unwrap();
    assert!(commit.session_id.is_some(), "{commit:#?}");
    assert_eq!(commit.evidence.as_deref(), Some(r#"{"signals":["s3"]}"#));
}

/// The Git directory of a linked worktree, from its `.git` file.
fn admin_dir(worktree: &Path) -> PathBuf {
    let content = std::fs::read_to_string(worktree.join(".git")).unwrap();
    PathBuf::from(content.trim().strip_prefix("gitdir: ").unwrap())
}

/// BR-CONS-001: the scenarios do not modify the repo (zero differences
/// outside the profile and what the test itself writes).
#[test]
fn repo_intact_editor_scenarios_do_not_modify_it() {
    // A copy of this binary acting as an editor or as Claude Code.
    if run_as_fake_process() {
        return;
    }
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = demo();
    let (pagos, login) = (m.feat_pagos.clone(), m.feat_login.clone());
    // What the editors write is the test's own, written before the check.
    m.editor("cursor", &pagos, "echo edited >> pagos.txt");
    m.editor("code", &login, "echo edited >> README.md");
    let report = check("US-GRP-008 editors", &m.f, &engine_profile(), || {
        m.add();
        let claude = m.launch_claude(&login);
        m.worktree_when(&login, |w| has_session(w, |s| is_claude(s, "active")));
        let w = m.with_change(&login, "README.md");
        assert_unattributed_change(&w);
        let w = m.with_change(&pagos, "pagos.txt");
        assert_unattributed_change(&w);
        let out = m.raptor(&["status"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        claude.close();
        m.stop();
    });
    report.assert_intact();
}
