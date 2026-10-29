//! US-TMC-012 end to end: a later event of the same agent session is not another actor, so `raptor
//! undo` raises no overlap for it. The real `raptor` binary as daemon and as client over a
//! temporary machine (temporary repo, home and profile). Claude Code is simulated as in
//! `undo_overlap_process.rs` (a copy of this test binary named `raptor-fake-agent`). The harness is
//! a trimmed copy of that file's. No fixed waits: each state is awaited with a deadline.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{
    ClientKind, EventsHistoryResult, GitEventKind, GitEventView, Snapshot,
};
use gitraptor_api::{Actor, AgentKind, PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{Oplog, SnapshotFilter};
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::json;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
const DONE: &str = "<<raptor-fake-claude-done>>";
const DEADLINE: Duration = Duration::from_secs(20);

/// Entry point of the simulated Claude Code (see the module docs). As a
/// normal test it does nothing.
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
                "fake claude died"
            );
            if let Some(ok) = line.split_once(DONE).map(|(_, ok)| ok) {
                return ok.trim() == "true";
            }
        }
    }
}

impl Drop for FakeClaude {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The temporary machine, plus what lives outside the fixture's root: the
/// folder Claude Code is installed in, its hooks and its output files.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    worktree: PathBuf,
}

impl Machine {
    /// "demo": `main` with `login.txt`, branch `feat-login` and its worktree
    /// "feat-login" (`<root>/wt-feat-login`), observed.
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("api.rs", "fn login() {}\n");
        f.write("other.rs", "fn other() {}\n");
        f.git(&["add", "api.rs", "other.rs"]);
        f.git(&["commit", "-q", "-m", "api"]);
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
        // hooks are never touched.
        let hooks = outside.path().join("slow-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let hook = hooks.join("reference-transaction");
        std::fs::write(
            &hook,
            "#!/bin/sh\n[ \"$1\" = committed ] && sleep 0.5\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let m = Self {
            f,
            outside,
            worktree,
        };
        let out = m.developer(
            &m.f.root,
            &["repo", "add", m.f.repo.to_str().unwrap()],
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
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", lang.into()),
        ]
    }

    /// The developer, from their own terminal in `cwd`.
    fn developer(&self, cwd: &Path, args: &[&str], lang: &str) -> Output {
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

    /// The developer launches Claude Code in the worktree.
    fn launch_claude(&self) -> FakeClaude {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
        let mut child = Command::new(agent)
            .args([
                "fake_claude_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env("en_US.UTF-8"))
            .env(FAKE_CLAUDE, "1")
            .env("HOME", &self.f.home)
            .current_dir(&self.worktree)
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

    /// Waits until the history has an event of `kind`, in any worktree.
    fn event(&self, kind: GitEventKind) -> GitEventView {
        let start = Instant::now();
        loop {
            let page: EventsHistoryResult = self
                .client()
                .call(methods::EVENTS_HISTORY, json!({"repo_id": self.repo_id()}))
                .unwrap();
            if let Some(e) = page.events.iter().find(|e| e.kind == kind) {
                return e.clone();
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no {kind:?}: {:#?}",
                page.events
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for a snapshot of the worktree whose `path` holds `content`.
    fn captured(&self, path: &str, content: &[u8]) -> String {
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &repo_id) {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    let found = store
                        .files(id, "wt-wt-feat-login")
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
                        .any(|(p, _, oid)| p == path && store.read_blob(oid).unwrap() == content);
                    if found {
                        return id.clone();
                    }
                }
            }
            assert!(start.elapsed() < DEADLINE, "{path} never captured");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for the snapshot the engine recorded as caused by git event `seq`.
    fn captured_by_event(&self, seq: i64) {
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            if let Ok((oplog, _)) = Oplog::open(&self.dirs(), &repo_id, 1)
                && let Ok(snaps) = oplog.snapshots(&SnapshotFilter::default())
                && snaps.iter().any(|s| s.record.cause_event_seq == Some(seq))
            {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no snapshot caused by event {seq}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&self.f.root, &["daemon", "stop", "--yes"], "en_US.UTF-8");
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

const WORK: &[u8] = b"fn login() { agents_work(); }\n";

/// The agent writes uncommitted work to `api.rs`, the Time Machine captures it and the agent
/// throws it away with a raw `git reset --hard` the engine attributes to its session.
fn agent_resets(m: &Machine, claude: &mut FakeClaude) {
    assert!(claude.run("printf 'fn login() { agents_work(); }\\n' > api.rs"));
    m.captured("api.rs", WORK);
    let hooks = m.outside.path().join("slow-hooks");
    assert!(claude.run(&format!(
        "git -c core.hooksPath={} reset -q --hard",
        hooks.display()
    )));
    let reset = m.event(GitEventKind::Reset);
    assert!(
        matches!(
            reset.actor,
            Actor::Agent {
                kind: AgentKind::ClaudeCode,
                ..
            }
        ),
        "{reset:#?}"
    );
    assert_eq!(
        std::fs::read(m.worktree.join("api.rs")).unwrap(),
        b"fn login() {}\n"
    );
    // The reset's own capture (base B) must exist before any later edit (spec G4: an edit inside
    // the quiet window lands in B).
    m.captured_by_event(reset.seq);
}

/// What the agent's `raptor undo <args>` printed, and whether it succeeded.
fn agent_undo(m: &Machine, claude: &mut FakeClaude, env: &str, args: &str) -> (bool, String) {
    let out = m
        .outside
        .path()
        .join(format!("undo-{}.txt", env.len() + args.len()));
    let ok = claude.run(&format!(
        "{env} {RAPTOR} undo {args} > {} 2>&1",
        out.display()
    ));
    (ok, std::fs::read_to_string(&out).unwrap_or_default())
}

/// D4: a later event of the same agent session on a branch of the scope is not another actor.
/// The newest event of the worktree's stack (the branch move) is undone first; the reset behind it
/// is then undone without an overlap rejection.
#[test]
fn the_same_agent_session_is_not_another_actor() {
    let m = Machine::new();
    // `main` moves ahead, so a branch created there differs from the worktree's HEAD.
    m.f.git(&["commit", "-q", "--allow-empty", "-m", "ahead"]);
    let mut claude = m.launch_claude();
    let hooks = m.outside.path().join("slow-hooks");
    // The branch exists (created by the session, observed) before the session moves it.
    assert!(claude.run(&format!(
        "git -c core.hooksPath={} branch feat-pagos main",
        hooks.display()
    )));
    m.event(GitEventKind::BranchCreate);
    agent_resets(&m, &mut claude);
    // The same session moves a branch with Git, after the reset.
    assert!(claude.run(&format!(
        "git -c core.hooksPath={} branch -f feat-pagos HEAD",
        hooks.display()
    )));
    let moved = m.event(GitEventKind::BranchUpdate);
    assert!(
        matches!(moved.actor, Actor::Agent { .. }),
        "the move is attributed to the agent's session: {moved:#?}"
    );

    // The branch move is the newest event: it goes first.
    let (ok, said) = agent_undo(&m, &mut claude, "LANG=en_US.UTF-8", "");
    assert!(ok, "undoing the branch move: {said}");
    // Then the reset: the move is the same session's, not another actor's.
    // (`X=2` only makes the output file name differ from the first call's.)
    let (ok, said) = agent_undo(&m, &mut claude, "LANG=en_US.UTF-8 X=2", "");

    assert!(ok, "no overlap for the agent's own event: {said}");
    assert_eq!(std::fs::read(m.worktree.join("api.rs")).unwrap(), WORK);
    drop(claude);
    m.stop();
}
