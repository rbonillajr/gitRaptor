//! US-MCP-023 (DS-US-MCP-023 § 9.4, T008): the JSON contract of `raptor --json` for agents. The
//! real `raptor` binary over a temporary machine: temporary repos and profile, never this repo
//! nor the real profile (NFR-01).
//!
//! The simulated agent is a copy of this test binary named `raptor-fake-agent` that runs each
//! line of its input with `/bin/sh`, so its `raptor` descends from it (as in `raw_git_undo.rs`).
//! The developer runs under a pty (`script`), as from their own terminal, or plain when the
//! test needs stdout and stderr apart.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows: Pendiente: etapa de
//! validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, Snapshot};
use gitraptor_api::methods::McpStatus;
use gitraptor_api::timemachine::{McpRedoResult, McpUndoResult};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_CLAUDE: &str = "RAPTOR_FAKE_CLAUDE";
const DONE: &str = "<<raptor-fake-claude-done>>";
const DEADLINE: Duration = Duration::from_secs(20);
const WORK: &[u8] = b"user\npassword\n";

/// Entry point of the simulated Claude Code. As a normal test it does nothing.
#[test]
fn fake_claude_entry() {
    if std::env::var_os(FAKE_CLAUDE).is_none() {
        return;
    }
    for line in std::io::stdin().lock().lines() {
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(line.unwrap())
            .status();
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

/// What a command printed: its exit code, stdout and stderr apart.
struct Cap {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Cap {
    fn of(out: &Output) -> Self {
        Self {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// Stdout is exactly one JSON line (compact), stderr is empty; returns the parsed envelope.
    fn envelope(&self, what: &str) -> Value {
        assert!(
            self.stderr.is_empty(),
            "{what}: nothing may go to stderr with --json: {:?}",
            self.stderr
        );
        let body = self.stdout.strip_suffix('\n').unwrap_or(&self.stdout);
        assert!(
            !body.is_empty() && !body.contains('\n'),
            "{what}: stdout must be exactly one line of JSON, got {:?}",
            self.stdout
        );
        let value: Value = serde_json::from_str(body)
            .unwrap_or_else(|e| panic!("{what}: stdout is not JSON ({e}): {:?}", self.stdout));
        assert_eq!(
            value["schema_version"], "1.0",
            "{what}: the output must declare schema_version 1.0: {value}"
        );
        value
    }

    /// A refusal: exit code `code`, one JSON line with `error.{code,message,params,action}`.
    fn refusal(&self, what: &str, expected: &str, exit: i32) -> Value {
        let value = self.envelope(what);
        assert_eq!(self.code, exit, "{what}: exit code: {value}");
        assert!(value.get("data").is_none(), "{what}: no data: {value}");
        let error = &value["error"];
        assert_eq!(error["code"], expected, "{what}: {value}");
        for field in ["message", "params", "action"] {
            assert!(
                error.get(field).is_some(),
                "{what}: error.{field} is part of the contract: {value}"
            );
        }
        error.clone()
    }

    /// A success: exit 0, one JSON line with `data`; returns `data`.
    fn data(&self, what: &str) -> Value {
        let value = self.envelope(what);
        assert_eq!(self.code, 0, "{what}: exit code: {value}");
        assert!(value.get("error").is_none(), "{what}: no error: {value}");
        assert!(
            value.get("data").is_some(),
            "{what}: the output carries data: {value}"
        );
        value["data"].clone()
    }
}

struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    worktree: PathBuf,
}

impl Machine {
    /// A repo with a worktree `wt-<name>` on `branch`, observed and enabled for the MCP
    /// (or only observed when `enable` is false).
    fn build(name: &str, branch: &str, enable: bool) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", branch]);
        f.git(&["branch", "victim"]);
        let worktree = f.add_worktree(name, branch).canonicalize().unwrap();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
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
        let repo = m.f.repo.to_str().unwrap().to_owned();
        m.ok(&m.f.root, &["repo", "add", &repo]);
        if enable {
            m.ok(&m.f.root, &["mcp", "enable", &repo]);
        }
        m
    }

    fn new() -> Self {
        Self::build("feat-login", "feat-login", true)
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
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// The developer, from their own terminal (a pty); stdout and stderr come merged.
    fn developer(&self, cwd: &Path, args: &[&str]) -> Output {
        let mut argv = vec!["-q", "/dev/null", RAPTOR];
        argv.extend_from_slice(args);
        Command::new("/usr/bin/script")
            .args(argv)
            .env_clear()
            .envs(self.env())
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, cwd: &Path, args: &[&str]) {
        let out = self.developer(cwd, args);
        assert!(out.status.success(), "{args:?}: {:?}", out);
    }

    /// The developer's JSON from a pty: the first object of the output.
    fn developer_json(&self, cwd: &Path, args: &[&str]) -> Value {
        let out = self.developer(cwd, args);
        let shown = String::from_utf8_lossy(&out.stdout).into_owned();
        let start = shown
            .find('{')
            .unwrap_or_else(|| panic!("{args:?}: no JSON in {shown:?}"));
        serde_json::Deserializer::from_str(&shown[start..])
            .into_iter::<Value>()
            .next()
            .unwrap()
            .unwrap_or_else(|e| panic!("{args:?}: {e}: {shown:?}"))
    }

    /// `raptor <args>` without a pty and without an agent above it: stdout and stderr apart.
    fn plain(&self, args: &[&str]) -> Cap {
        let out = Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .env("HOME", &self.f.home)
            .current_dir(&self.worktree)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Cap::of(&out)
    }

    fn launch_claude(&self) -> FakeClaude {
        let agent = self.outside.path().join(FAKE_AGENT);
        if !agent.exists() {
            gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &agent);
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

    /// `raptor <args>` run by the simulated agent: its stdout, stderr and exit code apart.
    fn as_agent(&self, claude: &mut FakeClaude, args: &[&str]) -> Cap {
        let dir = self.outside.path();
        let quoted: Vec<String> = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
            .collect();
        let (out, err, code) = (dir.join("out"), dir.join("err"), dir.join("code"));
        for p in [&out, &err, &code] {
            let _ = std::fs::remove_file(p);
        }
        claude.run(&format!(
            "{RAPTOR} {} > {} 2> {}; echo $? > {}",
            quoted.join(" "),
            out.display(),
            err.display(),
            code.display()
        ));
        Cap {
            code: std::fs::read_to_string(&code)
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(-1),
            stdout: std::fs::read_to_string(&out).unwrap_or_default(),
            stderr: std::fs::read_to_string(&err).unwrap_or_default(),
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

    /// Waits for the Time Machine to capture `path` with `content` in the worktree.
    fn captured(&self, path: &str, content: &[u8]) {
        let repo_id = self.repo_id();
        let wt = self.worktree.file_name().unwrap().to_string_lossy();
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &repo_id) {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    let found = store
                        .files(id, &format!("wt-wt-{}", wt.trim_start_matches("wt-")))
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
                        .any(|(p, _, oid)| {
                            p == path && store.read_blob(oid).is_ok_and(|b| b == content)
                        });
                    if found {
                        return;
                    }
                }
            }
            assert!(start.elapsed() < DEADLINE, "{path} never captured");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The agent edits, throws its work away with raw Git: something to undo.
    fn agent_resets(&self, claude: &mut FakeClaude) {
        assert!(claude.run("printf 'user\\npassword\\n' > login.txt"));
        self.captured("login.txt", WORK);
        let hooks = self.outside.path().join("slow-hooks");
        assert!(claude.run(&format!(
            "git -c core.hooksPath={} reset -q --hard",
            hooks.display()
        )));
    }

    fn branch_exists(&self, name: &str) -> bool {
        !self.f.git(&["branch", "--list", name]).trim().is_empty()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// Keys of `data` that an agent register output carries today (DS § T001).
const REGISTER_KEYS: [&str; 7] = [
    "repo_id",
    "session_id",
    "outcome",
    "agent",
    "agent_name",
    "origin",
    "support",
];

// ----------------------------------------------------------------------------------- V1

#[test]
fn every_agent_subcommand_declares_the_contract_version() {
    let m = Machine::new();
    let mut claude = m.launch_claude();

    m.as_agent(&mut claude, &["status", "--json"])
        .data("status");
    m.as_agent(&mut claude, &["agent", "register", "Claude Code", "--json"])
        .data("agent register");
    let daemon = m
        .as_agent(&mut claude, &["daemon", "status", "--json"])
        .data("daemon status");
    assert!(
        daemon.get("engine_state").is_some(),
        "daemon status as an agent carries engine_state: {daemon}"
    );

    m.agent_resets(&mut claude);
    m.as_agent(&mut claude, &["undo", "--json"]).data("undo");
    m.as_agent(&mut claude, &["redo", "--json"]).data("redo");
}

#[test]
fn agent_outputs_deserialize_into_their_contract_type() {
    let m = Machine::new();
    let mut claude = m.launch_claude();

    let status = m
        .as_agent(&mut claude, &["status", "--json"])
        .data("status");
    serde_json::from_value::<McpStatus>(status.clone())
        .unwrap_or_else(|e| panic!("status data is not McpStatus ({e}): {status}"));

    let register = m
        .as_agent(&mut claude, &["agent", "register", "Claude Code", "--json"])
        .data("agent register");
    for key in REGISTER_KEYS {
        assert!(
            register.get(key).is_some(),
            "AgentRegisterView carries {key}: {register}"
        );
    }
    assert_eq!(
        register.as_object().unwrap().len(),
        REGISTER_KEYS.len(),
        "AgentRegisterView carries only its contract fields: {register}"
    );

    m.agent_resets(&mut claude);
    let undo = m.as_agent(&mut claude, &["undo", "--json"]).data("undo");
    serde_json::from_value::<McpUndoResult>(undo.clone())
        .unwrap_or_else(|e| panic!("undo data is not McpUndoResult ({e}): {undo}"));
    let redo = m.as_agent(&mut claude, &["redo", "--json"]).data("redo");
    serde_json::from_value::<McpRedoResult>(redo.clone())
        .unwrap_or_else(|e| panic!("redo data is not McpRedoResult ({e}): {redo}"));
}

// ----------------------------------------------------------------------------------- V2

#[test]
fn outside_the_agent_view_is_a_json_refusal() {
    let m = Machine::new();
    let mut claude = m.launch_claude();

    for args in [
        &["sessions", "--json"][..],
        &["timeline", "--json"][..],
        &["guard", "log", "--json"][..],
    ] {
        let what = args.join(" ");
        let cap = m.as_agent(&mut claude, args);
        let error = cap.refusal(&what, "command-unavailable", 1);
        assert_eq!(
            error["params"]["reason"], "agent-view",
            "{what}: the refusal says why: {error}"
        );
    }

    let cap = m.as_agent(&mut claude, &["agent", "withdraw", "Claude Code", "--json"]);
    cap.refusal("agent withdraw", "reserved-command", 1);

    let cap = m.as_agent(
        &mut claude,
        &["guard", "exec", "--json", "--yes", "--", "git", "commit"],
    );
    cap.refusal("guard exec -- git commit", "reserved-command", 1);
}

#[test]
fn an_agent_guard_exec_is_a_reserved_command_refusal() {
    let m = Machine::new();
    let mut claude = m.launch_claude();
    let repo = path(&m.f.repo).to_owned();
    let cap = m.as_agent(
        &mut claude,
        &[
            "guard", "exec", "--json", "--yes", "--", "git", "-C", &repo, "branch", "-d", "victim",
        ],
    );
    let error = cap.refusal("guard exec", "reserved-command", 1);
    assert_ne!(
        error["code"], "command-unavailable",
        "the engine decides a reserved command, the CLI does not refuse it first: {error}"
    );
    assert!(
        m.branch_exists("victim"),
        "a refused guard exec must run nothing"
    );
}

// ----------------------------------------------------------------------------------- V3

#[test]
fn the_developer_withdraws_with_json() {
    let m = Machine::new();
    let wt = path(&m.worktree).to_owned();
    m.ok(
        &m.f.root,
        &["agent", "register", "Claude Code", "--worktree", &wt],
    );
    let out = m.developer_json(
        &m.f.root,
        &[
            "agent",
            "withdraw",
            "Claude Code",
            "--worktree",
            &wt,
            "--json",
        ],
    );
    assert_eq!(out["schema_version"], "1.0", "{out}");
    let data = &out["data"];
    for key in ["repo_id", "session_id"] {
        assert!(
            data.get(key).is_some(),
            "agent withdraw --json carries the withdrawn registration ({key}): {out}"
        );
    }
}

#[test]
fn the_developer_reads_daemon_status_and_runs_guard_exec_with_json() {
    let m = Machine::new();
    let status = m.developer_json(&m.f.root, &["daemon", "status", "--json"]);
    assert_eq!(status["schema_version"], "1.0", "{status}");
    assert!(
        status["data"].get("engine_state").is_some(),
        "daemon status --json carries data.engine_state: {status}"
    );

    // Without --yes there is no interactive question in JSON: confirmation-required, nothing runs.
    let repo = path(&m.f.repo).to_owned();
    let argv = ["--", "git", "-C", &repo, "branch", "-d", "victim"];
    let mut ask = vec!["guard", "exec", "--json"];
    ask.extend_from_slice(&argv);
    let refused = m.developer_json(&m.f.root, &ask);
    assert_eq!(refused["schema_version"], "1.0", "{refused}");
    assert_eq!(
        refused["error"]["code"], "confirmation-required",
        "guard exec --json without --yes: {refused}"
    );
    assert!(m.branch_exists("victim"), "nothing ran without --yes");

    let mut run = vec!["guard", "exec", "--json", "--yes"];
    run.extend_from_slice(&argv);
    let done = m.developer_json(&m.f.root, &run);
    assert_eq!(done["schema_version"], "1.0", "{done}");
    assert_eq!(
        done["data"]["git_exit_code"], 0,
        "guard exec --json --yes carries git's exit code: {done}"
    );
}

// ----------------------------------------------------------------------------------- V4

#[test]
fn an_engine_that_cannot_start_answers_in_json() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    for dir in ["", "data", "config", "state"] {
        let p = profile.join(dir);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    // The runtime folder is a regular file: the engine cannot create its socket.
    std::fs::write(profile.join("run"), "not a folder").unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();

    let out = Command::new(RAPTOR)
        .args(["status", "--json"])
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", &profile)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(tmp.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let state = ProfileDirs::under_root(&profile).state;
    if let Ok(Some(pid)) = running_pid(&state) {
        let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
    }
    let cap = Cap::of(&out);
    cap.refusal("status with no engine", "engine-unavailable", 1);
}

// ----------------------------------------------------------------------------------- V5

#[test]
fn a_repo_not_enabled_gives_no_data_in_json() {
    let m = Machine::build("feat-login", "feat-login", false);
    let mut claude = m.launch_claude();
    let cap = m.as_agent(&mut claude, &["status", "--json"]);
    let error = cap.refusal("status in a repo not enabled", "repo-not-enabled", 1);
    assert!(
        error["action"].as_str().is_some_and(|a| !a.is_empty()),
        "the refusal carries its action: {error}"
    );
    for secret in [path(&m.f.repo), path(&m.worktree), "feat-login"] {
        assert!(
            !cap.stdout.contains(secret),
            "no datum of the repo ({secret}) in: {}",
            cap.stdout
        );
    }
}

// ----------------------------------------------------------------------------------- V8

#[test]
fn a_usage_error_with_json_is_json() {
    let m = Machine::new();
    let cap = m.plain(&["status", "--json", "--no-such-flag"]);
    let error = cap.refusal("usage error", "invalid-args", 2);
    assert!(
        error["params"].get("kind").is_some(),
        "invalid-args params carry the kind of the error: {error}"
    );
}

#[test]
fn a_usage_error_does_not_echo_the_typed_value() {
    let m = Machine::new();
    let typed = "zz-typed-secret-value";
    for args in [
        vec!["status", "--json", typed],
        vec!["undo", "--json", "--since", typed, "--bogus", typed],
        vec!["status", "--json", &format!("--flag-{typed}")],
    ] {
        let cap = m.plain(&args);
        let error = cap.refusal(&format!("{args:?}"), "invalid-args", 2);
        assert!(
            !cap.stdout.contains(typed),
            "{args:?}: the typed value must not be echoed: {error}"
        );
    }
}

// ----------------------------------------------------------------------------------- V9

#[test]
fn hostile_names_arrive_escaped_in_json() {
    let m = Machine::build("\u{1b}[31mred", "feat\u{202e}x\u{200b}y", true);
    let mut claude = m.launch_claude();
    let cap = m.as_agent(&mut claude, &["status", "--json"]);
    cap.data("status in a hostile worktree");
    assert!(
        !cap.stdout.as_bytes().contains(&0x1b),
        "no ESC byte reaches stdout: {:?}",
        cap.stdout
    );
    for hidden in ['\u{202e}', '\u{200b}'] {
        assert!(
            !cap.stdout.contains(hidden),
            "no bidi or zero-width character reaches stdout: {:?}",
            cap.stdout
        );
    }
}

#[test]
fn every_json_output_is_one_line() {
    let m = Machine::new();
    let mut claude = m.launch_claude();
    for args in [
        &["status", "--json"][..],
        &["agent", "register", "Claude Code", "--json"][..],
        &["daemon", "status", "--json"][..],
        &["sessions", "--json"][..],
    ] {
        let cap = m.as_agent(&mut claude, args);
        assert!(
            cap.stderr.is_empty(),
            "{args:?}: stderr must be empty: {:?}",
            cap.stderr
        );
        assert_eq!(
            cap.stdout.trim_end_matches('\n').lines().count(),
            1,
            "{args:?}: one compact line, not pretty-printed JSON: {:?}",
            cap.stdout
        );
        assert!(
            cap.stdout.ends_with('\n') && !cap.stdout.trim_end().contains('\n'),
            "{args:?}: {:?}",
            cap.stdout
        );
    }
    for args in [&["status", "--json"][..], &["sessions", "--json"][..]] {
        let cap = m.plain(args);
        assert_eq!(
            cap.stdout.trim_end_matches('\n').lines().count(),
            1,
            "{args:?} as the developer: one compact line: {:?}",
            cap.stdout
        );
    }
}

// ----------------------------------------------------------------------------------- V10

#[test]
fn without_json_the_output_is_unchanged() {
    let m = Machine::new();
    for args in [&["status"][..], &["sessions"][..], &["events"][..]] {
        let cap = m.plain(args);
        assert!(
            !cap.stdout.contains("schema_version"),
            "{args:?}: the human output carries no envelope: {:?}",
            cap.stdout
        );
        let first = cap.stdout.trim_start().chars().next();
        assert!(
            !matches!(first, Some('{') | Some('[')),
            "{args:?}: the human output is not JSON: {:?}",
            cap.stdout
        );
        assert!(
            !cap.stdout.trim().is_empty(),
            "{args:?}: the human output is still there: stderr {:?}",
            cap.stderr
        );
    }
}
