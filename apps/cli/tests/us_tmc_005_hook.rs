//! US-TMC-005 end to end (Brief US-TMC-005): with the Guardrails hooks installed, a raw Git
//! operation the hooks see before its effects (a branch deletion, a rebase) is preceded by a
//! `hook-prior` point; a failed point is never shown as one; a query takes no point.
//!
//! The real `raptor` as daemon and hook client, the real dispatcher and plain Git, over a
//! temporary repo and a temporary profile (NFR-01). The agent is `raptor-fake-agent`, a copy of
//! this test binary the debug daemon knows as Claude Code (`agent → sh → git → hook`). No fixed
//! waits: every state is awaited with a deadline.
//!
//! Unix only, as `guard_us_grd_008.rs` (Windows: pending cross-platform validation stage).
#![cfg(unix)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, GitEventKind, Snapshot};
use gitraptor_api::timemachine::{EntryOrigin, ProtectionLevel, TimelineEntry, TimelineResult};
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
use serde_json::json;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The command the simulated agent runs with `/bin/sh`.
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
/// The hook prior's deadline override of the debug daemon.
const DEADLINE_ENV: &str = "GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS";
const DEADLINE: Duration = Duration::from_secs(60);
const API_RS: &str = "pub fn api() -> u32 { 42 }\n";
/// What the hook writes when no prior snapshot was saved (`hookprior.failed`, English).
const FAILED_NOTICE: &str = "no prior snapshot was saved";

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

/// "demo" with `main`; `feat-x` with a commit that adds `api.rs` and is on no other branch;
/// `feat-y` with its own commit behind a newer `main` (to rebase). Observed and protected by
/// `raptor guard install`, with the Time Machine ready.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    extra: Vec<(&'static str, OsString)>,
    feat_x_tip: String,
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
        // feat-x: api.rs only there.
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.write("api.rs", API_RS);
        f.git(&["add", "api.rs"]);
        f.git(&["commit", "-q", "-m", "add api"]);
        let feat_x_tip = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
        f.git(&["switch", "-q", "main"]);
        // feat-y: one commit, behind a newer main.
        f.git(&["switch", "-q", "-c", "feat-y"]);
        f.write("y.txt", "y\n");
        f.git(&["add", "y.txt"]);
        f.git(&["commit", "-q", "-m", "y"]);
        f.git(&["switch", "-q", "main"]);
        f.write("m.txt", "m\n");
        f.git(&["add", "m.txt"]);
        f.git(&["commit", "-q", "-m", "m"]);

        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self {
            f,
            outside,
            extra: extra.iter().map(|(k, v)| (*k, OsString::from(v))).collect(),
            feat_x_tip,
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
            ("LANG", "en_US.UTF-8".into()),
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
        self.agent_in(&self.f.repo, args)
    }

    /// `git <args>` run by the simulated Claude Code in `dir`.
    fn agent_in(&self, dir: &std::path::Path, args: &str) -> Output {
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
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
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

    /// The Time Machine took its first point of the repo (its store is seeded): an untracked
    /// note is written and a snapshot is awaited.
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

    /// The timeline entry of the first raw Git event of `kind`, once the engine recorded it.
    fn entry(&self, kind: GitEventKind) -> TimelineEntry {
        let start = Instant::now();
        loop {
            let t: TimelineResult = self
                .client()
                .call(
                    methods::TM_TIMELINE,
                    json!({ "worktree": self.f.repo.to_str().unwrap() }),
                )
                .unwrap();
            if let Some(e) = t
                .entries
                .into_iter()
                .find(|e| matches!(&e.origin, EntryOrigin::GitEvent { kind: k, .. } if *k == kind))
            {
                return e;
            }
            assert!(start.elapsed() < DEADLINE, "no {kind:?} in the timeline");
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

    fn store_path(&self, repo_id: &str) -> PathBuf {
        SnapshotStore::location(&self.dirs(), repo_id).unwrap()
    }

    /// `api.rs` of the commit `tip`, read from the Time Machine's store.
    fn api_rs_in_store(&self, repo_id: &str, tip: &str) -> String {
        let store = self.store_path(repo_id);
        let out = Command::new(&self.f.git)
            .arg("--git-dir")
            .arg(&store)
            .args(["cat-file", "-p", &format!("{tip}:api.rs")])
            .env_clear()
            .envs(self.env())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// The single complete `hook-prior` point, which must keep `feat-x` and its `api.rs`.
    fn assert_feat_x_kept(&self, repo_id: &str, rows: &[SnapshotView]) {
        let complete: Vec<&SnapshotView> = rows
            .iter()
            .filter(|s| s.state == SnapshotState::Complete)
            .collect();
        assert_eq!(complete.len(), 1, "{rows:#?}");
        let store = SnapshotStore::open_existing(&self.dirs(), repo_id)
            .unwrap()
            .unwrap();
        let meta = store.meta(&complete[0].record.snapshot_id).unwrap();
        assert!(
            meta.branches.values().any(|tip| *tip == self.feat_x_tip),
            "feat-x is not in the point: {:?}",
            meta.branches
        );
        assert_eq!(self.api_rs_in_store(repo_id, &self.feat_x_tip), API_RS);
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// Escenario 1 (reworded, pending PO): an agent deletes `feat-x` with raw Git; a recoverable
/// point from before the deletion holds `api.rs` of `feat-x`, at level `hook-prior`.
#[test]
fn scenario_1_branch_delete_keeps_api_rs_in_a_hook_prior() {
    let m = Machine::new(&[]);
    let repo_id = m.repo_id();
    let out = m.ok(m.agent("branch -D feat-x"));
    assert!(!text(&out).contains(FAILED_NOTICE), "{}", text(&out));
    let mut gone =
        m.f.git_command(&m.f.repo, &["rev-parse", "--verify", "-q", "feat-x"]);
    assert!(
        !gone.output().unwrap().status.success(),
        "feat-x still exists"
    );
    let rows = m.hook_prior_rows(&repo_id);
    m.assert_feat_x_kept(&repo_id, &rows);
}

/// Escenario 1: the point shows as "snapshot previo" on the operation it protects.
#[test]
fn scenario_1_rebase_entry_shows_hook_prior() {
    let m = Machine::new(&[]);
    let out = m.ok(m.agent("rebase -q main feat-y"));
    assert!(!text(&out).contains(FAILED_NOTICE), "{}", text(&out));
    let entry = m.entry(GitEventKind::Rebase);
    assert_eq!(
        entry.protection.level,
        ProtectionLevel::HookPrior,
        "{entry:#?}"
    );
    assert!(entry.protection.snapshot_id.is_some());
}

/// Escenario 3: the prior snapshot fails (its deadline is over at once). The operation goes
/// ahead with a notice, and it never shows as protected by a prior snapshot.
#[test]
fn scenario_3_failed_prior_is_not_shown_as_prior() {
    let m = Machine::new(&[(DEADLINE_ENV, "0")]);
    let repo_id = m.repo_id();
    let out = m.ok(m.agent("rebase -q main feat-y"));
    assert!(
        text(&out).contains(FAILED_NOTICE),
        "the hook did not say the prior failed: {}",
        text(&out)
    );
    let entry = m.entry(GitEventKind::Rebase);
    assert_ne!(
        entry.protection.level,
        ProtectionLevel::HookPrior,
        "{entry:#?}"
    );
    let rows = m.hook_prior_rows(&repo_id);
    assert!(
        rows.iter().all(|s| s.state != SnapshotState::Complete),
        "{rows:#?}"
    );
}

/// Escenario 4: queries that do not modify the repo take no point. Control: the deletion that
/// follows takes exactly one.
#[test]
fn scenario_4_read_only_queries_take_no_point() {
    let m = Machine::new(&[]);
    let repo_id = m.repo_id();
    for query in [
        "status --porcelain",
        "log --oneline -n 3",
        "diff",
        "branch --list",
        "rev-parse HEAD",
        "show --stat HEAD",
        "tag --list",
        "for-each-ref refs/heads",
    ] {
        m.ok(m.agent(query));
    }
    m.ok(m.agent("branch -D feat-x"));
    let rows = m.hook_prior_rows(&repo_id);
    assert_eq!(rows.len(), 1, "only the deletion takes a point: {rows:#?}");
    m.assert_feat_x_kept(&repo_id, &rows);
}

/// TMC005-UNTRUSTED (#226): the hook runs in a folder whose `.git` names a worktree of the
/// protected repo without being it (the repo does not register it). The daemon resolves the
/// worktree itself and refuses it: no point is taken, and the operation still goes ahead (the
/// hook does not break). Control: the same deletion from the repo itself takes exactly one.
#[test]
fn untrusted_worktree_hook_takes_no_point_and_the_operation_proceeds() {
    let m = Machine::new(&[]);
    let repo_id = m.repo_id();
    let wt = m.f.root.join("wt-a");
    m.ok(m.human(&format!("worktree add -q '{}' -b a", wt.to_str().unwrap())));
    m.ok(m.human("branch feat-z feat-y"));
    let posing = m.f.root.join("posing");
    std::fs::create_dir_all(&posing).unwrap();
    std::fs::write(
        posing.join(".git"),
        std::fs::read_to_string(wt.join(".git")).unwrap(),
    )
    .unwrap();

    m.ok(m.agent_in(&posing, "branch -D feat-x"));
    let mut gone =
        m.f.git_command(&m.f.repo, &["rev-parse", "--verify", "-q", "feat-x"]);
    assert!(
        !gone.output().unwrap().status.success(),
        "feat-x still exists"
    );

    m.ok(m.agent("branch -D feat-z"));
    let rows = m.hook_prior_rows(&repo_id);
    assert_eq!(
        rows.len(),
        1,
        "only the deletion from the repo takes a point: {rows:#?}"
    );
    let store = SnapshotStore::open_existing(&m.dirs(), &repo_id)
        .unwrap()
        .unwrap();
    let meta = store.meta(&rows[0].record.snapshot_id).unwrap();
    assert!(
        !meta.branches.values().any(|tip| *tip == m.feat_x_tip),
        "the point is from before the untrusted deletion: {:?}",
        meta.branches
    );
}
