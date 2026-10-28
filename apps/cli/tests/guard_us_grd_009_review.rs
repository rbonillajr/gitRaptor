//! Regression tests for the review fixes of DS-US-GRD-009 § 10, end to end: a commit parked
//! under remote-tracking refs and a local `git fetch` into a branch are still evaluated. Same
//! harness as `guard_us_grd_009.rs`: real `raptor` as daemon and hook, plain Git, temporary repos
//! and profile (NFR-01); the agent is `raptor-fake-agent`.
//!
//! Unix only (no channel transport on Windows yet).
#![cfg(unix)]

use std::ffi::OsString;
use std::process::{Command, Output, Stdio};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{copy_executable, git_from_path};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
const DIFF_400: &str = r#"{"policies":{"diffSize":{"maxLines":400}}}"#;

/// Entry point of the simulated Claude Code (see `guard_us_grd_009.rs`).
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

fn lines(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

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

/// "demo" with `main`, a bare remote, the team settings on `main`, the repo protected; the work
/// branch is `feat-x`.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
}

impl Machine {
    fn new(settings: &str) -> Self {
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
        f.write(".gitraptor/settings.json", settings);
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "seed and team settings"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.git(&["push", "-q", "origin", "feat-x"]);
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self { f, outside };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["guard", "install", "--yes", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        m
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

    fn developer(&self, args: &[&str]) -> Output {
        developer_command(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn git_path(&self) -> String {
        self.f.git.to_str().unwrap().to_owned()
    }

    /// `git <args>` by the developer: no agent in the ancestry.
    fn human(&self, args: &str) -> Output {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("'{}' {args}", self.git_path()))
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.repo)
            .output()
            .unwrap()
    }

    /// A shell line run by the simulated Claude Code.
    fn agent_sh(&self, line: &str) -> Output {
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

    fn rev(&self, what: &str) -> String {
        let out = self.human(&format!("rev-parse {what}"));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// The agent builds a 1 200-line commit on top of `feat-x` with plumbing and tries to park
    /// it under `park` (a ref outside `refs/heads`). Returns the commit id and the outcome of
    /// the `update-ref`. Nothing here touches a branch.
    fn try_park(&self, park: &str) -> (String, Output) {
        self.f.write("big.txt", &lines(1200));
        let g = self.git_path();
        let out = self.agent_sh(&format!(
            "G='{g}'; $G add big.txt && T=$($G write-tree) && \
             C=$($G commit-tree $T -p HEAD -m 'feat: parked') && echo $C > .git/parked && \
             $G update-ref {park} $C"
        ));
        let oid = std::fs::read_to_string(self.f.repo.join(".git/parked"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        assert_eq!(oid.len(), 40, "the commit was built: {}", text(&out));
        (oid, out)
    }

    /// Like [`Self::try_park`] for a ref the policies do not judge: it must succeed.
    fn park_oversized(&self, park: &str) -> String {
        let (oid, out) = self.try_park(park);
        assert!(
            out.status.success(),
            "parking under {park} is not a branch update: {}",
            text(&out)
        );
        oid
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// A commit created by plumbing and parked under `refs/remotes/x/y` is judged at the park step
/// like a branch line, and landing it afterwards (`branch -f`, `merge --ff-only`) is denied too.
#[test]
fn a_commit_parked_under_remote_tracking_refs_is_still_evaluated() {
    let m = Machine::new(DIFF_400);
    let (oid, out) = m.try_park("refs/remotes/x/y");
    assert!(
        !out.status.success(),
        "parking a 1200-line commit under refs/remotes must be denied: {}",
        text(&out)
    );
    assert!(text(&out).contains("1200"), "{}", text(&out));
    assert!(
        !m.human("rev-parse --verify -q refs/remotes/x/y")
            .status
            .success(),
        "the parked ref must not exist"
    );

    let g = m.git_path();
    // `feat-x` is checked out, so `branch -f` targets another branch.
    assert!(m.human("branch other main").status.success());
    let other = m.rev("other");
    let out = m.agent_sh(&format!("'{g}' branch -f other {oid}"));
    assert!(
        !out.status.success(),
        "landing a parked 1200-line commit with `branch -f` must be denied: {}",
        text(&out)
    );
    assert!(text(&out).contains("1200"), "{}", text(&out));
    assert_eq!(m.rev("other"), other, "the branch did not move (branch -f)");

    // The same commit through a merge onto the checked-out branch.
    let tip = m.rev("feat-x");
    let out = m.agent_sh(&format!("'{g}' merge -q --ff-only {oid}"));
    assert!(
        !out.status.success(),
        "merging a parked 1200-line commit must be denied: {}",
        text(&out)
    );
    assert_eq!(m.rev("feat-x"), tip, "the branch did not move (merge)");
}

/// `git fetch . refs/tmp/x:refs/heads/feat` writes a branch from a local ref: it is not exempt.
#[test]
fn a_local_fetch_into_a_branch_is_evaluated() {
    let m = Machine::new(DIFF_400);
    m.park_oversized("refs/tmp/x");

    let g = m.git_path();
    let out = m.agent_sh(&format!("'{g}' fetch -q . refs/tmp/x:refs/heads/feat"));
    assert!(
        !out.status.success(),
        "a local fetch of a 1200-line commit into a branch must be denied: {}",
        text(&out)
    );
    assert!(text(&out).contains("1200"), "{}", text(&out));
    let created = m.human("rev-parse --verify -q refs/heads/feat");
    assert!(
        !created.status.success(),
        "the branch must not exist: {}",
        text(&created)
    );
}
