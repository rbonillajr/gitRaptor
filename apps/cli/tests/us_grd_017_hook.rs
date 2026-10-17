//! US-GRD-017 end to end: with the Guardrails hooks installed, a rebase or a local branch deletion
//! that the rules allow only runs if the Time Machine saved its prior point first. If it could
//! not (its deadline is over, there is no daemon, the repo is no longer observed) the operation
//! does not run, the reason says why and how to go on, and the branch and the working tree stay
//! as they were. Non-destructive operations are untouched.
//!
//! The real `raptor` as daemon and hook client, the real dispatcher and plain Git, over a
//! temporary repo, a bare remote and a temporary profile (NFR-01). The agent is
//! `raptor-fake-agent`, a copy of this test binary the debug daemon knows as Claude Code
//! (`agent → sh → git → hook`). The failed prior is induced with
//! `GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS=0` in the daemon's environment. No fixed waits: every
//! state is awaited with a deadline. The messages are read in Spanish (`LANG=es_ES.UTF-8`, the
//! only language variable the hook dispatcher forwards), the wording the story fixes.
//!
//! Unix only, as `guard_us_grd_008.rs` (Windows: pending cross-platform validation stage).
#![cfg(unix)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, Snapshot};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{
    Oplog, SnapshotFilter, SnapshotLevel, SnapshotState, SnapshotView,
};
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{copy_executable, git_from_path};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The command the simulated agent runs with `/bin/sh`.
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
/// The hook prior's deadline override of the debug daemon.
const DEADLINE_ENV: &str = "GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS";
const DEADLINE: Duration = Duration::from_secs(60);
const API_RS: &str = "pub fn api() -> u32 { 42 }\n";
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";

/// The wording the story fixes (Spanish), as the hook writes it.
const WHY_TIME_LIMIT: &str = "se agotó el plazo para guardarlo";
const HINT_START: &str = "arranca GitRaptor y repite la operación";
const NOT_RUNNING: &str = "GitRaptor no está en marcha";
/// `degraded`'s own reason (`guard.degraded`, Spanish).
const DEGRADED: &str = "sin conexión con el servicio";
/// The US-TMC-005 warning that lets the operation through (`hookprior.failed`, Spanish): after
/// US-GRD-017 it may only appear when the operation was allowed.
const GOES_AHEAD_NOTICE: [&str; 2] = ["sigue adelante", "no se guardó un snapshot previo"];

/// Entry point of the simulated Claude Code: when this binary runs as `raptor-fake-agent` with
/// [`AGENT_CMD`], it runs that command and exits with its status. As a normal test, nothing.
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

/// "demo" with `main` (newer than the branches); `feat-x` with 3 commits that are on no other
/// branch (the first adds `api.rs`); `feat-y` with 2 commits of its own, behind `main`; `ff`, a
/// commit ahead of the first `main`; a bare remote with `main`. Observed and protected by
/// `raptor guard install`, with the Time Machine ready.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    remote: PathBuf,
    extra: Vec<(&'static str, OsString)>,
}

impl Machine {
    fn new(extra: &[(&'static str, &str)]) -> Self {
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
        let commit = |file: &str, content: &str, msg: &str| {
            f.write(file, content);
            f.git(&["add", file]);
            f.git(&["commit", "-q", "-m", msg]);
        };
        f.git(&["switch", "-q", "-c", "feat-x"]);
        commit("api.rs", API_RS, "add api");
        commit("x2.txt", "2\n", "x2");
        commit("x3.txt", "3\n", "x3");
        f.git(&["switch", "-q", "main"]);
        f.git(&["switch", "-q", "-c", "feat-y"]);
        commit("y1.txt", "1\n", "y1");
        commit("y2.txt", "2\n", "y2");
        f.git(&["switch", "-q", "main"]);
        commit("m.txt", "m\n", "m");
        f.git(&["switch", "-q", "-c", "ff"]);
        commit("ff.txt", "ff\n", "ff");
        f.git(&["switch", "-q", "main"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);

        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self {
            f,
            outside,
            remote,
            extra: extra.iter().map(|(k, v)| (*k, OsString::from(v))).collect(),
        };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["guard", "install", "--yes", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        m.wait_tm_ready();
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        let mut env = vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "es_ES.UTF-8".into()),
        ];
        env.extend(self.extra.iter().cloned());
        env
    }

    fn developer(&self, args: &[&str]) -> Output {
        developer_command(args)
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

    /// `git <args>` by the developer in the repo: no agent in the ancestry.
    fn human(&self, args: &str) -> Output {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(self.git_line(args))
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `git <args>` run by the simulated Claude Code in the repo.
    fn agent(&self, args: &str) -> Output {
        let line = self.git_line(args);
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
            .envs(self.env())
            .env(AGENT_CMD, line)
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    /// The tip of the local branch, `None` if it does not exist.
    fn branch(&self, name: &str) -> Option<String> {
        let out = self.human(&format!("rev-parse --verify -q refs/heads/{name}"));
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Commits of `branch` that no other branch has: `rev-list --count branch ^main`.
    fn own_commits(&self, branch: &str) -> usize {
        let out = self.ok(self.human(&format!("rev-list --count {branch} ^main")));
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    fn git_out(&self, args: &str) -> String {
        String::from_utf8_lossy(&self.ok(self.human(args)).stdout).into_owned()
    }

    fn remote_main(&self) -> String {
        let out = Command::new(&self.f.git)
            .arg("-C")
            .arg(&self.remote)
            .args(["rev-parse", "--verify", "-q", "refs/heads/main"])
            .env_clear()
            .envs(self.env())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
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

    /// The Time Machine took its first point of the repo (its store is seeded).
    fn wait_tm_ready(&self) {
        self.f.write("notes.txt", "ready\n");
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &repo_id)
                && snapshot_refs(&store).is_ok_and(|r| !r.is_empty())
            {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "the Time Machine took no point");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "daemon did not stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `raptor guard log --json` of "demo".
    fn log(&self) -> Value {
        let out = self.developer(&["guard", "log", "--json", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The pty of `script` may echo control characters before the output.
        let line = stdout
            .lines()
            .find_map(|l| l.find('{').map(|i| &l[i..]))
            .unwrap_or_else(|| panic!("no JSON: {stdout}"));
        serde_json::from_str(line.trim()).unwrap()
    }

    /// The log entry of a denial by `system.snapshot-failed`, once the daemon wrote it.
    fn snapshot_denial_entry(&self) -> Value {
        let start = Instant::now();
        loop {
            let log = self.log();
            let found = log["entries"].as_array().and_then(|entries| {
                entries.iter().find(|e| {
                    e["kind"] == "denial"
                        && e["reasons"].as_array().is_some_and(|rs| {
                            rs.iter().any(|r| r["rule"] == "system.snapshot-failed")
                        })
                })
            });
            if let Some(entry) = found {
                return entry.clone();
            }
            assert!(
                start.elapsed() < DEADLINE,
                "no system.snapshot-failed denial in the log: {log:#}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Every `hook-prior` row of the repo's oplog, read with the daemon stopped.
    fn hook_prior_rows(&self, repo_id: &str) -> Vec<SnapshotView> {
        self.stop();
        let (oplog, _) = Oplog::open(&self.dirs(), repo_id, 1).unwrap();
        oplog
            .snapshots(&SnapshotFilter {
                level: Some(SnapshotLevel::HookPrior),
                ..SnapshotFilter::default()
            })
            .unwrap()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

fn assert_no_goes_ahead_notice(shown: &str) {
    for fragment in GOES_AHEAD_NOTICE {
        assert!(
            !shown.contains(fragment),
            "the warning that lets the operation through appeared on a denial («{fragment}»): {shown}"
        );
    }
}

/// Escenario 1: with its prior point the allowed deletion of `feat-x` runs, and its 3 commits
/// are recoverable from that point (level `hook-prior`).
#[test]
fn scenario_1_allowed_branch_delete_runs_with_a_hook_prior() {
    let m = Machine::new(&[]);
    let repo_id = m.repo_id();
    let tip = m.branch("feat-x").unwrap();
    assert_eq!(m.own_commits("feat-x"), 3);
    let total: usize = m.git_out("rev-list --count feat-x").trim().parse().unwrap();

    m.ok(m.agent("branch -D feat-x"));
    assert_eq!(m.branch("feat-x"), None, "the deletion did not run");

    let rows = m.hook_prior_rows(&repo_id);
    let complete: Vec<&SnapshotView> = rows
        .iter()
        .filter(|s| s.state == SnapshotState::Complete)
        .collect();
    assert_eq!(complete.len(), 1, "{rows:#?}");
    let store = SnapshotStore::open_existing(&m.dirs(), &repo_id)
        .unwrap()
        .unwrap();
    let meta = store.meta(&complete[0].record.snapshot_id).unwrap();
    assert!(
        meta.branches.values().any(|t| *t == tip),
        "feat-x is not in the point: {:?}",
        meta.branches
    );
    let store_path = SnapshotStore::location(&m.dirs(), &repo_id).unwrap();
    let in_store = |args: &[&str]| {
        let out = Command::new(&m.f.git)
            .arg("--git-dir")
            .arg(&store_path)
            .args(args)
            .env_clear()
            .envs(m.env())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    assert_eq!(
        in_store(&["cat-file", "-p", &format!("{tip}:api.rs")]),
        API_RS.trim()
    );
    assert_eq!(
        in_store(&["rev-list", "--count", &tip]),
        total.to_string(),
        "the 3 commits of feat-x are in the point"
    );
}

/// Escenario 2, ejemplo "plazo": the prior's deadline is over, so the deletion does not run, the
/// reason says the time to save it ran out, and `feat-x` keeps its commits. The warning of
/// US-TMC-005 ("goes ahead") is not shown: it only belongs to an allowed operation.
#[test]
fn scenario_2_time_limit_denies_branch_delete_and_keeps_feat_x() {
    let m = Machine::new(&[(DEADLINE_ENV, "0")]);
    let tip = m.branch("feat-x").unwrap();

    let out = m.agent("branch -D feat-x");
    assert!(!out.status.success(), "the deletion ran: {}", text(&out));
    let shown = text(&out);
    assert!(shown.contains(WHY_TIME_LIMIT), "{shown}");
    assert_no_goes_ahead_notice(&shown);

    assert_eq!(m.branch("feat-x").as_deref(), Some(tip.as_str()));
    assert_eq!(m.own_commits("feat-x"), 3);
}

/// Escenario 2, ejemplo "GitRaptor parado" (D2, guard 5.1): without the daemon an allowed
/// branch deletion and an allowed rebase are denied, with both reasons (the snapshot one and the
/// degraded one) and "start GitRaptor and repeat the operation".
#[test]
fn scenario_2_degraded_mode_denies_branch_delete_and_rebase() {
    let m = Machine::new(&[]);
    let feat_x = m.branch("feat-x").unwrap();
    let feat_y = m.branch("feat-y").unwrap();
    m.stop();

    let out = m.agent("branch -D feat-x");
    assert!(!out.status.success(), "the deletion ran: {}", text(&out));
    let shown = text(&out);
    assert!(
        shown.contains(NOT_RUNNING),
        "snapshot-failed reason: {shown}"
    );
    assert!(shown.contains(HINT_START), "hint: {shown}");
    assert!(shown.contains(DEGRADED), "system.degraded reason: {shown}");
    assert_eq!(m.branch("feat-x").as_deref(), Some(feat_x.as_str()));

    let out = m.agent("rebase -q main feat-y");
    assert!(!out.status.success(), "the rebase ran: {}", text(&out));
    let shown = text(&out);
    assert!(
        shown.contains(NOT_RUNNING),
        "snapshot-failed reason: {shown}"
    );
    assert!(shown.contains(HINT_START), "hint: {shown}");
    assert!(shown.contains(DEGRADED), "system.degraded reason: {shown}");
    assert_eq!(m.branch("feat-y").as_deref(), Some(feat_y.as_str()));
    assert_eq!(m.own_commits("feat-y"), 2);
}

/// Escenario 2 / D9 (guard 4.1): the denial is an entry of `raptor guard log`, a `denial` with
/// the rule `system.snapshot-failed` and its cause, not an allow.
#[test]
fn scenario_2_denial_reaches_the_decision_log_with_its_cause() {
    let m = Machine::new(&[(DEADLINE_ENV, "0")]);
    let out = m.agent("branch -D feat-x");
    assert!(!out.status.success(), "{}", text(&out));

    let entry = m.snapshot_denial_entry();
    assert_eq!(entry["kind"], "denial", "{entry:#}");
    assert_eq!(entry["effect"], "deny", "{entry:#}");
    assert_eq!(entry["appliedEffect"], "deny", "{entry:#}");
    let reason = entry["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == "system.snapshot-failed")
        .unwrap();
    assert_eq!(reason["cause"], "snapshot-time-limit", "{entry:#}");
    assert_eq!(reason["level"], "system", "{entry:#}");
}

/// Escenario 3 (D10): a rebase of `feat-y` without a prior does not run, `feat-y` keeps its 2
/// commits, and with `--autostash` the working tree is the same as before.
#[test]
fn scenario_3_failed_prior_denies_rebase_and_feat_y_keeps_its_commits() {
    let m = Machine::new(&[(DEADLINE_ENV, "0")]);
    let feat_y = m.branch("feat-y").unwrap();

    let out = m.agent("rebase -q main feat-y");
    assert!(!out.status.success(), "the rebase ran: {}", text(&out));
    let shown = text(&out);
    assert!(shown.contains(WHY_TIME_LIMIT), "{shown}");
    assert_no_goes_ahead_notice(&shown);
    assert_eq!(m.branch("feat-y").as_deref(), Some(feat_y.as_str()));
    assert_eq!(m.own_commits("feat-y"), 2);

    // With --autostash: a dirty tracked file and an untracked one on feat-y.
    m.ok(m.human("switch -q feat-y"));
    m.f.write("y1.txt", "edited by hand\n");
    m.f.write("scratch.txt", "untracked\n");
    let status_before = m.git_out("status --porcelain");
    assert!(status_before.contains("y1.txt"), "{status_before}");

    let out = m.agent("rebase -q --autostash main");
    assert!(!out.status.success(), "the rebase ran: {}", text(&out));
    assert_eq!(m.branch("feat-y").as_deref(), Some(feat_y.as_str()));
    assert_eq!(m.own_commits("feat-y"), 2);
    assert_eq!(m.git_out("status --porcelain"), status_before);
    assert_eq!(
        std::fs::read_to_string(m.f.repo.join("y1.txt")).unwrap(),
        "edited by hand\n"
    );
    assert_eq!(
        std::fs::read_to_string(m.f.repo.join("scratch.txt")).unwrap(),
        "untracked\n"
    );
    assert!(
        m.git_out("stash list").trim().is_empty(),
        "the autostash was left behind"
    );
    assert!(
        !m.f.repo.join(".git/rebase-merge").exists()
            && !m.f.repo.join(".git/rebase-apply").exists(),
        "a rebase is left in progress"
    );
}

/// D4 (Q-GRD-38): the repo is still protected but the Time Machine no longer observes it: the
/// allowed deletion is denied and the reason says how to observe it again.
#[test]
fn unobserved_repo_denies_and_says_how_to_observe_it() {
    let m = Machine::new(&[]);
    let tip = m.branch("feat-x").unwrap();
    let out = m.developer(&["repo", "retire", m.f.repo.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));

    let out = m.agent("branch -D feat-x");
    assert!(!out.status.success(), "the deletion ran: {}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("raptor repo add"), "{shown}");
    assert_no_goes_ahead_notice(&shown);
    assert_eq!(m.branch("feat-x").as_deref(), Some(tip.as_str()));
}

/// D1, positive control: with the prior failing every time, a commit, creating a branch, a
/// fast-forward and a push still go through.
#[test]
fn non_destructive_operations_are_not_denied() {
    let m = Machine::new(&[(DEADLINE_ENV, "0")]);

    // fast-forward
    let ff = m.branch("ff").unwrap();
    m.ok(m.agent("merge -q --ff-only ff"));
    assert_eq!(m.branch("main").as_deref(), Some(ff.as_str()));

    // commit (with the trailer an agent's commit carries)
    let msg = m.outside.path().join("msg");
    std::fs::write(&msg, format!("feat: change\n\n{CLAUDE}\n")).unwrap();
    m.f.write("new.txt", "n\n");
    m.ok(m.human("add -- new.txt"));
    m.ok(m.agent(&format!("commit -q -F '{}'", msg.to_str().unwrap())));
    let main = m.branch("main").unwrap();
    assert_ne!(main, ff, "the commit did not run");

    // create a branch
    m.ok(m.agent("branch fresh"));
    assert_eq!(m.branch("fresh").as_deref(), Some(main.as_str()));

    // push
    m.ok(m.agent("push -q origin main"));
    assert_eq!(m.remote_main(), main);
}
