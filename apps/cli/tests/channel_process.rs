//! TS-GRP-004 at process level: the real `raptor` binary as daemon and as
//! client, over a temporary profile (`GITRAPTOR_PROFILE_DIR`, debug builds
//! only). Never this repo nor the real profile (NFR-01).
//!
//! The simulated agent is a copy of this test binary named
//! `raptor-fake-agent`, which runs a command and exits with its status (see
//! `fake_agent_entry`). The daemon is told, through the debug-only
//! `GITRAPTOR_AGENT_EXECUTABLES`, that this name is the agent, so the real
//! Claude Code session that may run these tests is not taken for one.
//!
//! macOS only: `script`, `nc -U` and `lsof` options are the macOS ones.
//! Linux: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::{AuditListResult, AuditOutcome, ClientKind, RefusalReason};
use gitraptor_api::methods;
use gitraptor_core::client::{Client, DAEMON_PATH};
use gitraptor_core::daemon::{LOG_FILE, running_pid};
use gitraptor_core::profile::{Profile, ProfileDirs};

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

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().join("profile")
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(self.root())
    }

    fn socket(&self) -> PathBuf {
        self.dirs().runtime.unwrap().join("raptor.sock")
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dirs().state.join(LOG_FILE)).unwrap_or_default()
    }

    /// Base environment of every process of the test: the temporary
    /// profile and the simulated agent's name.
    fn base_env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            ("GITRAPTOR_PROFILE_DIR", self.root().into_os_string()),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
        ]
    }

    fn raptor(&self, args: &[&str], extra: &[(&str, OsString)]) -> Command {
        let mut cmd = Command::new(RAPTOR);
        cmd.args(args)
            .env_clear()
            .envs(self.base_env())
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd
    }

    /// `raptor daemon status`, which starts the daemon on demand.
    fn status(&self, extra: &[(&str, OsString)]) -> Output {
        self.raptor(&["daemon", "status"], extra).output().unwrap()
    }

    fn daemon_pid(&self) -> u32 {
        running_pid(&self.dirs().state)
            .unwrap()
            .expect("daemon running")
    }

    /// The developer stops it: pty, not under an agent.
    fn stop_as_developer(&self) -> Output {
        let out = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR, "daemon", "stop", "--yes"])
            .env_clear()
            .envs(self.base_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        out
    }

    /// Runs `argv` as a child of the simulated agent.
    fn as_agent(&self, argv: &[&str]) -> Output {
        let agent = self.tmp.path().join(FAKE_AGENT);
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
            .envs(self.base_env())
            .env("PATH", "/usr/bin:/bin")
            .env(FAKE_AGENT_ARGV, serde_json::to_string(argv).unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn wait_released(&self) {
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Fixture {
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

/// Environment of a running process of this user, as `ps` shows it.
fn process_env(pid: u32) -> String {
    let out = Command::new("/bin/ps")
        .args(["eww", "-o", "command=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ------------------------------------------------------- on demand, SEC-10

/// With the daemon stopped, two clients at once start it and complete the
/// handshake; only one daemon runs.
#[test]
fn two_clients_at_once_start_a_single_daemon_on_demand() {
    let fx = Fixture::new();
    let a = fx.raptor(&["daemon", "status"], &[]).spawn().unwrap();
    let b = fx.raptor(&["daemon", "status"], &[]).spawn().unwrap();
    let (a, b) = (a.wait_with_output().unwrap(), b.wait_with_output().unwrap());
    assert!(a.status.success(), "{}", text(&a));
    assert!(b.status.success(), "{}", text(&b));
    let pid = fx.daemon_pid().to_string();
    assert!(text(&a).contains(&pid) && text(&b).contains(&pid));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        fx.log().matches("daemon_started").count(),
        1,
        "{}",
        fx.log()
    );
    fx.stop_as_developer();
    fx.wait_released();
}

/// A client with a hostile environment starts a daemon that inherits none
/// of it, in its own session.
#[test]
fn an_on_demand_daemon_does_not_inherit_the_client_environment() {
    let fx = Fixture::new();
    let fake_bin = fx.tmp.path().join("bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let hostile_home = fx.tmp.path().join("hostile-home");
    let hostile: Vec<(&str, OsString)> = vec![
        ("GIT_EXEC_PATH", "/tmp/evil-git".into()),
        (
            "GIT_CONFIG_PARAMETERS",
            "'core.fsmonitor'='/tmp/evil'".into(),
        ),
        ("LD_PRELOAD", "/tmp/evil.so".into()),
        ("DYLD_LIBRARY_PATH", "/tmp/evil-dyld".into()),
        ("XDG_CONFIG_HOME", "/tmp/evil-config".into()),
        ("AWS_SECRET_ACCESS_KEY", "s3cr3t-value".into()),
        ("HOME", hostile_home.clone().into_os_string()),
        (
            "PATH",
            format!(".:relative/bin:{}:/usr/bin:/bin", fake_bin.display()).into(),
        ),
    ];
    let out = fx.status(&hostile);
    assert!(out.status.success(), "{}", text(&out));
    let pid = fx.daemon_pid();
    let env = process_env(pid);
    for needle in [
        "GIT_EXEC_PATH",
        "GIT_CONFIG_PARAMETERS",
        "LD_PRELOAD",
        "DYLD_LIBRARY_PATH",
        "XDG_CONFIG_HOME",
        "s3cr3t-value",
        "hostile-home",
        "relative/bin",
    ] {
        assert!(!env.contains(needle), "{needle} reached the daemon: {env}");
    }
    assert!(env.contains(&format!("PATH={DAEMON_PATH}")), "{env}");
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        assert!(env.contains(&format!("HOME={home}")), "{env}");
    }
    // Its own session and process group: the client's exit cannot take it
    // down.
    let pgid = Command::new("/bin/ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&pgid.stdout).trim(),
        pid.to_string()
    );
    fx.stop_as_developer();
    fx.wait_released();
}

// ---------------------------------------------------------- SEC-01, NFR-03

/// The socket and its folder are private, and the daemon has no network
/// listener at all.
#[test]
fn the_channel_is_private_and_never_a_network_port() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    assert!(fx.status(&[]).status.success());
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&fx.socket()), 0o600);
    assert_eq!(mode(fx.socket().parent().unwrap()), 0o700);
    let pid = fx.daemon_pid().to_string();
    let lsof = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid, "-i"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&lsof.stdout).trim().is_empty(),
        "network sockets: {}",
        String::from_utf8_lossy(&lsof.stdout)
    );
    // The process does have the Unix socket (the check above is not vacuous).
    let unix = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid, "-U"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&unix.stdout).contains("raptor.sock"));
    fx.stop_as_developer();
    fx.wait_released();
}

#[test]
fn a_precreated_open_socket_folder_stops_the_daemon() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    let run = fx.dirs().runtime.unwrap();
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(fx.root(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = fx.raptor(&["daemon"], &[]).output().unwrap();
    assert!(!out.status.success());
    assert!(!fx.socket().exists());
}

// ------------------------------------------------------- SEC-03, SEC-13

/// `raptor daemon stop` from a process of the agent, the same under a pty,
/// and a hand-written JSON-RPC client, are all refused; the daemon keeps
/// running; each attempt is in the audit; the developer then stops it.
#[test]
fn reserved_commands_from_an_agent_are_refused_and_audited() {
    let fx = Fixture::new();
    assert!(fx.status(&[]).status.success());
    let pid = fx.daemon_pid();

    let direct = fx.as_agent(&[RAPTOR, "daemon", "stop", "--yes"]);
    assert!(!direct.status.success());
    assert!(
        text(&direct).contains("only the developer"),
        "{}",
        text(&direct)
    );

    let pty = fx.as_agent(&[
        "/usr/bin/script",
        "-q",
        "/dev/null",
        RAPTOR,
        "daemon",
        "stop",
        "--yes",
    ]);
    assert!(!pty.status.success());
    assert!(text(&pty).contains("only the developer"), "{}", text(&pty));

    let script = format!(
        "(printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\",\"params\":{{\"protocol\":{PROTOCOL_VERSION},\"client\":\"cli\",\"client_version\":\"x\"}}}}' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"daemon.stop\"}}'; sleep 1) | /usr/bin/nc -U {}",
        fx.socket().display()
    );
    let rpc = fx.as_agent(&["/bin/sh", "-c", &script]);
    let rpc_out = text(&rpc);
    assert!(rpc_out.contains("\"code\":-32003"), "{rpc_out}");
    assert!(rpc_out.contains("agent-ancestry"), "{rpc_out}");

    // Still running, same process.
    assert_eq!(fx.daemon_pid(), pid);
    let mut client = fx.client();
    let audit: AuditListResult = client
        .call(methods::AUDIT_LIST, serde_json::json!({}))
        .unwrap();
    assert_eq!(audit.entries.len(), 3, "{:?}", audit.entries);
    for entry in &audit.entries {
        assert_eq!(entry.operation, "daemon.stop");
        assert_eq!(entry.outcome, AuditOutcome::Rejected);
        assert_eq!(entry.reason, Some(RefusalReason::AgentAncestry));
        assert!(entry.client.agent_ancestor);
    }
    // The pty one had a controlling terminal and still was refused.
    assert!(audit.entries.iter().any(|e| e.client.controlling_terminal));
    drop(client);

    fx.stop_as_developer();
    fx.wait_released();
    let (profile, _) = Profile::open(fx.dirs()).unwrap();
    let rows = profile.audit(0, 10).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3].1.outcome, "accepted");
}

/// Without a terminal (and not under an agent) the stop is refused too.
#[test]
fn a_stop_without_a_controlling_terminal_is_refused() {
    let fx = Fixture::new();
    assert!(fx.status(&[]).status.success());
    let out = fx
        .raptor(&["daemon", "stop", "--yes"], &[])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out).contains("your own terminal"), "{}", text(&out));
    assert!(fx.log().contains("reason=no-controlling-terminal"));
    fx.stop_as_developer();
    fx.wait_released();
}

/// Without `--yes` and without a terminal to ask on, nothing is sent.
#[test]
fn stop_confirmation_needs_a_terminal() {
    let fx = Fixture::new();
    assert!(fx.status(&[]).status.success());
    let out = fx.raptor(&["daemon", "stop"], &[]).output().unwrap();
    assert!(!out.status.success());
    assert!(!fx.log().contains("reserved_command"));
    fx.stop_as_developer();
    fx.wait_released();
}

// ---------------------------------------------------------------- SEC-12

/// A repo whose path carries OSC escapes is printed without them.
#[test]
fn untrusted_repo_text_is_printed_sanitized() {
    let fx = Fixture::new();
    let repo = fx.tmp.path().join("evil\u{1b}]52;c;cHduZWQ=\u{7}repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    let common = PathBuf::from(git(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]));
    let (mut profile, _) = Profile::open(fx.dirs()).unwrap();
    profile.add_repo(&common, None, 1).unwrap();
    drop(profile);

    let out = fx.status(&[]);
    assert!(out.status.success(), "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("evil"), "{stdout}");
    assert!(
        !stdout.contains('\u{1b}') && !stdout.contains('\u{7}'),
        "{stdout:?}"
    );
    fx.stop_as_developer();
    fx.wait_released();
}

/// N5 (ADR-CKP-003 § 4) with the real binaries: `raptor daemon status`
/// says who the daemon sees and the layer it fixes, in English and in
/// Spanish; the developer's terminal gets `cockpit`, a process under an
/// agent gets `mcp`. It also shows the autostart state (N3).
#[test]
fn daemon_status_says_who_you_act_as() {
    let fx = Fixture::new();
    let developer = |lang: &str| {
        Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", RAPTOR, "daemon", "status"])
            .env_clear()
            .envs(fx.base_env())
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", lang)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let en = developer("en_US.UTF-8");
    assert!(en.status.success(), "{}", text(&en));
    assert!(
        text(&en).contains("you act as: no agent (layer cockpit)"),
        "{}",
        text(&en)
    );
    assert!(
        text(&en).contains("autostart at login: unknown"),
        "{}",
        text(&en)
    );
    let es = developer("es_ES.UTF-8");
    assert!(
        text(&es).contains("actúas como: sin agente (capa cockpit)"),
        "{}",
        text(&es)
    );
    assert!(
        text(&es).contains("autoarranque al iniciar sesión: desconocido"),
        "{}",
        text(&es)
    );

    let agent = fx.as_agent(&[RAPTOR, "daemon", "status"]);
    assert!(agent.status.success(), "{}", text(&agent));
    let out = text(&agent);
    assert!(out.contains("you act as: agent"), "{out}");
    assert!(out.contains("(layer mcp)"), "{out}");
}
