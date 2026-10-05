//! US-GRP-017 end to end: each Gherkin scenario of `raptor status
//! --resources` with the real `raptor` binary as daemon and as client, over
//! a temporary machine built by the "intact repo" harness (INF-GRP-001):
//! temporary repo and profile, never this repo nor the real profile
//! (NFR-01). No fixed waits: the tests wait for the state they need.
//!
//! macOS only: `script` options are the macOS ones. Linux and Windows:
//! Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exception, Exceptions, Fixture, check};
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const TARGETS_ENV: &str = "GITRAPTOR_RESOURCE_TARGETS";

struct Machine {
    f: Fixture,
    /// Linked worktrees of "demo".
    worktrees: Vec<PathBuf>,
}

impl Machine {
    /// "demo" with 3 worktrees: the main one and two linked ones. Its
    /// files hold a marker that must never reach the output.
    fn demo() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "SECRET-CONTENT-OF-THE-REPO\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-a"]);
        f.git(&["branch", "feat-b"]);
        let worktrees = vec![
            f.add_worktree("feat-a", "feat-a"),
            f.add_worktree("feat-b", "feat-b"),
        ];
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        Self { f, worktrees }
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
            // No real agent classifies this test's processes as an agent.
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
        ]
    }

    /// `raptor <args>` without a terminal.
    fn raptor(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The developer, from their own terminal (reserved commands).
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

    /// Adds "demo"; the on-demand daemon gets `extra` in its environment.
    fn add_demo(&self, extra: &[(&str, &str)]) {
        let out = self.developer(&["repo", "add", self.f.repo.to_str().unwrap()], extra);
        assert!(out.status.success(), "{}", text(&out));
    }

    fn resources_json(&self) -> Value {
        let out = self.raptor(&["status", "--resources", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Waits until the observer watches the 3 worktrees: their watches
    /// start after the add returns.
    fn wait_for_watches(&self, roots: u64) {
        let start = Instant::now();
        loop {
            let v = self.resources_json();
            if v["engine"]["watches"]["roots"] == roots {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{v}");
            std::thread::sleep(Duration::from_millis(50));
        }
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

fn folder(path: &Path) -> String {
    path.canonicalize().unwrap().to_str().unwrap().to_owned()
}

/// The line that starts with `label`.
fn line<'a>(shown: &'a str, label: &str) -> &'a str {
    shown
        .lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} line in:\n{shown}"))
}

/// The engine's own data, the only difference measuring may show outside
/// the repo.
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

// ------------------------------------------------------------ Escenario 1

/// El desarrollador ve el consumo del motor.
#[test]
fn the_developer_sees_the_engine_consumption() {
    let m = Machine::demo();
    let report = check("US-GRP-017 measure", &m.f, &engine_profile(), || {
        m.add_demo(&[]);
        m.wait_for_watches(3);
        let out = m.raptor(&["status", "--resources"]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let shown = String::from_utf8_lossy(&out.stdout).into_owned();

        assert!(shown.starts_with("engine: running (pid "), "{shown}");
        // CPU: mean over its window and peak, against its target, judged
        // either way (a young daemon may be over it).
        let cpu = line(&shown, "CPU: ");
        assert!(
            cpu.contains("average over") && cpu.contains("(peak "),
            "{cpu}"
        );
        assert!(
            cpu.ends_with("target < 1.00 %: ok") || cpu.ends_with("OVER TARGET (target < 1.00 %)"),
            "{cpu}"
        );
        assert!(
            line(&shown, "memory (RSS): ").ends_with("target < 150.0 MiB: ok"),
            "{shown}"
        );
        assert!(
            line(&shown, "open descriptors: ").ends_with("target ≤ 256: ok"),
            "{shown}"
        );
        assert_eq!(
            line(&shown, "watches: "),
            "watches: 3 watched folders — no target"
        );
        assert!(
            line(&shown, "profile disk (without the Time Machine): ")
                .ends_with("target ≤ 250.0 MiB: ok"),
            "{shown}"
        );
        assert!(
            line(&shown, "Time Machine disk: ")
                .ends_with("reference 10.0 GiB (the cap is not enforced yet)"),
            "{shown}"
        );
        let demo = format!("  {}: ", folder(&m.f.repo));
        assert!(shown.lines().any(|l| l.starts_with(&demo)), "{shown}");
        assert!(shown.contains("work classes: not available"), "{shown}");
        assert!(shown.contains("power saving: not available"), "{shown}");
        assert!(!shown.contains("SECRET-CONTENT"), "{shown}");
        for wt in &m.worktrees {
            assert!(wt.exists());
        }
        m.stop();
    });
    report.assert_intact();
}

// ------------------------------------------------------------ Escenario 2

/// Un valor fuera de objetivo se señala, sin cambiar el código de salida.
#[test]
fn a_value_over_target_is_flagged() {
    let m = Machine::demo();
    // The daemon starts with an RSS target of 1 byte (debug-only hook).
    m.add_demo(&[(TARGETS_ENV, "rss_bytes=1")]);
    let out = m.raptor(&["status", "--resources"]);
    let shown = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(
        out.status.code(),
        Some(0),
        "same exit code as within target: {}",
        text(&out)
    );
    let rss = line(&shown, "memory (RSS): ");
    assert!(rss.ends_with("OVER TARGET (target < 1 B)"), "{rss}");
    // Everything else is still judged against its real target.
    assert!(
        line(&shown, "open descriptors: ").ends_with("target ≤ 256: ok"),
        "{shown}"
    );
    let v = m.resources_json();
    assert_eq!(v["engine"]["rss_bytes"]["within"], false, "{v}");
    assert_eq!(v["engine"]["rss_bytes"]["target"], 1, "{v}");

    // In Spanish too.
    let out = Command::new(RAPTOR)
        .args(["status", "--resources"])
        .env_clear()
        .envs(m.env())
        .env("LANG", "es_ES.UTF-8")
        .output()
        .unwrap();
    let shown = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        line(&shown, "memoria (RSS): ").ends_with("FUERA DE OBJETIVO (objetivo < 1 B)"),
        "{shown}"
    );
    assert_eq!(out.status.code(), Some(0));
    m.stop();
}

// ------------------------------------------------------------ Escenario 3

/// Every string of the JSON output, with its path.
fn strings(v: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::String(s) => out.push((path.to_owned(), s.clone())),
        Value::Array(a) => a.iter().for_each(|v| strings(v, &format!("{path}[]"), out)),
        Value::Object(o) => {
            for (k, v) in o {
                strings(v, &format!("{path}.{k}"), out);
            }
        }
        _ => {}
    }
}

/// Salida para scripts: unidades fijas y ningún texto de presentación.
#[test]
fn json_output_has_fixed_units_and_no_presentation_text() {
    let m = Machine::demo();
    m.add_demo(&[]);
    m.wait_for_watches(3);
    let v = m.resources_json();
    let e = &v["engine"];
    assert_eq!(v["running"], true);
    assert!(e["cpu"]["mean_pct"].is_f64(), "{v}");
    assert!(e["cpu"]["peak_pct"].is_f64(), "{v}");
    assert!(e["cpu"]["window_s"].is_u64(), "{v}");
    assert_eq!(e["cpu"]["target_pct"], 1.0);
    assert!(e["cpu"]["within"].is_boolean());
    assert!(
        e["rss_bytes"]["value"].as_u64().unwrap() > 1024 * 1024,
        "{v}"
    );
    assert_eq!(e["rss_bytes"]["target"], 150 * 1024 * 1024);
    assert!(e["open_fds"]["value"].as_u64().unwrap() > 3, "{v}");
    assert_eq!(e["open_fds"]["target"], 256);
    assert_eq!(e["watches"]["roots"], 3);
    assert!(e["watches"]["inotify"].is_null());
    assert!(e["pools"].is_null() && e["power_saving"].is_null());
    let d = &v["disk"];
    assert_eq!(d["complete"], true);
    assert!(d["profile_bytes"]["value"].as_u64().unwrap() > 0);
    assert_eq!(d["profile_bytes"]["target"], 250 * 1024 * 1024);
    let repos = d["time_machine"]["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1, "{v}");
    assert!(repos[0]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(
        d["time_machine"]["bytes"], repos[0]["bytes"],
        "the total is the sum of the repos"
    );
    assert!(d["time_machine"]["within"].is_null());

    // The only strings: the repo's id and its Git directory.
    let mut found = Vec::new();
    strings(&v, "", &mut found);
    let paths: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            ".disk.time_machine.repos[].path",
            ".disk.time_machine.repos[].repo_id"
        ]
    );
    assert_eq!(found[0].1, folder(&m.f.repo.join(".git")));
    assert!(!v.to_string().contains("SECRET-CONTENT"));
    m.stop();
}

// ------------------------------------------------------------ Escenario 4

/// Sin el motor en marcha no se arranca solo para medir.
#[test]
fn without_the_engine_it_does_not_start_it() {
    let m = Machine::demo();
    m.add_demo(&[]);
    m.stop();
    assert!(running_pid(&m.dirs().state).unwrap().is_none());

    let out = m.raptor(&["status", "--resources"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let shown = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(shown.starts_with("engine: not running"), "{shown}");
    assert!(!shown.contains("CPU"), "{shown}");
    assert!(
        line(&shown, "profile disk (without the Time Machine): ").ends_with(": ok"),
        "{shown}"
    );
    line(&shown, "Time Machine disk: ");
    // The repo's name comes from the profile's index, read-only.
    let demo = format!("  {}: ", folder(&m.f.repo));
    assert!(shown.lines().any(|l| l.starts_with(&demo)), "{shown}");

    let v = m.resources_json();
    assert_eq!(v["running"], false);
    assert!(v["engine"].is_null());
    assert!(v["disk"]["time_machine"]["bytes"].as_u64().unwrap() > 0);

    // Neither call started the engine.
    assert!(running_pid(&m.dirs().state).unwrap().is_none());
    assert!(matches!(
        Client::connect(&m.dirs(), ClientKind::Cli, PROTOCOL_VERSION),
        Err(ClientError::NotRunning)
    ));
}

// ------------------------------------------------------------ Escenario 5

/// Un agente no ve el consumo por MCP: the method does not exist for an
/// MCP connection. (`apps/mcp/tests/handshake.rs` checks that no MCP tool
/// exposes it.)
#[test]
fn an_mcp_connection_does_not_see_resources() {
    let m = Machine::demo();
    m.add_demo(&[]);
    let cli = Client::connect(&m.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    assert!(
        cli.hello()
            .methods
            .iter()
            .any(|n| n == methods::ENGINE_RESOURCES)
    );
    let mut mcp = Client::connect(&m.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    assert!(
        !mcp.hello()
            .methods
            .iter()
            .any(|n| n == methods::ENGINE_RESOURCES)
    );
    match mcp.call::<_, Value>(methods::ENGINE_RESOURCES, serde_json::json!({})) {
        Err(ClientError::Rpc(err)) => assert_eq!(err.code, code::METHOD_NOT_FOUND),
        other => panic!("{other:?}"),
    }
    drop((cli, mcp));
    m.stop();
}
