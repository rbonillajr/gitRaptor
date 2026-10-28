//! US-GRP-010 through the real channel: `attribution.correct` and
//! `attribution.withdraw-correction`, reserved commands, with the rejections of the scenario
//! outline and the projections the Time Machine reads.
//!
//! The daemon runs in a child process of this test binary over a temporary repo, profile and HOME
//! (NFR-01), and knows one agent executable, `raptor-fake-agent`, so a Claude Code session that
//! runs the suite is not taken for an agent. Every process but the daemon is this binary with a
//! role (`role_entry`, env `RAPTOR_T_GRP010`):
//! - `shell`: a copy named `raptor-fake-agent` with its cwd in a worktree; the engine detects it as
//!   Claude Code, and it runs each line of its input with `/bin/sh` (so a `git commit` descends
//!   from it and S3 attributes the commit to its session);
//! - `calls` as the developer: the test binary under `script`, so it has a controlling terminal
//!   and the engine takes it for the developer; as an agent: the same role from a copy named
//!   `raptor-fake-agent` (working outside every worktree, so it is not a session of its own).
//!
//! Methods go by their names and parameters as JSON, so the file does not depend on the types
//! the build creates. "Nothing changes" is the row count of `sessions` and `attribution_records`
//! in the repo's store plus `sessions.list`, before and after. Waits end on explicit signals with a
//! deadline; no fixed sleeps.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::{
    AuditListResult, AuditOutcome, ClientKind, RefusalReason, SessionStateView, SessionView,
    SessionsListResult,
};
use gitraptor_api::rpc::code;
use gitraptor_api::{Actor, AgentKind, AgentOrigin, PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use serde_json::{Value, json};

const ENV: &str = "RAPTOR_T_GRP010";
const DEADLINE: Duration = Duration::from_secs(30);
const AGENT_NAME: &str = "raptor-fake-agent";
const DONE: &str = "<<raptor-fake-agent-done>>";
/// `attribution-rejected`, the block of the attribution module (ADR-GRP-016, D7).
const ATTRIBUTION_REJECTED: i64 = -33160;

// ----- The roles ---------------------------------------------------------------------------------

/// Every process of the test but the test itself is this binary with a role; as a normal test it
/// does nothing.
#[test]
fn role_entry() {
    let Some(spec) = std::env::var_os(ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    match spec["role"].as_str().unwrap() {
        "daemon" => daemon_role(&spec, dirs),
        "shell" => shell_role(),
        "calls" => calls_role(&spec, &dirs),
        other => panic!("unknown role {other}"),
    }
}

fn daemon_role(spec: &Value, dirs: ProfileDirs) {
    let vars = std::env::vars_os()
        .filter(|(key, _)| key != "HOME")
        .chain([("HOME".into(), spec["home"].as_str().unwrap().into())]);
    let env = DaemonEnv::from_vars(vars);
    let config = DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig {
            agents: AgentMatcher::only(vec![AGENT_NAME.into()]),
            ..ChannelConfig::default()
        },
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let stop = PathBuf::from(spec["stop"].as_str().unwrap());
    std::thread::spawn(move || {
        while !stop.exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
        handle.request(StopCause::Signal("TERM"));
    });
    daemon.run();
}

/// The simulated agent: runs each line of its input with `/bin/sh` and prints [`DONE`] after it.
fn shell_role() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let status = Command::new("/bin/sh").arg("-c").arg(&line).status();
        println!("{DONE} {}", status.map(|s| s.success()).unwrap_or(false));
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

/// Sends each `[method, params]` of `calls` and writes the answers to `out`: `{ok}`, `{code,
/// data}` or `{other}`.
fn calls_role(spec: &Value, dirs: &ProfileDirs) {
    let mut client = Client::connect(dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let answers: Vec<Value> = spec["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| {
            let method = call[0].as_str().unwrap();
            match client.call::<_, Value>(method, call[1].clone()) {
                Ok(v) => json!({ "ok": v }),
                Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
                Err(e) => json!({ "other": e.to_string() }),
            }
        })
        .collect();
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&answers).unwrap()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

// ----- The machine -------------------------------------------------------------------------------

struct ShellAgent {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl ShellAgent {
    fn run(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "({command}) 2>&1").unwrap();
        stdin.flush().unwrap();
        let mut output = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "fake agent died"
            );
            if let Some((before, ok)) = line.split_once(DONE) {
                output.push_str(before);
                assert_eq!(ok.trim(), "true", "{command} failed: {output}");
                return;
            }
            output.push_str(&line);
        }
    }

    /// A commit with a slow `post-commit` hook, so `git` is alive when S3 samples it.
    fn commit(&mut self, hooks: &Path, message: &str) {
        self.run(&format!(
            "printf 'user\\n{message}\\n' > login.txt && git add login.txt && \
             git -c core.hooksPath={} commit -q -m '{message}'",
            hooks.display()
        ));
    }
}

impl Drop for ShellAgent {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Env {
    fx: Fixture,
    tp: TempProfile,
    repo_id: String,
    login: PathBuf,
    pagos: PathBuf,
    stop: PathBuf,
    daemon: Child,
    bin: PathBuf,
    hooks: PathBuf,
    out_n: std::cell::Cell<u32>,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.stop, b"");
        let start = Instant::now();
        while self.daemon.try_wait().ok().flatten().is_none() {
            if start.elapsed() > DEADLINE {
                let _ = self.daemon.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn role_command(exe: &Path, spec: &Value) -> Command {
    let mut c = Command::new(exe);
    c.args(["role_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV, spec.to_string());
    c
}

/// "demo": `main`, the worktrees `feat-login` and `feat-pagos`, a repo `other-repo` the engine does
/// not observe; the daemon observes `demo`.
fn env() -> Env {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    fx.git(&["branch", "feat-login"]);
    fx.git(&["branch", "feat-pagos"]);
    let login = canonical(&fx.add_worktree("feat-login", "feat-login"));
    let pagos = canonical(&fx.add_worktree("feat-pagos", "feat-pagos"));
    let tp = TempProfile::new();
    let (entry, _) = tp
        .open()
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    let bin = tp.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let hooks = tp.root.path().join("slow-hooks");
    std::fs::create_dir(&hooks).unwrap();
    let post_commit = hooks.join("post-commit");
    std::fs::write(&post_commit, "#!/bin/sh\nsleep 1\n").unwrap();
    std::fs::set_permissions(&post_commit, std::fs::Permissions::from_mode(0o755)).unwrap();
    gitraptor_testkit::fixture::clone_binary(
        &std::env::current_exe().unwrap(),
        &bin.join(AGENT_NAME),
    );
    let stop = tp.root.path().join("stop-daemon");
    let spec = json!({
        "role": "daemon",
        "root": tp.root.path().join("profile"),
        "home": fx.home,
        "stop": stop,
    });
    let daemon = role_command(&std::env::current_exe().unwrap(), &spec)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
        assert!(start.elapsed() < DEADLINE, "the daemon never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    Env {
        fx,
        tp,
        repo_id: entry.repo_id,
        login,
        pagos,
        stop,
        daemon,
        bin,
        hooks,
        out_n: std::cell::Cell::new(0),
    }
}

impl Env {
    fn client(&self) -> Client {
        Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    /// The simulated Claude Code, working in `worktree`.
    fn agent_in(&self, worktree: &Path) -> ShellAgent {
        let spec = json!({"role": "shell", "root": self.tp.root.path().join("profile")});
        let mut child = role_command(&self.bin.join(AGENT_NAME), &spec)
            .env("HOME", &self.fx.home)
            .env("PATH", "/usr/bin:/bin")
            .current_dir(worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        ShellAgent {
            child,
            stdin,
            stdout,
        }
    }

    fn sessions(&self) -> Vec<SessionView> {
        let r: SessionsListResult = self
            .client()
            .call(
                methods::SESSIONS_LIST,
                json!({"repo_id": self.repo_id, "include_ended": true}),
            )
            .unwrap();
        r.sessions
    }

    fn sessions_of(&self, worktree: &Path) -> Vec<SessionView> {
        self.sessions()
            .into_iter()
            .filter(|s| Path::new(s.worktree.raw()) == worktree)
            .collect()
    }

    /// Waits until the sessions of `worktree` satisfy `ok`.
    fn wait_sessions(
        &self,
        worktree: &Path,
        ok: impl Fn(&[SessionView]) -> bool,
    ) -> Vec<SessionView> {
        let start = Instant::now();
        loop {
            let sessions = self.sessions_of(worktree);
            if ok(&sessions) {
                return sessions;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "{}: {sessions:#?}",
                worktree.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for `n` present detected Claude Code sessions in `worktree`.
    fn wait_detected(&self, worktree: &Path, n: usize) -> Vec<SessionView> {
        self.wait_sessions(worktree, |s| {
            s.iter()
                .filter(|s| is_detected_claude(s) && present(s))
                .count()
                == n
        })
        .into_iter()
        .filter(present)
        .collect()
    }

    /// Rows of `sessions` and `attribution_records` of the repo's store.
    fn rows(&self) -> (i64, i64) {
        let path = self
            .tp
            .dirs()
            .repos_dir()
            .join(format!("{}.sqlite", self.repo_id));
        let conn =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        (count("sessions"), count("attribution_records"))
    }

    /// `(rows, sessions.list)`: what "nothing changes" compares.
    fn state(&self) -> ((i64, i64), Vec<SessionView>) {
        (self.rows(), self.sessions())
    }

    /// The calls from a process that `who` runs: the developer in a terminal, or an agent.
    fn calls(&self, agent: bool, calls: &[(&str, Value)]) -> Vec<Value> {
        let n = self.out_n.get();
        self.out_n.set(n + 1);
        let out = self.tp.root.path().join(format!("answers-{n}.json"));
        let spec = json!({
            "role": "calls",
            "root": self.tp.root.path().join("profile"),
            "calls": calls.iter().map(|(m, p)| json!([m, p])).collect::<Vec<_>>(),
            "out": out,
        });
        if agent {
            let status = role_command(&self.bin.join(AGENT_NAME), &spec)
                .env_clear()
                .env(ENV, spec.to_string())
                .env("HOME", &self.fx.home)
                .current_dir(&self.fx.root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(
                out.exists(),
                "the agent's client ended ({status}) without answers"
            );
        } else {
            let exe = std::env::current_exe().unwrap();
            let entry = ["role_entry", "--exact", "--nocapture", "--test-threads=1"];
            let mut command = if cfg!(target_os = "macos") {
                let mut c = Command::new("/usr/bin/script");
                c.args(["-q", "/dev/null"]).arg(&exe).args(entry);
                c
            } else {
                let line = std::iter::once(exe.to_string_lossy().into_owned())
                    .chain(entry.iter().map(|s| (*s).to_owned()))
                    .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut c = Command::new("script");
                c.args(["-q", "-e", "-c", &line, "/dev/null"]);
                c
            };
            // A clean environment, as a developer's terminal has.
            let status = command
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", &self.fx.home)
                .env(ENV, spec.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(
                out.exists(),
                "the developer's client ended ({status}) without answers"
            );
        }
        serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
    }

    fn developer(&self, method: &str, params: Value) -> Value {
        self.calls(false, &[(method, params)]).remove(0)
    }

    fn correct(&self, agent: Value, worktree: &Path, session: Option<&str>) -> Value {
        let mut params = json!({"agent": agent, "worktree": worktree});
        if let Some(id) = session {
            params["session_id"] = json!(id);
        }
        self.developer(methods::ATTRIBUTION_CORRECT, params)
    }

    fn withdraw(&self, worktree: &Path, session: Option<&str>) -> Value {
        let mut params = json!({ "worktree": worktree });
        if let Some(id) = session {
            params["session_id"] = json!(id);
        }
        self.developer(methods::ATTRIBUTION_WITHDRAW, params)
    }

    /// `registration.register` by the developer.
    fn register(&self, name: &str, worktree: &Path) -> Value {
        self.developer(
            methods::REGISTRATION_REGISTER,
            json!({"agent": {"kind": "other", "name": name}, "worktree": worktree}),
        )
    }
}

fn codex() -> Value {
    json!({"kind": "other", "name": "Codex"})
}

fn claude() -> Value {
    json!({"kind": "claude-code"})
}

fn present(s: &SessionView) -> bool {
    s.state != SessionStateView::Ended
}

fn is_detected_claude(s: &SessionView) -> bool {
    matches!(
        &s.actor,
        Actor::Agent {
            kind: AgentKind::ClaudeCode,
            origin: AgentOrigin::Detected,
            ..
        }
    )
}

fn is_codex_registered(s: &SessionView) -> bool {
    matches!(
        &s.actor,
        Actor::Agent {
            kind: AgentKind::Other,
            name: Some(name),
            origin: AgentOrigin::Registered,
        } if name.raw() == "Codex"
    )
}

/// The rejection reason of an `ATTRIBUTION_REJECTED` answer; panics with the answer otherwise.
fn rejected(answer: &Value) -> &str {
    assert_eq!(
        answer["code"], ATTRIBUTION_REJECTED,
        "expected attribution-rejected: {answer}"
    );
    answer["data"]["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("no reason in {answer}"))
}

fn ok(answer: &Value) -> &Value {
    answer
        .get("ok")
        .unwrap_or_else(|| panic!("expected success: {answer}"))
}

// ----- Tests -------------------------------------------------------------------------------------

/// GRP010-CORE-1 (6a): correcting in a repo the engine does not observe.
#[test]
fn correcting_in_an_unobserved_repo_is_rejected() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    let before = e.state();

    let answer = e.correct(codex(), &canonical(&e.fx.other_repo), None);

    assert_eq!(rejected(&answer), "repo-not-observed");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-2 (6b): a worktree without a detected session.
#[test]
fn correcting_without_a_detected_attribution_is_rejected() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    let before = e.state();

    let answer = e.correct(codex(), &e.pagos, None);

    assert_eq!(rejected(&answer), "no-detected-attribution");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-3 (6b, D5): `--session` of a registered session says to withdraw the registration.
#[test]
fn a_registered_session_is_told_to_withdraw_it() {
    let e = env();
    let registered = e.register("Codex", &e.pagos);
    let id = ok(&registered)["session_id"].as_str().unwrap().to_owned();
    let before = e.state();

    let answer = e.correct(claude(), &e.pagos, Some(&id));

    assert_eq!(rejected(&answer), "registered-session");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-4 (D9): with a correction active, correcting to the detected agent succeeds and
/// says so.
#[test]
fn correcting_back_to_the_detected_agent_suggests_uncorrect() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    let session = e.wait_detected(&e.login, 1).remove(0);

    let first = e.correct(codex(), &e.login, None);
    assert_eq!(ok(&first)["matches_detected"], false, "{first}");
    assert_eq!(ok(&first)["session_id"], session.session_id.as_str());
    let back = e.correct(claude(), &e.login, None);

    assert_eq!(ok(&back)["matches_detected"], true, "{back}");
    assert_eq!(
        ok(&back)["previous"]["name"]["untrusted"],
        "Codex",
        "{back}"
    );
    assert_eq!(ok(&back)["actor"]["kind"], "claude-code", "{back}");
    assert_eq!(
        e.sessions_of(&e.login).len(),
        1,
        "a correction adds no session"
    );
}

/// GRP010-CORE-5 (6c): an agent is refused on both methods and it is audited; then the developer
/// corrects that same session.
#[test]
fn an_agent_cannot_correct_and_it_is_audited() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    let session = e.wait_detected(&e.login, 1).remove(0);
    let before = e.state();

    let answers = e.calls(
        true,
        &[
            (
                methods::ATTRIBUTION_CORRECT,
                json!({"agent": codex(), "worktree": e.login}),
            ),
            (methods::ATTRIBUTION_WITHDRAW, json!({"worktree": e.login})),
        ],
    );
    for answer in &answers {
        assert_eq!(answer["code"], code::RESERVED_REFUSED, "{answer}");
    }
    assert_eq!(e.state(), before, "no session nor attribution changed");
    let audit: AuditListResult = e.client().call(methods::AUDIT_LIST, json!({})).unwrap();
    for method in [methods::ATTRIBUTION_CORRECT, methods::ATTRIBUTION_WITHDRAW] {
        let entry = audit
            .entries
            .iter()
            .find(|entry| entry.operation == method)
            .unwrap_or_else(|| panic!("{method} was not audited"));
        assert_eq!(entry.outcome, AuditOutcome::Rejected, "{method}");
        assert_eq!(entry.reason, Some(RefusalReason::AgentAncestry), "{method}");
    }

    // The developer, on that same session, is served: the refusal was about who asks.
    let done = e.correct(codex(), &e.login, Some(&session.session_id));
    assert_eq!(
        ok(&done)["session_id"],
        session.session_id.as_str(),
        "{done}"
    );
    let sessions = e.sessions_of(&e.login);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_codex_registered(&sessions[0]), "{sessions:#?}");
}

/// GRP010-CORE-6: what the Time Machine reads (the timeline's `SessionActors` over
/// `sessions.list`, and `requester_of` over the same projection) follows the correction and its
/// withdrawal.
#[test]
fn the_time_machine_sees_the_current_attribution() {
    let e = env();
    let mut claude_agent = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    claude_agent.commit(&e.hooks, "c1");

    let actors = |want: &Value| -> Vec<Value> {
        let start = Instant::now();
        loop {
            let timeline: Value = e
                .client()
                .call(methods::TM_TIMELINE, json!({"worktree": e.login}))
                .unwrap();
            let actors: Vec<Value> = timeline["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["origin"]["entry"] == "git-event")
                .map(|entry| entry["actor"].clone())
                .filter(|actor| actor["actor"] != "unattributed")
                .collect();
            if actors.iter().any(|a| a == want) || start.elapsed() > DEADLINE {
                return actors;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let detected = json!({"actor": "agent", "kind": "claude-code", "origin": "detected"});
    let corrected = json!({"actor": "agent", "kind": "other", "name": {"untrusted": "Codex"}, "origin": "registered"});

    let before = actors(&detected);
    assert!(
        !before.is_empty() && before.iter().all(|a| *a == detected),
        "the commit is Claude Code, detected: {before:?}"
    );
    ok(&e.correct(codex(), &e.login, None));
    let after = actors(&corrected);
    assert!(
        !after.is_empty() && after.iter().all(|a| *a == corrected),
        "the timeline follows the correction: {after:?}"
    );
    ok(&e.withdraw(&e.login, None));
    let back = actors(&detected);
    assert!(
        !back.is_empty() && back.iter().all(|a| *a == detected),
        "the timeline follows the withdrawal: {back:?}"
    );
}

/// GRP010-CORE-7 (D5): correcting to the agent the session already has.
#[test]
fn a_same_agent_correction_is_rejected() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    let before = e.state();

    let answer = e.correct(claude(), &e.login, None);
    assert_eq!(rejected(&answer), "same-agent");
    assert_eq!(e.state(), before, "no session nor attribution changed");

    ok(&e.correct(codex(), &e.login, None));
    let before = e.state();
    let answer = e.correct(codex(), &e.login, None);
    assert_eq!(rejected(&answer), "same-agent");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-8 (D4): several detected sessions present, no `--session`.
#[test]
fn ambiguous_sessions_list_candidates() {
    let e = env();
    let _first = e.agent_in(&e.login);
    let _second = e.agent_in(&e.login);
    let sessions = e.wait_detected(&e.login, 2);
    let before = e.state();

    let answer = e.correct(codex(), &e.login, None);

    assert_eq!(rejected(&answer), "ambiguous-session");
    let candidates = answer["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "{answer}");
    assert!(candidates.len() <= 10);
    for session in &sessions {
        assert!(
            candidates
                .iter()
                .any(|c| c["session_id"] == session.session_id.as_str() && c["present"] == true),
            "{} is not a candidate: {answer}",
            session.session_id
        );
    }
    for c in candidates {
        assert!(c["started_utc_ms"].is_i64(), "{c}");
        assert_eq!(c["actor"]["kind"], "claude-code", "{c}");
    }
    // SEC-12: no paths.
    let root = e.fx.root.to_string_lossy().into_owned();
    assert!(!answer["data"].to_string().contains(&root), "{answer}");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-9 (D4): `--session` picks an ended session of the worktree and corrects it.
#[test]
fn correcting_an_ended_session_by_id() {
    let e = env();
    let agent = e.agent_in(&e.login);
    let session = e.wait_detected(&e.login, 1).remove(0);
    drop(agent);
    e.wait_sessions(&e.login, |s| s.len() == 1 && !present(&s[0]));
    // Without `--session` there is no present detected session to pick.
    assert_eq!(
        rejected(&e.correct(codex(), &e.login, None)),
        "no-detected-attribution"
    );

    let done = e.correct(codex(), &e.login, Some(&session.session_id));

    assert_eq!(
        ok(&done)["session_id"],
        session.session_id.as_str(),
        "{done}"
    );
    let sessions = e.sessions_of(&e.login);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert!(is_codex_registered(&sessions[0]), "{sessions:#?}");
    assert!(!present(&sessions[0]), "it stays ended: {sessions:#?}");
}

/// GRP010-CORE-10: withdrawing a correction that is not there.
#[test]
fn withdrawing_without_a_correction_is_rejected() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    let before = e.state();

    let answer = e.withdraw(&e.login, None);

    assert_eq!(rejected(&answer), "not-corrected");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-11 (D6): registering the corrected agent adds no session and no confirmation.
#[test]
fn registering_the_corrected_agent_does_not_add_a_session() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    ok(&e.correct(codex(), &e.login, None));
    let before = e.state();

    let answer = e.register("Codex", &e.login);

    assert_eq!(ok(&answer)["outcome"], "already-registered", "{answer}");
    assert_eq!(e.state(), before, "no new session and no confirm record");
}

/// GRP010-CORE-12: `--session` of an unknown id, or of another worktree's session.
#[test]
fn an_unknown_session_is_rejected() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    let other = e.register("Cursor", &e.pagos);
    let other_id = ok(&other)["session_id"].as_str().unwrap().to_owned();
    let before = e.state();

    let answer = e.correct(codex(), &e.login, Some("9999:1"));
    assert_eq!(rejected(&answer), "unknown-session");
    let answer = e.correct(codex(), &e.login, Some(&other_id));
    assert_eq!(
        rejected(&answer),
        "unknown-session",
        "not a session of that worktree"
    );
    assert_eq!(e.state(), before, "no session nor attribution changed");
}

/// GRP010-CORE-13: a folder that is not a worktree.
#[test]
fn correcting_outside_a_worktree_is_rejected() {
    let e = env();
    let _claude = e.agent_in(&e.login);
    e.wait_detected(&e.login, 1);
    let plain = e.tp.root.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let before = e.state();

    let answer = e.correct(codex(), &canonical(&plain), None);

    assert_eq!(rejected(&answer), "not-a-worktree");
    assert_eq!(e.state(), before, "no session nor attribution changed");
}
