//! The temporary machine of one case: repos, canaries, the folders and links of `setup`, the
//! daemon state the developer would have built with the reserved commands, and the simulated
//! agent. Nothing here touches this repo, the real profile or the real Git config (NFR-01).

use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::ffi::OsString;
use std::hash::{BuildHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::exceptions::{Exception, Exceptions};
use gitraptor_testkit::fixture::{copy_executable, git_from_path};
use gitraptor_testkit::mcp_corpus::case::{CANARY_NAMES, Parent, RepoState, Roots};
use gitraptor_testkit::mcp_corpus::{Case, Secrets, Tier};
use serde_json::Value;

pub const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
pub const FAKE_AGENT: &str = "raptor-fake-agent";

/// How long the daemon has to show the repos the setup registered.
const SETUP_DEADLINE: Duration = Duration::from_secs(15);
/// Pause between polls of the daemon: starts short and doubles up to the cap.
const POLL_FIRST: Duration = Duration::from_millis(20);
const POLL_LAST: Duration = Duration::from_millis(250);

/// An environment as `(name, value)` pairs.
pub type Env = Vec<(String, OsString)>;

/// The environment variables that carry a canary, by canary name.
const ENV_CANARIES: [(&str, &str); 3] = [
    ("env-github-token", "GITHUB_TOKEN"),
    ("env-aws-secret", "AWS_SECRET_ACCESS_KEY"),
    ("env-anthropic-key", "ANTHROPIC_API_KEY"),
];

/// Parts of the planted remote that must never appear on the wire either.
const REMOTE_USER: &str = "canary-user";
const REMOTE_HOST: &str = "example.invalid";

/// The header line of a private key, in two parts so this file holds no key-like literal.
const PEM_HEADER: &str = concat!("-----BEGIN ", "OPENSSH PRIVATE KEY-----");

const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const UPPER_DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const TOKEN_SAFE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

/// `len` characters of `alphabet`, drawn from a per-run random state, the case and the canary.
fn draw(state: &RandomState, case_id: &str, name: &str, alphabet: &[u8], len: usize) -> String {
    (0..len)
        .map(|i| {
            let mut hasher = state.build_hasher();
            case_id.hash(&mut hasher);
            name.hash(&mut hasher);
            i.hash(&mut hasher);
            let pick = usize::try_from(hasher.finish() % alphabet.len() as u64).unwrap_or(0);
            char::from(alphabet[pick])
        })
        .collect()
}

/// A canary with the shape of a real secret, so the shape detector and the exact match both see
/// it.
fn canary_value(state: &RandomState, case_id: &str, name: &str) -> String {
    let body = |alphabet: &[u8], len: usize| draw(state, case_id, name, alphabet, len);
    match name {
        "env-github-token" => format!("ghp_{}", body(ALNUM, 36)),
        "env-aws-secret" => format!("AKIA{}", body(UPPER_DIGITS, 16)),
        "env-anthropic-key" => format!("sk-ant-{}", body(TOKEN_SAFE, 40)),
        "remote-userinfo" => body(ALNUM, 24),
        "remote-query" => format!("glpat-{}", body(ALNUM, 20)),
        "dotenv-file" => format!("xoxb-{}", body(ALNUM, 24)),
        "commit-message" => format!("{PEM_HEADER}{}", body(ALNUM, 24)),
        "git-extraheader" => format!("ghs_{}", body(ALNUM, 36)),
        other => panic!("no generator for the canary {other}"),
    }
}

/// `value` as one single-quoted shell word (only the Linux `script -qec` form needs it).
#[cfg(target_os = "linux")]
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `argv` of `raptor` inside a pseudo-terminal, as from the developer's own terminal.
#[cfg(target_os = "macos")]
fn pty_command(args: &[&str]) -> Command {
    let mut cmd = Command::new("/usr/bin/script");
    cmd.args(["-q", "/dev/null", RAPTOR]).args(args);
    cmd
}

#[cfg(target_os = "linux")]
fn pty_command(args: &[&str]) -> Command {
    let mut line = quote(RAPTOR);
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    let mut cmd = Command::new("script");
    cmd.args(["-qec", &line, "/dev/null"]);
    cmd
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn pty_command(args: &[&str]) -> Command {
    panic!("the engine tier needs a Unix pty ({args:?})");
}

/// One temporary machine, built for one case. The daemon, if one started, dies with it.
pub struct Machine {
    pub f: Fixture,
    canaries: Vec<(&'static str, String)>,
    repo_ids: BTreeMap<&'static str, String>,
    tier: Tier,
    path_trap: bool,
    agent: Option<PathBuf>,
}

impl Machine {
    /// Builds the machine the case's `setup` describes. A step that cannot be done is a harness
    /// failure, reported as a panic.
    pub fn new(case: &Case) -> Self {
        let f = Fixture::with_commit(&git_from_path());
        #[cfg(unix)]
        for dir in ["", "data", "config", "state"] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap_or_else(|e| panic!("cannot protect the profile: {e}"));
        }
        let state = RandomState::new();
        let canaries: Vec<(&'static str, String)> = CANARY_NAMES
            .iter()
            .map(|name| (*name, canary_value(&state, &case.id, name)))
            .collect();
        let mut machine = Self {
            f,
            canaries,
            repo_ids: BTreeMap::new(),
            tier: case.tier,
            path_trap: case.setup.path_trap,
            agent: None,
        };
        machine.seed();
        machine.build_setup(case);
        if case.tier == Tier::Engine {
            machine.register(case);
        }
        if case.session.parent == Parent::Agent {
            let agent = machine.f.root.join(FAKE_AGENT);
            let exe = std::env::current_exe()
                .unwrap_or_else(|e| panic!("cannot locate the test binary: {e}"));
            copy_executable(&exe, &agent);
            machine.agent = Some(agent);
        }
        machine
    }

    fn secret(&self, name: &str) -> &str {
        self.canaries
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("no canary named {name}"))
    }

    /// Plants the canaries that live on disk: in the working tree, the repo config and a commit.
    fn seed(&self) {
        let f = &self.f;
        f.write(
            ".env",
            &format!("API_TOKEN={}\n", self.secret("dotenv-file")),
        );
        let url = format!(
            "https://{REMOTE_USER}:{}@{REMOTE_HOST}/x.git?private_token={}",
            self.secret("remote-userinfo"),
            self.secret("remote-query")
        );
        f.git(&["remote", "add", "origin", &url]);
        f.git(&[
            "config",
            "http.extraHeader",
            &format!("Authorization: Bearer {}", self.secret("git-extraheader")),
        ]);
        let message = self.secret("commit-message");
        f.git(&["commit", "--allow-empty", "-q", "-m", message]);
        f.git_in(
            &f.other_repo,
            &["commit", "--allow-empty", "-q", "-m", message],
        );
    }

    pub fn roots(&self) -> Roots {
        Roots {
            root: self.f.root.clone(),
            repo: self.f.repo.clone(),
            other_repo: self.f.other_repo.clone(),
            home: self.f.home.clone(),
        }
    }

    /// Worktrees, folders, links and the `PATH` trap of the case.
    fn build_setup(&self, case: &Case) {
        let roots = self.roots();
        for tree in &case.setup.worktrees {
            self.f.git(&["branch", &tree.branch]);
            self.f.add_worktree(&tree.name, &tree.branch);
        }
        for dir in &case.setup.dirs {
            let path = dir.resolve(&roots);
            std::fs::create_dir_all(&path)
                .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
        }
        for link in &case.setup.symlinks {
            let at = link.at.resolve(&roots);
            let to = link.to.resolve(&roots);
            if let Some(parent) = at.parent() {
                std::fs::create_dir_all(parent)
                    .unwrap_or_else(|e| panic!("cannot create {}: {e}", parent.display()));
            }
            symlink(&to, &at);
        }
        if self.path_trap {
            self.arm_trap();
        }
    }

    /// `<root>/trap-bin/{git,raptor,raptor-mcp,sh}`: each one leaves a file in `<root>/trap-fired`
    /// if anything runs it.
    #[cfg(unix)]
    fn arm_trap(&self) {
        let bin = self.f.root.join("trap-bin");
        let fired = self.f.root.join("trap-fired");
        for dir in [&bin, &fired] {
            std::fs::create_dir_all(dir)
                .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
        }
        for name in ["git", "raptor", "raptor-mcp", "sh"] {
            gitraptor_testkit::canary::script(
                &bin.join(name),
                &format!(": > '{}'", fired.join(name).display()),
            );
        }
    }

    #[cfg(not(unix))]
    fn arm_trap(&self) {
        panic!("the PATH trap needs Unix");
    }

    /// What the developer did with the reserved commands, and the ids of the repos it created.
    fn register(&mut self, case: &Case) {
        let wanted = [
            ("repo", case.setup.repo, self.f.repo.clone()),
            (
                "other_repo",
                case.setup.other_repo,
                self.f.other_repo.clone(),
            ),
        ];
        let mut observed = 0;
        for (_, state, path) in &wanted {
            if *state == RepoState::None {
                continue;
            }
            let shown = path.to_string_lossy().into_owned();
            self.developer_ok(&["repo", "add", &shown]);
            observed += 1;
            if *state == RepoState::Enabled {
                self.developer_ok(&["mcp", "enable", &shown]);
            }
        }
        // The signal, not a clock: the daemon lists exactly the repos of the setup.
        if observed > 0 {
            self.wait_observed(observed);
        }
        for (label, state, path) in &wanted {
            if *state != RepoState::None {
                let id = self.repo_id(path);
                self.repo_ids.insert(label, id);
            }
        }
    }

    /// The environment of the daemon, the developer's commands and `raptor-mcp`.
    pub fn env(&self) -> Env {
        let mut env: Env = vec![
            (
                "GITRAPTOR_PROFILE_DIR".into(),
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES".into(), FAKE_AGENT.into()),
        ];
        if cfg!(unix) {
            env.push(("PATH".into(), "/usr/bin:/bin".into()));
        }
        for (canary, var) in ENV_CANARIES {
            env.push((var.into(), self.secret(canary).into()));
        }
        env
    }

    /// The developer, from their own terminal (a pty, not under an agent).
    fn developer_ok(&self, args: &[&str]) {
        let out = pty_command(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|e| panic!("cannot run the pty helper: {e}"));
        assert!(
            out.status.success(),
            "raptor {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The repos `raptor status --json` lists, or `None` while it does not answer yet.
    fn observed_repos(&self, deadline: Instant) -> Option<Vec<Value>> {
        let stdout = self.raptor_until(&["status", "--json"], deadline)?;
        let status: Value = serde_json::from_slice(&stdout).ok()?;
        status["repos"].as_array().cloned()
    }

    /// Stdout of `raptor args` if it ends successfully before `deadline`; the process is killed
    /// when the deadline comes first.
    fn raptor_until(&self, args: &[&str], deadline: Instant) -> Option<Vec<u8>> {
        let mut child = Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot run raptor: {e}"));
        // Drained on its own thread so a full pipe cannot keep the child from ending.
        let Some(mut pipe) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
            buf
        });
        let mut pause = POLL_FIRST;
        let ended = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(
                        pause.min(deadline.saturating_duration_since(Instant::now())),
                    );
                    pause = (pause * 2).min(POLL_LAST);
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
            }
        };
        let stdout = reader.join().unwrap_or_default();
        ended.then_some(stdout)
    }

    fn wait_observed(&self, expected: usize) {
        let deadline = Instant::now() + SETUP_DEADLINE;
        let mut pause = POLL_FIRST;
        loop {
            let seen = self.observed_repos(deadline).map(|repos| repos.len());
            if seen == Some(expected) {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "the daemon lists {seen:?} repos, the setup made {expected}"
            );
            // A growing pause between polls, never past the deadline.
            std::thread::sleep(pause.min(left));
            pause = (pause * 2).min(POLL_LAST);
        }
    }

    fn repo_id(&self, path: &Path) -> String {
        let want = gitraptor_core::observe::canonical(path);
        let repos = self
            .observed_repos(Instant::now() + SETUP_DEADLINE)
            .unwrap_or_else(|| panic!("raptor status gave no repos"));
        repos
            .iter()
            .find(|repo| {
                repo["worktrees"].as_array().is_some_and(|trees| {
                    trees
                        .iter()
                        .any(|t| t["path"].as_str().map(Path::new) == Some(want.as_path()))
                })
            })
            .and_then(|repo| repo["repo_id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("{} is not an observed repo", path.display()))
    }

    /// The values behind the `{...}` markers of the case.
    pub fn vars(&self) -> BTreeMap<String, String> {
        let shown = |p: &Path| p.to_string_lossy().into_owned();
        let mut vars = BTreeMap::from([
            ("root".to_owned(), shown(&self.f.root)),
            ("repo".to_owned(), shown(&self.f.repo)),
            ("other_repo".to_owned(), shown(&self.f.other_repo)),
            ("home".to_owned(), shown(&self.f.home)),
        ]);
        for (label, id) in &self.repo_ids {
            vars.insert(format!("repo_id:{label}"), id.clone());
        }
        for (name, value) in &self.canaries {
            vars.insert(format!("canary:{name}"), value.clone());
        }
        vars
    }

    /// Everything planted that must never reach the wire, with its name.
    pub fn secrets(&self) -> Secrets {
        let mut all: Vec<(String, String)> = self
            .canaries
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
        for own in [REMOTE_USER, REMOTE_HOST] {
            all.push((own.to_owned(), own.to_owned()));
        }
        Secrets(all)
    }

    /// The differences the fingerprint may show: the engine's own folders, and only for `engine`.
    /// A `server` case admits none (nothing may touch the profile).
    pub fn exceptions(&self) -> Exceptions {
        match self.tier {
            Tier::Server => Exceptions::none(),
            // The daemon's runtime folder (its socket) appears with the first connection, and the
            // profile root's times change with it: the case that observes nothing starts the
            // daemon itself.
            Tier::Engine => Exceptions::engine_profile("profile")
                .with(Exception::Subtree {
                    scope: "profile".into(),
                    prefix: "run".into(),
                })
                .with(Exception::DirTimes {
                    scope: "profile".into(),
                    path: PathBuf::new(),
                }),
        }
    }

    /// The simulated agent, when the case runs under one.
    pub fn agent(&self) -> Option<&Path> {
        self.agent.as_deref()
    }

    /// Names of the trap programs that ran.
    pub fn traps_fired(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.f.root.join("trap-fired")) else {
            return Vec::new();
        };
        let mut fired: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        fired.sort();
        fired
    }
}

#[cfg(unix)]
fn symlink(to: &Path, at: &Path) {
    std::os::unix::fs::symlink(to, at)
        .unwrap_or_else(|e| panic!("cannot link {}: {e}", at.display()));
}

#[cfg(not(unix))]
fn symlink(_to: &Path, _at: &Path) {
    panic!("symlinks need Unix");
}

impl Drop for Machine {
    fn drop(&mut self) {
        // The daemon is detached: never leave one behind.
        let dirs = ProfileDirs::under_root(&self.f.profile);
        if let Ok(Some(pid)) = running_pid(&dirs.state) {
            kill(pid);
        }
    }
}

#[cfg(unix)]
fn kill(pid: impl ToString) {
    let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
}

#[cfg(windows)]
fn kill(pid: impl ToString) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status();
}
