//! US-GRP-017, Enmienda (2026-10-07): `raptor status --resources` shows the
//! active and dormant repos and what each tier costs. The real `raptor` is
//! the client; the daemon runs in process with a short threshold check so a
//! repo sleeps during the test (the real binary checks every 30 s at
//! least). Temporary repos and profile, never this repo nor the real
//! profile (NFR-01); every step waits for a state, never a fixed time.
//!
//! macOS only, like the other channel tests. Linux and Windows: Pendiente:
//! etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TierConfig,
};
use gitraptor_core::profile::{NewEvent, Profile, ProfileDirs, Timestamp, WriteOp};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const DEADLINE: Duration = Duration::from_secs(30);

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

/// Two observed repos: "busy" just added (active) and "idle", whose last
/// Git event is two hours old, so it sleeps at the first threshold check.
struct Machine {
    _busy: Fixture,
    _idle: Fixture,
    _tmp: tempfile::TempDir,
    profile: PathBuf,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn machine() -> Machine {
    let busy = Fixture::with_commit(&git_from_path());
    let idle = Fixture::with_commit(&git_from_path());
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("profile");
    let dirs = ProfileDirs::under_root(&profile);
    let mut store = Profile::open(dirs.clone()).unwrap().0;
    store
        .add_repo(&canonical(&busy.repo.join(".git")), None, 1)
        .unwrap();
    let (entry, _) = store
        .add_repo(&canonical(&idle.repo.join(".git")), None, 1)
        .unwrap();
    let two_hours_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        - 2 * 3600 * 1000;
    let (mut repo_store, _) = store.open_store(&entry.repo_id).unwrap();
    let root = canonical(&idle.repo);
    repo_store
        .write_batch(&[
            WriteOp::UpsertWorktree {
                path: root.clone(),
                admin_name: None,
                seen_ms: two_hours_ago,
            },
            WriteOp::AppendEvent(NewEvent {
                worktree: root,
                kind: "commit".into(),
                metadata: "{}".into(),
                observed: Timestamp {
                    utc_ms: two_hours_ago,
                    offset_s: 0,
                },
                session_id: None,
                evidence: None,
                gap_id: None,
                authorship: None,
            }),
        ])
        .unwrap();
    drop(repo_store);
    drop(store);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let daemon = Daemon::start(DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: TierConfig {
            dormant_after: Some(Duration::from_secs(3600)),
            check_every: Duration::from_millis(50),
            ..TierConfig::default()
        },
        tm_capture: Default::default(),
        discovery: Default::default(),
    })
    .unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let m = Machine {
        _busy: busy,
        _idle: idle,
        _tmp: tmp,
        profile,
        handle,
        join: Some(join),
    };
    m.wait_for_tiers();
    m
}

impl Machine {
    /// `raptor status --resources <args>` in `lang`, without a terminal.
    fn raptor(&self, lang: &str, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(["status", "--resources"])
            .args(args)
            .env_clear()
            .env("GITRAPTOR_PROFILE_DIR", &self.profile)
            .env("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent")
            .env("GITRAPTOR_LANG", lang)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn json(&self) -> Value {
        let out = self.raptor("en", &["--json"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Waits until "idle" sleeps and "busy" stays active.
    fn wait_for_tiers(&self) {
        let start = Instant::now();
        loop {
            let out = self.raptor("en", &["--json"]);
            if out.status.success() {
                let v: Value = serde_json::from_slice(&out.stdout).unwrap();
                let o = &v["engine"]["observation"];
                if o["dormant"]["repos"] == 1 && o["active"]["repos"] == 1 {
                    return;
                }
                assert!(start.elapsed() < DEADLINE, "{v:#}");
            } else {
                assert!(start.elapsed() < DEADLINE, "{}", text(&out));
            }
            std::thread::sleep(Duration::from_millis(50));
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

/// The line that starts with `label`.
fn line<'a>(shown: &'a str, label: &str) -> &'a str {
    shown
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} line in:\n{shown}"))
}

/// Ve cuántos repos están activos y cuántos dormidos y, por nivel, las
/// vigilancias y lo que cuestan las redes de seguridad (inglés).
#[test]
fn text_in_english_shows_the_tiers() {
    let m = machine();
    let out = m.raptor("en", &[]);
    assert!(out.status.success(), "{}", text(&out));
    let shown = String::from_utf8(out.stdout).unwrap();
    let tiers = line(&shown, "observed repos:");
    assert!(tiers.contains("1 active"), "{shown}");
    assert!(tiers.contains("1 dormant"), "{shown}");
    let active = line(&shown, "active:");
    assert!(active.contains("1 repos"), "{shown}");
    assert!(active.contains("watches"), "{shown}");
    let dormant = line(&shown, "dormant:");
    assert!(dormant.contains("1 repos"), "{shown}");
    assert!(dormant.contains("watches"), "{shown}");
    let nets = line(&shown, "dormant safety nets:");
    assert!(nets.contains("sweep every 2 min"), "{shown}");
    assert!(nets.contains("reconciliation every 60 min"), "{shown}");
    line(&shown, "polled worktrees");
}

/// Lo mismo en español.
#[test]
fn text_in_spanish_shows_the_tiers() {
    let m = machine();
    let out = m.raptor("es", &[]);
    assert!(out.status.success(), "{}", text(&out));
    let shown = String::from_utf8(out.stdout).unwrap();
    let tiers = line(&shown, "repos observados:");
    assert!(tiers.contains("1 activos"), "{shown}");
    assert!(tiers.contains("1 dormidos"), "{shown}");
    let active = line(&shown, "activos:");
    assert!(active.contains("1 repos"), "{shown}");
    assert!(active.contains("vigilancias"), "{shown}");
    let dormant = line(&shown, "dormidos:");
    assert!(dormant.contains("1 repos"), "{shown}");
    assert!(dormant.contains("vigilancias"), "{shown}");
    let nets = line(&shown, "redes de seguridad de los dormidos:");
    assert!(nets.contains("barrido cada 2 min"), "{shown}");
    assert!(nets.contains("reconciliación cada 60 min"), "{shown}");
    line(&shown, "worktrees sondeados");
}

/// `--json` trae los mismos recuentos en unidades fijas.
#[test]
fn json_has_the_tier_counts() {
    let m = machine();
    let v = m.json();
    let o = &v["engine"]["observation"];
    assert_eq!(o["active"]["repos"], 1, "{v:#}");
    assert_eq!(o["active"]["worktrees"], 1, "{v:#}");
    assert!(o["active"]["watches"].as_u64().unwrap() >= 1, "{v:#}");
    assert_eq!(o["waking"]["repos"], 0, "{v:#}");
    assert_eq!(o["dormant"]["repos"], 1, "{v:#}");
    assert_eq!(o["dormant"]["worktrees"], 1, "{v:#}");
    assert!(o["dormant"]["watches"].as_u64().unwrap() >= 1, "{v:#}");
    assert_eq!(o["dormant"]["sweep_interval_s"], 120, "{v:#}");
    assert_eq!(o["dormant"]["reconcile_interval_s"], 3600, "{v:#}");
    assert_eq!(o["degraded"]["worktrees"], 0, "{v:#}");
}
