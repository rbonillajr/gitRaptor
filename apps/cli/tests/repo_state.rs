//! US-GRP-001 end to end: each Gherkin scenario with the real `raptor`
//! binary as daemon and as client, over a temporary machine built by the
//! "intact repo" harness (INF-GRP-001): temporary repo, other repo and
//! profile, never this repo nor the real profile (NFR-01).
//!
//! The developer runs reserved commands under a pty (`script`), as from
//! their own terminal. The simulated agent is a copy of this test binary
//! named `raptor-fake-agent` (see `fake_agent_entry`), declared to the
//! daemon through the debug-only `GITRAPTOR_AGENT_EXECUTABLES`.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::event::{REPO_OBSERVATION, WORKTREE_STATE};
use gitraptor_api::messages::{
    AuditListResult, AuditOutcome, ClientKind, RefusalReason, SubscribeResult, WorktreeStateData,
};
use gitraptor_api::methods;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";

/// Entry point of the simulated agent: when this test binary runs as
/// `raptor-fake-agent` with `RAPTOR_FAKE_AGENT_ARGV`, it runs that command
/// as its child and exits with its status. As a normal test it does nothing.
#[test]
fn fake_agent_entry() {
    let Some(argv) = std::env::var_os(FAKE_AGENT_ARGV) else {
        return;
    };
    let argv: Vec<String> = serde_json::from_str(argv.to_str().unwrap()).unwrap();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .env_remove(FAKE_AGENT_ARGV)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// The temporary machine plus the processes of one scenario.
struct Machine {
    f: Fixture,
}

impl Machine {
    /// The profile folders private, as the engine requires (SEC-06): part
    /// of the machine's setup, before any fingerprint.
    fn new(f: Fixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self { f }
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
        ]
    }

    /// `raptor <args>` without a terminal.
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

    /// The developer, from their own terminal (a pty, not under an agent).
    fn developer(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        let mut cmd = Command::new("/usr/bin/script");
        cmd.args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    /// `argv` run as a child of the simulated agent.
    fn as_agent(&self, argv: &[&str]) -> Output {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        Command::new(&agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(FAKE_AGENT_ARGV, serde_json::to_string(argv).unwrap())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn add(&self, path: &Path) -> Output {
        self.developer(&["repo", "add", path.to_str().unwrap()], &[])
    }

    fn retire(&self, path: &Path) -> Output {
        self.developer(&["repo", "retire", path.to_str().unwrap()], &[])
    }

    /// `raptor status --json`, parsed.
    fn status(&self) -> Value {
        let out = self.raptor(&["status", "--json"], &[]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"], &[]);
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
        // A failed test must not leave a daemon behind (it is detached).
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

fn canonical(path: &Path) -> String {
    path.canonicalize().unwrap().to_str().unwrap().to_owned()
}

/// The observed repos of a `status --json`, by folder.
fn repo_paths(status: &Value) -> Vec<String> {
    status["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap().to_owned())
        .collect()
}

/// "demo": `main` with `login.txt` committed, and a linked worktree
/// "feat-login" on branch "feat-login".
fn demo() -> (Machine, PathBuf) {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    let wt = f.add_worktree("feat-login", "feat-login");
    (Machine::new(f), wt)
}

// ------------------------------------------------------------ Escenario 1

/// El estado de cada worktree queda disponible al añadir el repo.
#[test]
fn the_state_of_each_worktree_is_available_once_the_repo_is_added() {
    let (m, wt) = demo();
    std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();

    // A subscriber sees the add on the stream, with the change timings.
    assert!(m.raptor(&["daemon", "status"], &[]).status.success());
    let mut watcher = m.client();
    let _: SubscribeResult = watcher.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();

    let out = m.add(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("observing"), "{}", text(&out));

    let status = m.status();
    assert_eq!(status["engine"], "observing");
    let repos = status["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1, "{status}");
    let wts = repos[0]["worktrees"].as_array().unwrap();
    assert_eq!(wts.len(), 2, "{status}");

    let main = &wts[0];
    assert_eq!(main["main"], true);
    assert_eq!(main["path"], canonical(&m.f.repo));
    assert_eq!(main["branch"], "main");
    assert_eq!(main["clean"], true, "{main}");

    let feat = &wts[1];
    assert_eq!(feat["main"], false);
    assert_eq!(feat["path"], canonical(&wt));
    assert_eq!(feat["branch"], "feat-login");
    assert_eq!(feat["clean"], false);
    assert_eq!(
        feat["changes"],
        json!([{"path": "login.txt", "area": "unstaged", "kind": "modified"}])
    );

    // The text output says the same.
    let out = m.raptor(&["status"], &[]);
    let shown = String::from_utf8_lossy(&out.stdout);
    assert!(
        shown.contains("branch main") && shown.contains("no changes"),
        "{shown}"
    );
    assert!(shown.contains("branch feat-login"), "{shown}");
    assert!(shown.contains("not staged modified login.txt"), "{shown}");

    let mut kinds = Vec::new();
    let mut state = None;
    while let Some(n) = watcher
        .next_notification(Duration::from_millis(500))
        .unwrap()
    {
        let event = &n.params["event"];
        kinds.push(event["kind"].as_str().unwrap().to_owned());
        if event["kind"] == WORKTREE_STATE {
            assert!(event["timings"].is_object(), "{event}");
            state =
                Some(serde_json::from_value::<WorktreeStateData>(event["data"].clone()).unwrap());
        }
    }
    assert!(kinds.iter().any(|k| k == REPO_OBSERVATION), "{kinds:?}");
    assert_eq!(state.unwrap().worktrees.len(), 2);
    drop(watcher);
    m.stop();
}

// ------------------------------------------------------------ Escenario 2

/// Solo se observan los repos añadidos.
#[test]
fn only_the_added_repos_are_observed() {
    let m = Machine::new(Fixture::with_commit(&git_from_path()));
    assert!(m.add(&m.f.repo).status.success());

    let status = m.status();
    assert_eq!(repo_paths(&status), [canonical(&m.f.repo)]);
    assert!(!status.to_string().contains("other-repo"), "{status}");

    let out = m.retire(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("no longer observing"), "{}", text(&out));
    let status = m.status();
    assert!(repo_paths(&status).is_empty(), "{status}");
    assert_eq!(status["engine"], "no-repos");
    m.stop();
}

// ------------------------------------------------------------ Escenario 3

/// Un agente no puede añadir ni retirar repos.
#[test]
fn an_agent_cannot_add_or_retire_repos() {
    let m = Machine::new(Fixture::with_commit(&git_from_path()));
    assert!(m.add(&m.f.repo).status.success());
    let before = m.status();

    let other = m.f.other_repo.to_str().unwrap().to_owned();
    let repo = m.f.repo.to_str().unwrap().to_owned();
    let attempts = [
        m.as_agent(&[RAPTOR, "repo", "add", &other]),
        m.as_agent(&[RAPTOR, "repo", "retire", &repo]),
        // Under a pty of its own: still the agent.
        m.as_agent(&[
            "/usr/bin/script",
            "-q",
            "/dev/null",
            RAPTOR,
            "repo",
            "add",
            &other,
        ]),
    ];
    for out in &attempts {
        assert!(!out.status.success(), "{}", text(out));
        assert!(
            text(out).contains("only the developer can change the observed repos"),
            "{}",
            text(out)
        );
    }
    let after = m.status();
    assert_eq!(repo_paths(&after), repo_paths(&before));
    assert_eq!(repo_paths(&after), [canonical(&m.f.repo)]);

    let mut client = m.client();
    let audit: AuditListResult = client.call(methods::AUDIT_LIST, json!({})).unwrap();
    let refused: Vec<_> = audit
        .entries
        .iter()
        .filter(|e| e.outcome == AuditOutcome::Rejected)
        .collect();
    assert_eq!(refused.len(), 3, "{:?}", audit.entries);
    for entry in refused {
        assert!(entry.operation.starts_with("repo."));
        assert_eq!(entry.reason, Some(RefusalReason::AgentAncestry));
    }
    drop(client);
    m.stop();
}

// ------------------------------------------------------------ Escenario 4

/// Un directorio que no es un repo Git no se añade.
#[test]
fn a_folder_that_is_not_a_git_repo_is_not_added() {
    let m = Machine::new(Fixture::new(&git_from_path()));
    let notas = m.f.root.join("notas");
    std::fs::create_dir(&notas).unwrap();
    std::fs::write(notas.join("ideas.md"), "x\n").unwrap();

    let out = m.add(&notas);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("notas is not a Git repository"),
        "{}",
        text(&out)
    );
    // The same, in Spanish.
    let out = m.developer(
        &["repo", "add", notas.to_str().unwrap()],
        &[("LANG", "es_ES.UTF-8")],
    );
    assert!(!out.status.success());
    assert!(
        text(&out).contains("notas no es un repo Git"),
        "{}",
        text(&out)
    );

    let status = m.status();
    assert!(repo_paths(&status).is_empty(), "{status}");
    assert_eq!(status["engine"], "no-repos");
    m.stop();
}

// ------------------------------------------------------------ Escenario 5

/// The engine's own data, the only difference this scenario may show
/// outside the repo: its profile data and state, and the socket folder.
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

/// Observar el repo no lo modifica.
#[test]
fn repo_intact_observing_the_repo_does_not_modify_it() {
    let f = Fixture::with_commit(&git_from_path());
    // A stash, a known remote branch, a linked worktree and a hook of the
    // developer's own.
    f.write("a.txt", "stashed\n");
    f.git(&["stash", "-q"]);
    f.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    f.git(&["branch", "feat"]);
    f.add_worktree("feat", "feat");
    let hook = f.repo.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Modified, staged, untracked, and a tracked file whose stat is dirty
    // but whose content is not: a status that refreshed the index would
    // rewrite it.
    f.write("a.txt", "alpha changed\n");
    f.write("staged.txt", "staged\n");
    f.git(&["add", "staged.txt"]);
    f.write("untracked.txt", "u\n");
    f.dirty_stat("b.txt");
    let m = Machine::new(f);

    let report = check("US-GRP-001 observe", &m.f, &engine_profile(), || {
        let out = m.add(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        let status = m.status();
        let wts = &status["repos"][0]["worktrees"];
        assert_eq!(wts.as_array().unwrap().len(), 2, "{status}");
        assert_eq!(
            wts[0]["counts"],
            json!({"staged": 1, "unstaged": 1, "untracked": 1}),
            "{status}"
        );
        assert!(m.retire(&m.f.repo).status.success());
        m.stop();
    });
    report.assert_intact();
}

// ------------------------------------------------------------ Escenario 6

/// Preparar todos los cambios no recoge nada del motor.
#[test]
fn staging_everything_picks_up_nothing_from_the_engine() {
    let (m, _) = demo();
    assert!(m.add(&m.f.repo).status.success());
    m.f.write("login.txt", "user\nchanged\n");
    // The engine is running and has observed the repo.
    assert_eq!(repo_paths(&m.status()), [canonical(&m.f.repo)]);

    m.f.git(&["add", "-A"]);
    let staged = m.f.git(&["diff", "--cached", "--name-only"]);
    assert_eq!(staged.lines().collect::<Vec<_>>(), ["login.txt"]);
    m.stop();
}
