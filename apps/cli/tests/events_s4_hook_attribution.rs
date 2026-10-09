//! S4 end to end (DS-US-GRP-007 § 7, SPIKE-GRP-001): with Guardrails installed, the
//! `reference-transaction` hook resolves who runs the `git` while it is still alive, and the event
//! the observer records for that same ref and new oid is attributed with it, so a quick commit of
//! Claude Code no longer loses the S3 race. The real `raptor` as daemon and hook, the real
//! `raptor-hook` dispatcher and plain Git, over a temporary repo and profile (NFR-01).
//!
//! Claude Code is simulated as in `claude_sessions.rs`: a long-lived copy of this test binary
//! named `raptor-fake-agent` that runs each line of its input with `/bin/sh`, so its `git`s
//! descend from it. No fixed waits.
//!
//! macOS only, like `claude_sessions.rs` (`script` options, process detection). Linux and
//! Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{ClientKind, EventsHistoryResult, GitEventView, Snapshot};
use gitraptor_api::{Actor, AgentKind, AgentOrigin, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
/// Printed by the simulated Claude Code after each command.
const DONE: &str = "<<raptor-fake-claude-done>>";
const TRAILER: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";
const S4: &str = r#"{"signals":["s4"]}"#;
const S3: &str = r#"{"signals":["s3"]}"#;

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of the simulated Claude Code: as `raptor-fake-agent` with `RAPTOR_FAKE_CLAUDE`, it
/// runs every line of its input with `/bin/sh` and prints [`DONE`] after each. As a normal test it
/// does nothing.
#[test]
fn fake_claude_entry() {
    if std::env::var_os(FAKE_CLAUDE).is_none() {
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

struct FakeClaude {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl FakeClaude {
    /// Runs `command` in its shell and waits for it.
    fn run(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake claude died"
            );
            if let Some(ok) = line.split_once(DONE).map(|(_, ok)| ok) {
                assert_eq!(ok.trim(), "true", "{command} failed");
                return;
            }
        }
    }
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    /// The main worktree, canonical (the form the engine stores).
    wt: PathBuf,
}

impl Machine {
    /// "demo" with `main`, observed; Guardrails installed when `guarded`.
    fn new(guarded: bool) -> Self {
        let f = Fixture::with_commit(&git_from_path());
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let wt = f.repo.canonicalize().unwrap();
        let m = Self { f, outside, wt };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        if guarded {
            m.guard();
        }
        m
    }

    fn guard(&self) {
        let out = self.developer(&["guard", "install", "--yes", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
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

    /// `raptor <args>` from the developer's own terminal (a pty through `script`).
    fn developer(&self, args: &[&str]) -> Output {
        Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR])
            .args(args)
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

    /// `git <args>` by the developer: no agent in the ancestry.
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

    /// Read from the files of `.git`, never with a `git`: a `git` of the developer in the repo
    /// right after an agent commit is a foreign `git` in its S3 window, which makes S3 ambiguous
    /// and, by ADR-GRP-012, leaves the event without the single-session hint.
    fn head(&self) -> String {
        let git_dir = self.f.repo.join(".git");
        let head = std::fs::read_to_string(git_dir.join("HEAD")).unwrap();
        let Some(name) = head.trim().strip_prefix("ref: ") else {
            return head.trim().to_owned();
        };
        if let Ok(oid) = std::fs::read_to_string(git_dir.join(name)) {
            return oid.trim().to_owned();
        }
        let packed = std::fs::read_to_string(git_dir.join("packed-refs")).unwrap();
        packed
            .lines()
            .find_map(|l| l.strip_suffix(name)?.strip_suffix(' '))
            .unwrap_or_else(|| panic!("no {name} in {}", git_dir.display()))
            .to_owned()
    }

    /// The developer launches Claude Code in the main worktree and it is detected.
    fn launch_claude(&self) -> FakeClaude {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        let mut child = Command::new(agent)
            .args([
                "fake_claude_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(FAKE_CLAUDE, "1")
            .current_dir(&self.f.repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let claude = FakeClaude {
            child,
            stdin,
            stdout,
        };
        self.wait_session();
        claude
    }

    /// Waits until `status --json` shows one active Claude Code session in the worktree.
    fn wait_session(&self) {
        let start = Instant::now();
        loop {
            let out = Command::new(RAPTOR)
                .args(["status", "--json"])
                .env_clear()
                .envs(self.env())
                .current_dir(&self.f.root)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            let status: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            let sessions = status["repos"][0]["worktrees"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|w| w["path"] == self.wt.to_str().unwrap())
                .and_then(|w| w["sessions"].as_array().cloned())
                .unwrap_or_default();
            if sessions.len() == 1
                && sessions[0]["agent"] == "claude-code"
                && sessions[0]["state"] == "active"
            {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no session: {status:#?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// A quick commit (`git commit -qm`, no slow hook) by the simulated Claude Code, with its
    /// trailer (the default `agents-commit` policy); its new oid.
    fn agent_commit(&self, claude: &mut FakeClaude, n: usize) -> String {
        claude.run(&format!(
            "printf '{n}\\n' >> agent.txt && {} add agent.txt && {} commit -q -m 'agent {n}' -m '{TRAILER}'",
            self.git_line(""),
            self.git_line("")
        ));
        self.head()
    }

    /// A commit by the developer; its new oid.
    fn human_commit(&self, n: usize) -> String {
        std::fs::write(self.f.repo.join(format!("human{n}.txt")), "h\n").unwrap();
        let out = self.human(&format!("add human{n}.txt"));
        assert!(out.status.success(), "{}", text(&out));
        let out = self.human(&format!("commit -q -m 'human {n}'"));
        assert!(out.status.success(), "{}", text(&out));
        self.head()
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

    /// Waits until the history has the event whose new commit is `oid`.
    fn event(&self, oid: &str) -> GitEventView {
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            let page: EventsHistoryResult = self
                .client()
                .call(methods::EVENTS_HISTORY, json!({"repo_id": repo_id}))
                .unwrap();
            if let Some(e) = page
                .events
                .iter()
                .find(|e| e.details.new_commit.as_deref() == Some(oid))
            {
                return e.clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no event for {oid}: {:#?}",
                page.events
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The stored evidence of the event of `oid`, read from the store once the engine stopped.
    fn stored_evidence(&self, repo_id: &str, oid: &str) -> (Option<String>, Option<String>) {
        let (profile, _) = Profile::open(self.dirs()).unwrap();
        let (store, _) = profile.open_store(repo_id).unwrap();
        let event = store
            .events_for_worktree(&self.wt)
            .unwrap()
            .into_iter()
            .find(|e| e.metadata.contains(oid))
            .unwrap_or_else(|| panic!("no stored event for {oid}"));
        (event.session_id, event.evidence)
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

fn claude_code_detected() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

/// With Guardrails installed, Claude Code's quick commit is attributed to its session by the hook
/// (evidence S4), and the developer's commit while that session is alive stays unattributed
/// (BR-EDGE-004).
#[test]
fn the_hook_attributes_a_quick_agent_commit_and_never_the_developers() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(true);
    let mut claude = m.launch_claude();

    let agent = m.agent_commit(&mut claude, 1);
    let e = m.event(&agent);
    assert_eq!(e.actor, claude_code_detected(), "{e:#?}");

    let human = m.human_commit(1);
    let e = m.event(&human);
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");

    let repo_id = m.repo_id();
    drop(claude);
    m.stop();
    let (session, evidence) = m.stored_evidence(&repo_id, &agent);
    assert!(session.is_some());
    assert_eq!(evidence.as_deref(), Some(S4));
    let (session, evidence) = m.stored_evidence(&repo_id, &human);
    assert_eq!(session, None);
    assert_ne!(evidence.as_deref(), Some(S4));
}

/// The developer's commit in the worktree where Claude Code's session is alive, with Guardrails
/// installed: the hook's ancestry is the developer's `git`, so there is no claim and the event
/// stays unattributed (BR-EDGE-004), never S4.
#[test]
fn a_developers_commit_next_to_a_live_agent_session_stays_unattributed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(true);
    let claude = m.launch_claude();

    let human = m.human_commit(1);
    let e = m.event(&human);
    assert_eq!(e.actor, Actor::Unattributed, "{e:#?}");

    let repo_id = m.repo_id();
    drop(claude);
    m.stop();
    let (session, evidence) = m.stored_evidence(&repo_id, &human);
    assert_eq!(session, None);
    assert_ne!(evidence.as_deref(), Some(S4));
}

/// Without Guardrails nothing changes: a quick commit of Claude Code never carries S4 (it is
/// Claude Code by S3 or unattributed, as before). Once the hooks are installed, the next one does.
#[test]
fn without_guardrails_a_quick_agent_commit_never_carries_s4() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(false);
    let mut claude = m.launch_claude();

    let unguarded = m.agent_commit(&mut claude, 1);
    let e = m.event(&unguarded);
    assert!(
        e.actor == claude_code_detected() || e.actor == Actor::Unattributed,
        "{e:#?}"
    );

    m.guard();
    let guarded = m.agent_commit(&mut claude, 2);
    let e = m.event(&guarded);
    assert_eq!(e.actor, claude_code_detected(), "{e:#?}");

    let repo_id = m.repo_id();
    drop(claude);
    m.stop();
    let (_, evidence) = m.stored_evidence(&repo_id, &unguarded);
    assert_ne!(evidence.as_deref(), Some(S4));
    let (_, evidence) = m.stored_evidence(&repo_id, &guarded);
    assert_eq!(evidence.as_deref(), Some(S4));
}

/// Without Guardrails, each quick commit of Claude Code with its only active session in the
/// worktree comes out attributed by S3 or, when S3 lost the race (`NoSighting`), with the
/// single-session hint (amendment of ADR-GRP-012), confirmed by its trailer. Never bare
/// "unattributed": the measurement of PR #155 showed it was the harness's own `git rev-parse`
/// that made S3 ambiguous.
#[test]
fn without_guardrails_every_quick_agent_commit_is_attributed_or_hinted() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new(false);
    let mut claude = m.launch_claude();
    let oids: Vec<String> = (0..10)
        .map(|n| {
            let oid = m.agent_commit(&mut claude, n);
            m.event(&oid);
            oid
        })
        .collect();

    let repo_id = m.repo_id();
    drop(claude);
    m.stop();
    for oid in &oids {
        match m.stored_evidence(&repo_id, oid) {
            (Some(_), evidence) => assert_eq!(evidence.as_deref(), Some(S3), "{oid}"),
            (None, evidence) => {
                let evidence: Value = serde_json::from_str(&evidence.unwrap_or_default())
                    .unwrap_or_else(|_| panic!("{oid}: unattributed without the hint"));
                assert_eq!(evidence["signals"], json!(["single-session"]), "{oid}");
                assert_eq!(evidence["trailer"], "confirmed", "{oid}");
            }
        }
    }
}

/// SPIKE-GRP-001: how many quick commits of Claude Code come out attributed, without and with
/// the hooks. Run by hand (`--ignored --nocapture`); it prints the counts by evidence.
#[test]
#[ignore = "measurement for SPIKE-GRP-001, run by hand"]
fn measure_quick_commit_attribution() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let runs: usize = std::env::var("RAPTOR_S4_RUNS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(20);
    for guarded in [false, true] {
        let m = Machine::new(guarded);
        let mut claude = m.launch_claude();
        let oids: Vec<String> = (0..runs)
            .map(|n| {
                let oid = m.agent_commit(&mut claude, n);
                m.event(&oid);
                oid
            })
            .collect();
        let repo_id = m.repo_id();
        drop(claude);
        m.stop();
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for oid in &oids {
            let (session, evidence) = m.stored_evidence(&repo_id, oid);
            let key = match (session, evidence) {
                (Some(_), Some(e)) => format!("attributed {e}"),
                (None, Some(e)) if e.contains("single-session") => "unattributed (hint)".into(),
                _ => "unattributed".into(),
            };
            *counts.entry(key).or_default() += 1;
        }
        println!(
            "S4 measurement · hooks: {} · quick agent commits: {runs} · {counts:?}",
            if guarded { "installed" } else { "none" }
        );
    }
}

/// TS-GRP-008 measurement (SPIKE-GRP-001): quick commits of the simulated Claude Code in the
/// main worktree while a foreign process (no session above it, like Orca's polling) runs
/// `git status` in a loop from another worktree of the same repo. Before TS-GRP-008 that foreign
/// `git` made S3 ambiguous for every commit it overlapped; with the worktree scope it no longer
/// counts. Run by hand on both sides to compare.
#[test]
#[ignore = "measurement for TS-GRP-008 and SPIKE-GRP-001, run by hand"]
fn measure_quick_commits_with_a_foreign_git_in_another_worktree() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let runs: usize = std::env::var("RAPTOR_S4_RUNS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(20);
    let m = Machine::new(false);
    let other = m.outside.path().join("other");
    let out = m.human(&format!("worktree add -q -b other '{}'", other.display()));
    assert!(out.status.success(), "{}", text(&out));
    let other = other.canonicalize().unwrap();
    // The engine knows the second worktree before any agent commit.
    let start = Instant::now();
    loop {
        let out = Command::new(RAPTOR)
            .args(["status", "--json"])
            .env_clear()
            .envs(m.env())
            .current_dir(&m.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let status: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        if status["repos"][0]["worktrees"]
            .as_array()
            .is_some_and(|w| w.len() == 2)
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{status:#?}");
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut claude = m.launch_claude();
    let stop = Arc::new(AtomicBool::new(false));
    let poller = {
        let stop = Arc::clone(&stop);
        let git = m.f.git.clone();
        let env = m.env();
        let other = other.clone();
        std::thread::spawn(move || {
            let mut n = 0usize;
            while !stop.load(Ordering::SeqCst) {
                let _ = Command::new(&git)
                    .arg("status")
                    .env_clear()
                    .envs(env.clone())
                    .current_dir(&other)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                n += 1;
            }
            n
        })
    };
    let oids: Vec<String> = (0..runs)
        .map(|n| {
            let oid = m.agent_commit(&mut claude, n);
            m.event(&oid);
            oid
        })
        .collect();
    stop.store(true, Ordering::SeqCst);
    let polls = poller.join().unwrap();
    let repo_id = m.repo_id();
    drop(claude);
    m.stop();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for oid in &oids {
        let (session, evidence) = m.stored_evidence(&repo_id, oid);
        let key = match (session, evidence) {
            (Some(_), Some(e)) => format!("attributed {e}"),
            (None, Some(e)) if e.contains("single-session") => "unattributed (hint)".into(),
            _ => "unattributed".into(),
        };
        *counts.entry(key).or_default() += 1;
    }
    // The diagnostic line of each commit (integers only, SEC-04): why S3 decided.
    let log = std::fs::read_to_string(m.dirs().state.join("daemon.log")).unwrap_or_default();
    for line in log
        .lines()
        .filter(|l| l.contains("s3_evidence") && l.contains("event=commit"))
    {
        println!(
            "{}",
            line.split_once("event=").map_or(line, |(_, rest)| rest)
        );
    }
    println!(
        "TS-GRP-008 measurement · foreign `git status` in another worktree ({polls} runs) · quick agent commits: {runs} · {counts:?}"
    );
}
