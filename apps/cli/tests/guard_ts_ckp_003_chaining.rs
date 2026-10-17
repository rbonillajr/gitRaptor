//! TS-CKP-003, regression of INF-GRD-001 with the executor as parent: for every prior hook setup
//! (`PriorHooks::ALL`: none, own, husky, lefthook, pre-commit, global `core.hooksPath`) the
//! `commit` of the debug test arm, run by the simulated agent, behaves as raw Git does with the
//! linter's marker: it fails the same, and without the marker it goes in. The real `raptor` daemon
//! with `GITRAPTOR_TEST_EXECUTOR_ARMS=1` over a temporary profile (NFR-01), the real `raptor-hook`
//! dispatcher and plain Git; only JSON-RPC and the CLI. No fixed waits: every child has a deadline.
//!
//! macOS only. Linux and Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{clone_binary, git_from_path};
use gitraptor_testkit::hooks::{LINT_MARKER, PriorHooks};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const RPC_ENV: &str = "RAPTOR_TEST_RPC";
const DEADLINE: Duration = Duration::from_secs(120);
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";

/// The simulated agent: runs `RAPTOR_FAKE_AGENT_ARGV` as its child.
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

/// The client: `{"root", "run": <prepare params>}`; prepares the plan, runs it and prints
/// `{"prepare", "run"}` as one JSON line.
#[test]
fn rpc_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let answer = |r: Result<Value, ClientError>| match r {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let prepared = answer(client.call(methods::OPERATION_PREPARE, spec["run"].clone()));
    let ran = match prepared.get("ok") {
        Some(plan) => answer(client.call(
            methods::OPERATION_RUN,
            json!({ "plan_id": plan["plan_id"], "accepted_warnings": plan["warnings"] }),
        )),
        None => Value::Null,
    };
    println!("RPC-ANSWERS {}", json!({ "prepare": prepared, "run": ran }));
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// Runs `cmd` to the end within [`DEADLINE`]; never waits without a bound.
fn run_within(mut cmd: Command, what: &str) -> Output {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(DEADLINE) {
        Ok(out) => out.unwrap(),
        Err(_) => {
            let _ = Command::new("/bin/kill")
                .args(["-9", &pid.to_string()])
                .status();
            panic!("{what} did not finish within {DEADLINE:?}");
        }
    }
}

struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
}

impl Machine {
    /// A repo with its prior hooks, observed, enabled for the MCP and protected.
    fn new(f: Fixture) -> Self {
        #[cfg(not(debug_assertions))]
        panic!(
            "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
        );
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self { f, outside };
        let repo = m.f.repo.to_str().unwrap().to_owned();
        for args in [
            vec!["repo", "add", &repo],
            vec!["mcp", "enable", &repo],
            vec!["guard", "install", "--yes", &repo],
        ] {
            let mut cmd = Command::new("/usr/bin/script");
            cmd.args(["-q", "/dev/null", RAPTOR])
                .args(&args)
                .env_clear()
                .envs(m.env())
                .current_dir(&m.f.root);
            let out = run_within(cmd, "raptor");
            assert!(out.status.success(), "{args:?}: {}", text(&out));
        }
        m
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_EXECUTOR_ARMS", "1".into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            ("PATH", "/usr/bin:/bin".into()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// The `commit` arm of the test catalog, asked by the simulated agent in the repo.
    fn agent_commit(&self, what: &str) -> Value {
        let me = std::env::current_exe().unwrap();
        let client_argv = [
            me.to_string_lossy().into_owned(),
            "rpc_client_entry".into(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ];
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            clone_binary(&me, &agent);
        }
        let message = format!("feat: {what}\n\n{CLAUDE}\n");
        let spec = json!({ "root": self.f.profile, "run": {
            "operation": "commit", "worktree": self.f.repo,
            "args": { "message": message }, "surface": "tui",
        }});
        let mut cmd = Command::new(agent);
        cmd.args([
            "fake_agent_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .envs(self.env())
        .env(
            FAKE_AGENT_ARGV,
            serde_json::to_string(&client_argv).unwrap(),
        )
        .env(RPC_ENV, spec.to_string())
        .current_dir(&self.f.root);
        let out = run_within(cmd, "agent client");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stdout
            .lines()
            .find_map(|l| {
                l.split_once("RPC-ANSWERS ")
                    .map(|(_, j)| j.trim().to_owned())
            })
            .unwrap_or_else(|| panic!("no answers: {} {}", stdout, text(&out)));
        serde_json::from_str(&line).unwrap()
    }

    fn head(&self) -> String {
        self.f.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let dirs = ProfileDirs::under_root(&self.f.profile);
        if let Ok(Some(pid)) = running_pid(&dirs.state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// INF-GRD-001 with the executor as parent: the prior hook of every setup rejects the marker
/// under the executor exactly as it does under raw Git, and a commit without it goes in.
#[test]
fn chained_prior_hooks_behave_the_same_under_the_executor() {
    for prior in PriorHooks::ALL {
        let f = Fixture::with_prior_hooks(&git_from_path(), prior);
        // Raw Git, before Guardrails: the linter's verdict for this setup.
        let raw_rejected = f.commit_with_marker_rejected(&f.repo);
        assert_eq!(raw_rejected, prior.has_linter(), "raw Git under {prior:?}");
        f.git(&["reset", "-q"]);
        let _ = std::fs::remove_file(f.repo.join("lint-target.txt"));
        let m = Machine::new(f);
        let tip = m.head();

        // With the marker: the executor's commit fails like raw Git did.
        m.f.write("lint-target.txt", &format!("{LINT_MARKER}\n"));
        m.f.git(&["add", "lint-target.txt"]);
        let marked = m.agent_commit("marker");
        assert!(
            marked["prepare"]["ok"].is_object(),
            "{prior:?}: the plan was not prepared: {marked:#}"
        );
        let went_in = marked["run"]["ok"]["outcome"] == "done";
        assert_eq!(went_in, !raw_rejected, "{prior:?}: {marked:#}");
        assert_eq!(m.head() != tip, !raw_rejected, "{prior:?}: {marked:#}");

        // Without it: it goes in.
        m.f.git(&["reset", "-q"]);
        let _ = std::fs::remove_file(m.f.repo.join("lint-target.txt"));
        let tip = m.head();
        m.f.write("clean.txt", "clean\n");
        m.f.git(&["add", "clean.txt"]);
        let clean = m.agent_commit("clean");
        assert_eq!(
            clean["run"]["ok"]["outcome"], "done",
            "{prior:?}: {clean:#}"
        );
        assert_ne!(
            m.head(),
            tip,
            "{prior:?}: the commit did not move the branch"
        );
    }
}
