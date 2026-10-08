//! US-GRP-020 end to end: the code folders and the discovered repos through the real `raptor`
//! binary, as daemon and as client, over a temporary profile (`GITRAPTOR_PROFILE_DIR`, debug
//! builds only) and temporary folders. Never this repo nor the real profile (NFR-01).
//!
//! The developer is `raptor` under a pty (`script`); an agent is a copy of this test binary
//! named `raptor-fake-agent` that runs the command as its descendant (see `fake_agent_entry`),
//! declared to the daemon through the debug-only `GITRAPTOR_AGENT_EXECUTABLES`. The daemon
//! takes `GITRAPTOR_TEST_DISCOVERY_HOME` as the user's home and lists every root every 100 ms
//! (`GITRAPTOR_TEST_DISCOVERY_POLL_MS`).
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(all(target_os = "macos", debug_assertions))]

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const DEADLINE: Duration = Duration::from_secs(30);

/// One scenario at a time: each runs its own engine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Entry point of the simulated agent: when this test binary runs as `raptor-fake-agent` with
/// `RAPTOR_FAKE_AGENT_ARGV`, it runs that command as its child and exits with its status. As
/// a normal test it does nothing.
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

/// The machine: a temporary profile, a fake home and a `code` folder with two repos, plus
/// things that must not be found there.
struct Machine {
    f: Fixture,
    code: PathBuf,
}

impl Machine {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let code = f.root.join("code");
        std::fs::create_dir_all(&code).unwrap();
        let m = Self { f, code };
        m.repo(&m.code.join("alpha"));
        m.repo(&m.code.join("beta"));
        m
    }

    /// A repo with one commit at `dir`.
    fn repo(&self, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        self.f.git_in(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("README.md"), "x\n").unwrap();
        self.f.git_in(dir, &["add", "README.md"]);
        self.f.git_in(dir, &["commit", "-q", "-m", "x"]);
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            (
                "GITRAPTOR_TEST_DISCOVERY_HOME",
                self.f.home.clone().into_os_string(),
            ),
            ("GITRAPTOR_TEST_DISCOVERY_POLL_MS", "100".into()),
            ("GITRAPTOR_LANG", "en".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("TERM", "xterm-256color".into()),
        ]
    }

    /// A read-only command, without a terminal.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A reserved command from the developer's own terminal: a pty, no agent above it.
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

    /// The developer's terminal exists, but `raptor` reads no answer from it: its standard
    /// input is `/dev/null`.
    fn developer_without_stdin_terminal(&self, args: &[&str]) -> Output {
        Command::new("/usr/bin/script")
            .args([
                "-q",
                "/dev/null",
                "/bin/sh",
                "-c",
                "exec \"$0\" \"$@\" </dev/null",
                RAPTOR,
            ])
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The developer types `answer` and Enter at the question of `raptor`.
    fn developer_answering(&self, args: &[&str], answer: &str) -> Output {
        let mut child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR])
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Kept open until the end: an end of input would close the terminal.
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, "{answer}").unwrap();
        stdin.flush().unwrap();
        let out = child.wait_with_output().unwrap();
        drop(stdin);
        out
    }

    /// `raptor` as a descendant of the simulated agent, with a terminal in between: only the
    /// agent ancestry can refuse it.
    fn agent(&self, args: &[&str]) -> Output {
        let agent = self.f.root.join(FAKE_AGENT);
        if !agent.exists() {
            std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        }
        let mut argv = vec!["/usr/bin/script", "-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new(&agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(FAKE_AGENT_ARGV, serde_json::to_string(&argv).unwrap())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The folders of the observed repos, from `raptor status --json`.
    fn observed(&self) -> Vec<String> {
        let out = self.run(&["status", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let status: Value = serde_json::from_slice(&out.stdout).unwrap();
        status["repos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["path"].as_str().unwrap().to_owned())
            .collect()
    }

    fn declare(&self, root: &Path) -> Output {
        let out = self.developer(&["repo", "roots", "add", root.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    fn roots(&self) -> String {
        let out = self.run(&["repo", "roots"]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }

    fn discovered(&self) -> String {
        let out = self.run(&["repo", "discovered"]);
        assert!(out.status.success(), "{}", text(&out));
        text(&out)
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // A daemon started on demand is detached: never leave it behind.
        let dirs = ProfileDirs::under_root(&self.f.profile);
        if let Ok(Some(pid)) = running_pid(&dirs.state) {
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

/// Polls `condition` until it holds, with a deadline: no fixed waits.
fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(25));
    }
}

const EMPTY_ROOTS: &str = "no code folders declared";

/// A1, A3, A6: declaring a normal folder lists its first-level repos, announces how many and
/// observes none; hidden folders, symbolic links and repos deeper down are not candidates.
#[test]
fn declaring_a_folder_lists_its_repos_without_observing_them() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    m.repo(&m.code.join(".dotted"));
    m.repo(&m.code.join("group").join("inner"));
    std::os::unix::fs::symlink(&m.f.other_repo, m.code.join("linked")).unwrap();

    let out = m.declare(&m.code);
    let said = text(&out);
    assert!(said.contains("2 repos discovered"), "{said}");
    assert!(said.contains("raptor repo discovered"), "{said}");

    let listed = m.discovered();
    for name in ["alpha", "beta"] {
        assert!(listed.contains(&format!("Observe {name}?")), "{listed}");
        assert!(listed.contains("raptor repo add"), "{listed}");
        assert!(listed.contains("raptor repo dismiss"), "{listed}");
    }
    for not in [".dotted", "inner", "linked"] {
        assert!(!listed.contains(not), "{not}: {listed}");
    }
    assert!(m.roots().contains("code"), "{}", m.roots());
    assert!(m.observed().is_empty(), "{:?}", m.observed());
}

/// A4: a folder that is a repo, a path that does not exist and a symbolic link are rejected
/// with what to do instead, and the list of roots stays empty.
#[test]
fn rejected_roots_are_not_declared() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    let link = m.f.root.join("code-link");
    std::os::unix::fs::symlink(&m.code, &link).unwrap();
    let missing = m.f.root.join("nope");
    for (path, said) in [
        (m.f.repo.clone(), "is a repo or inside one"),
        (missing, "does not exist"),
        (link, "symbolic link"),
    ] {
        let out = m.developer(&["repo", "roots", "add", path.to_str().unwrap()]);
        assert!(!out.status.success(), "{path:?}: {}", text(&out));
        let said_text = text(&out);
        assert!(said_text.contains(said), "{path:?}: {said_text}");
        if said == "symbolic link" {
            assert!(
                said_text.contains(m.code.to_str().unwrap()),
                "suggests the real path: {said_text}"
            );
        }
    }
    assert!(m.roots().contains(EMPTY_ROOTS), "{}", m.roots());
}

/// A5: a broad folder (here the home) is declared only when the developer confirms its cost
/// on their terminal; without a terminal to ask, or answering no, nothing is declared.
#[test]
fn a_broad_folder_needs_confirmation_on_the_terminal() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    let home = m.f.home.to_str().unwrap();

    let out = m.developer_without_stdin_terminal(&["repo", "roots", "add", home]);
    assert!(!out.status.success(), "{}", text(&out));
    let said = text(&out);
    assert!(said.contains("is a broad folder"), "{said}");
    assert!(said.contains("every 60 s"), "{said}");
    assert!(said.contains("home folder"), "{said}");
    assert!(m.roots().contains(EMPTY_ROOTS), "{}", m.roots());

    let out = m.developer_answering(&["repo", "roots", "add", home], "n");
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("[y/N]"), "{}", text(&out));
    assert!(m.roots().contains(EMPTY_ROOTS), "{}", m.roots());

    let out = m.developer_answering(&["repo", "roots", "add", home], "s");
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("watching the code folder"),
        "{}",
        text(&out)
    );
    let roots = m.roots();
    assert!(roots.contains(home) && roots.contains("broad"), "{roots}");
}

/// A8: dismissing removes the candidate for good; a path that is not a candidate is told so.
#[test]
fn dismissing_a_candidate_removes_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    m.declare(&m.code);
    let alpha = m.code.join("alpha");
    let out = m.developer(&["repo", "dismiss", alpha.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("dismissed"), "{}", text(&out));
    let listed = m.discovered();
    assert!(!listed.contains("alpha"), "{listed}");
    assert!(listed.contains("beta"), "{listed}");

    let out = m.developer(&["repo", "dismiss", alpha.to_str().unwrap()]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("not waiting for a decision"),
        "{}",
        text(&out)
    );
    // A dismissed repo can still be observed by hand.
    let out = m.developer(&["repo", "add", alpha.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(m.observed().len(), 1, "{:?}", m.observed());
}

/// A8: `repo add` of a candidate observes it and it leaves the list; removing the root drops
/// the pending candidates and keeps what is observed.
#[test]
fn accepting_observes_and_removing_the_root_keeps_the_observed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    m.declare(&m.code);
    let beta = m.code.join("beta");
    let out = m.developer(&["repo", "add", beta.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(m.observed().iter().any(|p| p.ends_with("/beta")));
    let listed = m.discovered();
    assert!(!listed.contains("beta"), "{listed}");
    assert!(listed.contains("alpha"), "{listed}");

    let out = m.developer(&["repo", "roots", "remove", m.code.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("no longer watching"), "{}", text(&out));
    assert!(m.roots().contains(EMPTY_ROOTS), "{}", m.roots());
    assert!(m.discovered().contains("no discovered repos"));
    assert!(m.observed().iter().any(|p| p.ends_with("/beta")));

    let out = m.developer(&["repo", "roots", "remove", m.code.to_str().unwrap()]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("not a declared code folder"),
        "{}",
        text(&out)
    );
}

/// US-GRP-022: a candidate deleted before it is accepted is refused with the reason and
/// stops being proposed.
#[test]
fn a_deleted_candidate_cannot_be_accepted() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    m.declare(&m.code);
    let beta = m.code.join("beta");
    std::fs::remove_dir_all(&beta).unwrap();
    let out = m.developer(&["repo", "add", beta.to_str().unwrap()]);
    assert!(!out.status.success(), "{}", text(&out));
    eventually("beta to leave the candidates", || {
        !m.discovered().contains("beta")
    });
    assert!(m.observed().is_empty(), "{:?}", m.observed());
}

/// A repo that appears later in a declared folder is found without anyone asking again.
#[test]
fn a_repo_that_appears_later_is_found_and_not_observed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    m.declare(&m.code);
    m.repo(&m.code.join("gamma"));
    eventually("gamma to be discovered", || {
        m.discovered().contains("gamma")
    });
    assert!(m.observed().is_empty(), "{:?}", m.observed());
}

/// A7: an agent, with a terminal in between, can neither declare nor remove a root nor
/// dismiss a candidate: the engine refuses it and nothing changes.
#[test]
fn an_agent_cannot_change_the_roots_nor_dismiss() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let m = Machine::new();
    let out = m.agent(&["repo", "roots", "add", m.code.to_str().unwrap()]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("only the developer"), "{}", text(&out));
    assert!(m.roots().contains(EMPTY_ROOTS), "{}", m.roots());

    m.declare(&m.code);
    let alpha = m.code.join("alpha");
    for args in [
        vec!["repo", "dismiss", alpha.to_str().unwrap()],
        vec!["repo", "roots", "remove", m.code.to_str().unwrap()],
    ] {
        let out = m.agent(&args);
        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(text(&out).contains("only the developer"), "{}", text(&out));
    }
    assert!(m.roots().contains("code"), "{}", m.roots());
    assert!(m.discovered().contains("alpha"));
}
