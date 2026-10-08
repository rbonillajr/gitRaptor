//! Freshness, scale and footprint bench of the engine (INF-GRP-002, ADR-GRP-011 § 4, NFR HUELLA).
//!
//! Runs the engine as an isolated daemon process over a temporary profile: this same bench
//! binary (release, as `cargo bench` builds it) re-executed with `--daemon-root`, which calls
//! `daemon::run_process` as `raptor daemon` does. The `raptor` release binary cannot be used: it
//! ignores `GITRAPTOR_PROFILE_DIR` (SEC-06) and would run on the real profile. It observes a clone of the 100K-commit reference repo (profile `H` of
//! `repogen`) with 10 worktrees. A headless subscriber records `t_client_recv` on the common
//! monotonic clock; the bench writes `t0` when the write or the Git command ends.
//!
//! ```sh
//! cargo bench -p gitraptor-cli --bench engine                    # full run, ~10-15 min
//! cargo bench -p gitraptor-cli --bench engine -- --quick         # smoke, ~2 min
//! cargo bench -p gitraptor-cli --bench engine -- --gate ci       # the gate of the CI runners
//! ENGINE_BENCH_ROOT=/scratch cargo bench -p gitraptor-cli --bench engine -- --keep
//! ```
//!
//! Scenario `tui-modify` (US-CKP-001, M1 criterion 6): the Cockpit's `App` on `TestBackend`,
//! connected to the daemon through the real channel, measures "modify a file" end to end, from
//! `t0` to `t_render` of the frame that shows it (NFR-04: 500 ms p95), and its own stage
//! (`t_client_recv` → `t_render`: 100 ms p95). `--only tui-modify` runs it alone.
//!
//! Gates (exit code 1), by `--gate` (INF-GRP-002, Enmienda 2026-10-05):
//! - `reference` (default outside CI): the p95 of the engine total (`t0` → `t_client_recv`)
//!   above 300 ms in any scenario, or above the provisional ceiling of a burst (NFR-04).
//! - `ci` (default under `GITHUB_ACTIONS`): that budget is only reported; a scenario over the
//!   regression ceilings calibrated for the runner (p50, and p95 where the runner can gate it)
//!   is measured again, and fails when two of three attempts are over.
//! - Both: the footprint of the isolated daemon above its limits; a sample without its event; a
//!   change lost by the stream recreation. A stage over its budget only warns, naming the stage.
//!
//! Everything lives under a root outside any Git repo (NFR-01): `ENGINE_BENCH_ROOT` or a
//! temporary folder. The generated reference repo is kept in that root and reused (CI caches
//! it); every run works on a fresh local clone that it deletes unless `--keep`.

#[cfg(not(unix))]
fn main() {
    // The channel client only exists on Unix (TS-GRP-004).
    // Pendiente: etapa de validación multiplataforma.
    println!("engine bench: skipped, the channel client is Unix-only for now");
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--daemon-root") {
        std::process::exit(unix::daemon_main(std::path::Path::new(&args[i + 1])));
    }
    std::process::exit(unix::run());
}

#[cfg(unix)]
mod unix {
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use gitraptor_api::PROTOCOL_VERSION;
    use gitraptor_api::clock::monotonic_ns;
    use gitraptor_api::event::{Timings, WORKTREE_STATE};
    use gitraptor_api::messages::{
        ChangeAreaView, ClientKind, HeadView, SubscribeResult, WorktreeStateData, WorktreeStatus,
        WorktreeView,
    };
    use gitraptor_api::methods;
    use gitraptor_cli::client::engine::EngineConnector;
    use gitraptor_cli::client::{self as tui_client, ClientThread};
    use gitraptor_cli::model::{ConnState, Model, Size, WorktreeState};
    use gitraptor_cli::present::i18n::Lang;
    use gitraptor_cli::tui::app::App;
    use gitraptor_core::channel::AgentMatcher;
    use gitraptor_core::client::{Client, ClientOptions, Launcher};
    use gitraptor_core::daemon::{self, DaemonConfig};
    use gitraptor_core::profile::{Profile, ProfileDirs};
    use gitraptor_core::watch::WatchConfig;
    use gitraptor_git::{ReaderOptions, RefName, RepoReader};
    use gitraptor_testkit::fixture::git_from_path;
    use gitraptor_testkit::freshness::{
        BURST_1K, BURST_10K, COCKPIT_P95_MS, E2E_BUDGET_MS, FOOTPRINT_LIMITS, Finding, Footprint,
        GateMode, Level, MAX_ATTEMPTS, MAX_SLACK_EXCESS_MS, Platform, Sample, Scenario, Stage,
        Summary, TUI_MODIFY, Verdict, confirm, evaluate_footprint, evaluate_latency,
        evaluate_regression,
    };
    use gitraptor_testkit::repogen;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::{Value, json};

    /// Executable name no process of the bench has: no client is classified as an agent, as
    /// with `GITRAPTOR_AGENT_EXECUTABLES` in the tests.
    const NO_AGENT: &str = "raptor-fake-agent";
    const SEED: u64 = 0x1f_9002;
    /// Samples dropped at the start of every scenario (ADR-GRP-011 § 4).
    const WARMUP: usize = 10;
    /// Events the daemon keeps for replay (`ChannelConfig::default().replay`).
    const REPLAY_EVENTS: usize = 1024;
    /// How long a sample may wait for the event that shows its change.
    const SAMPLE_DEADLINE: Duration = Duration::from_secs(5);
    /// Pause after a sample so the next action never lands in its trailing window.
    const SETTLE: Duration = Duration::from_millis(150);
    /// Tracked file every write scenario edits (exists in profile `H`).
    const TOUCHED: &str = "bench-touched.txt";

    struct Opts {
        profile: String,
        samples: usize,
        worktrees: usize,
        idle_secs: u64,
        recreations: usize,
        burst_files: usize,
        scale_files: usize,
        root: Option<PathBuf>,
        out: Option<PathBuf>,
        only: Option<Vec<String>>,
        keep: bool,
        gate: GateMode,
    }

    fn opts() -> Opts {
        let mut o = Opts {
            profile: "H".into(),
            samples: 200,
            worktrees: 10,
            idle_secs: 30,
            recreations: 40,
            burst_files: 10_000,
            scale_files: 1_000,
            root: std::env::var_os("ENGINE_BENCH_ROOT").map(PathBuf::from),
            out: None,
            only: None,
            keep: false,
            gate: if std::env::var_os("GITHUB_ACTIONS").is_some() {
                GateMode::SharedCi
            } else {
                GateMode::Reference
            },
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            let next = args.get(i + 1).cloned().unwrap_or_default();
            let mut used = true;
            match args[i].as_str() {
                "--quick" => {
                    o.profile = "S".into();
                    o.samples = 40;
                    o.idle_secs = 10;
                    o.recreations = 10;
                    o.burst_files = 2_000;
                    used = false;
                }
                "--profile" => o.profile = next,
                "--samples" => o.samples = next.parse().expect("--samples N"),
                "--worktrees" => o.worktrees = next.parse().expect("--worktrees N"),
                "--idle-secs" => o.idle_secs = next.parse().expect("--idle-secs N"),
                "--recreations" => o.recreations = next.parse().expect("--recreations N"),
                "--burst-files" => o.burst_files = next.parse().expect("--burst-files N"),
                "--scale-files" => o.scale_files = next.parse().expect("--scale-files N"),
                "--root" => o.root = Some(PathBuf::from(next)),
                "--out" => o.out = Some(PathBuf::from(next)),
                "--only" => o.only = Some(next.split(',').map(str::to_owned).collect()),
                "--gate" => {
                    o.gate = match next.as_str() {
                        "ci" => GateMode::SharedCi,
                        "reference" => GateMode::Reference,
                        _ => panic!("--gate ci|reference"),
                    }
                }
                "--keep" => {
                    o.keep = true;
                    used = false;
                }
                // `cargo bench` passes `--bench`; nothing else is expected.
                _ => used = false,
            }
            i += if used { 2 } else { 1 };
        }
        assert!(o.worktrees >= 4, "--worktrees: at least 4");
        assert!(o.samples > WARMUP, "--samples: more than {WARMUP}");
        o
    }

    fn os() -> &'static str {
        if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "other"
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = repogen::git_command(&git_from_path(), dir)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Output of a command, trimmed; `None` when it does not run or fails.
    fn output(program: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// What the measurement ran on, so reports of different releases can be compared (TD-GRP-003):
    /// commit, machine, system, cores, load and power source.
    fn conditions() -> Value {
        let manifest = env!("CARGO_MANIFEST_DIR");
        // The checked-out commit: with `workflow_dispatch -f ref=…`, `GITHUB_SHA` is another.
        let commit = output("git", &["-C", manifest, "rev-parse", "HEAD"])
            .or_else(|| std::env::var("GITHUB_SHA").ok());
        let dirty = output(
            "git",
            &[
                "-C",
                manifest,
                "status",
                "--porcelain",
                "--untracked-files=no",
            ],
        )
        .map(|s| !s.is_empty());
        let power = if cfg!(target_os = "macos") {
            output("pmset", &["-g", "batt"]).and_then(|s| s.lines().next().map(str::to_owned))
        } else {
            std::fs::read_to_string("/sys/class/power_supply/AC/online")
                .ok()
                .map(|v| format!("AC online: {}", v.trim()))
        };
        json!({
            "commit": commit,
            "worktree_dirty": dirty,
            "system": output("uname", &["-srm"]),
            "model": if cfg!(target_os = "macos") { output("sysctl", &["-n", "hw.model"]) } else { None },
            "cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
            "load": output("uptime", &[]),
            "power": power,
            "ci": std::env::var_os("GITHUB_ACTIONS").is_some(),
        })
    }

    fn now_ms() -> i64 {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        i64::try_from(d.as_millis()).unwrap()
    }

    fn private_dir(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    // ------------------------------------------------------------ reference repo

    /// The reference repo of `profile`, generated once under `root` and reused.
    fn reference(root: &Path, profile: &str) -> PathBuf {
        let p = repogen::profile(profile).expect("unknown --profile");
        let repo = root.join(format!("ref-{profile}"));
        let done = root.join(format!("ref-{profile}.done"));
        if done.exists() {
            println!("reference repo {profile}: reused {}", repo.display());
            return repo;
        }
        if repo.exists() {
            std::fs::remove_dir_all(&repo).unwrap();
        }
        let g = repogen::generate(&git_from_path(), &p, &repo, SEED).unwrap();
        // The file every write scenario edits.
        std::fs::write(repo.join(TOUCHED), "0\n").unwrap();
        git(&repo, &["add", TOUCHED]);
        git(&repo, &["commit", "-qm", "bench file"]);
        git(&repo, &["commit-graph", "write", "--reachable"]);
        std::fs::write(&done, format!("{g:?}\n")).unwrap();
        println!(
            "reference repo {profile}: {} commits, {} files in {:.0} s",
            g.commits, g.tracked_files, g.secs
        );
        repo
    }

    // ------------------------------------------------------------ the daemon

    struct Daemon {
        child: Child,
        dirs: ProfileDirs,
    }

    impl Daemon {
        fn start(profile_root: &Path, common_dir: &Path) -> Self {
            for dir in ["", "data", "config", "state"] {
                private_dir(&profile_root.join(dir));
            }
            let dirs = ProfileDirs::under_root(profile_root);
            {
                let (mut profile, _) = Profile::open(dirs.clone()).unwrap();
                profile.add_repo(common_dir, None, now_ms()).unwrap();
            }
            let child = Command::new(std::env::current_exe().unwrap())
                .arg("--daemon-root")
                .arg(profile_root)
                .env_clear()
                // Whatever reads the home folder lands in the temporary profile.
                .env("HOME", profile_root)
                .env("PATH", "/usr/bin:/bin:/usr/local/bin")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let d = Self { child, dirs };
            let start = Instant::now();
            let mut d = d;
            while d.connect().is_none() {
                if let Ok(Some(status)) = d.child.try_wait() {
                    panic!("daemon exited: {status}");
                }
                assert!(start.elapsed() < Duration::from_secs(20), "daemon not up");
                std::thread::sleep(Duration::from_millis(50));
            }
            d
        }

        fn connect(&self) -> Option<Client> {
            Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION).ok()
        }

        fn pid(&self) -> u32 {
            self.child.id()
        }
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            // An orderly stop, as the developer's `raptor daemon stop`.
            let _ = Command::new("/bin/kill")
                .arg(self.pid().to_string())
                .status();
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(10) {
                if let Ok(Some(_)) = self.child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// The daemon process: the configuration of `raptor daemon`, on the temporary profile.
    pub fn daemon_main(root: &Path) -> i32 {
        let mut config = match DaemonConfig::for_current_user() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("bench daemon: {e}");
                return 1;
            }
        };
        config.dirs = ProfileDirs::under_root(root);
        config.channel.agents = AgentMatcher::only(vec![NO_AGENT.into()]);
        match daemon::run_process(config) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("bench daemon: {e}");
                1
            }
        }
    }

    // ------------------------------------------------------------ headless subscriber

    /// A `worktree.state` event as the subscriber got it.
    struct Received {
        t_client_recv: u64,
        timings: Option<Timings>,
        data: WorktreeStateData,
    }

    /// Subscribes and forwards every `worktree.state` with the time it was read.
    fn subscribe(
        daemon: &Daemon,
        stop: Arc<AtomicBool>,
        sizes: Arc<Mutex<std::collections::VecDeque<usize>>>,
    ) -> Receiver<Received> {
        let mut client = daemon.connect().unwrap();
        let _: SubscribeResult = client.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Ok(Some(n)) = client.next_notification(Duration::from_millis(200)) else {
                    continue;
                };
                let t_client_recv = monotonic_ns();
                let event = &n.params["event"];
                let bytes = event.to_string().len();
                {
                    let mut ring = sizes.lock().unwrap();
                    ring.push_back(bytes);
                    if ring.len() > REPLAY_EVENTS {
                        ring.pop_front();
                    }
                }
                if event["kind"] != WORKTREE_STATE {
                    continue;
                }
                let timings = serde_json::from_value(event["timings"].clone()).ok();
                let Ok(data) = serde_json::from_value(event["data"].clone()) else {
                    continue;
                };
                let r = Received {
                    t_client_recv,
                    timings,
                    data,
                };
                if tx.send(r).is_err() {
                    return;
                }
            }
        });
        rx
    }

    /// Last known state of each worktree, from the stream.
    struct Stream {
        rx: Receiver<Received>,
        state: HashMap<PathBuf, WorktreeView>,
        /// `worktree.state` events received.
        received: u64,
    }

    impl Stream {
        fn absorb(&mut self, r: &Received) {
            self.received += 1;
            for w in &r.data.worktrees {
                self.state.insert(PathBuf::from(w.path.raw()), w.clone());
            }
            // Every event lists all the worktrees of the repo: the rest are gone.
            let listed: Vec<PathBuf> = r
                .data
                .worktrees
                .iter()
                .map(|w| PathBuf::from(w.path.raw()))
                .collect();
            self.state.retain(|p, _| listed.contains(p));
        }

        /// Absorbs what is pending.
        fn drain(&mut self) {
            while let Ok(r) = self.rx.try_recv() {
                self.absorb(&r);
            }
        }

        /// The first event after now whose state satisfies `shows`, with its marks.
        ///
        /// `since` is when the action started: an event published while a Git command was still
        /// running counts, with a total of zero (the change was visible at `t0`).
        fn wait(
            &mut self,
            since: u64,
            t0: u64,
            shows: impl Fn(&WorktreeStateData) -> bool,
        ) -> Option<Sample> {
            let deadline = Instant::now() + SAMPLE_DEADLINE;
            loop {
                let left = deadline.checked_duration_since(Instant::now())?;
                let r = match self.rx.recv_timeout(left) {
                    Ok(r) => r,
                    Err(RecvTimeoutError::Timeout) => return None,
                    Err(RecvTimeoutError::Disconnected) => panic!("subscriber gone"),
                };
                self.absorb(&r);
                if r.t_client_recv < since || !shows(&r.data) {
                    continue;
                }
                let t = r.timings?;
                return Some(Sample {
                    t0,
                    t_recv: t.t_recv,
                    t_flush: t.t_flush,
                    t_computed: t.t_computed,
                    t_persisted: t.t_persisted,
                    t_published: t.t_published,
                    t_client_recv: r.t_client_recv,
                });
            }
        }

        /// Waits until the known state satisfies `ok` (no timing).
        fn until(
            &mut self,
            timeout: Duration,
            ok: impl Fn(&HashMap<PathBuf, WorktreeView>) -> bool,
        ) -> bool {
            let deadline = Instant::now() + timeout;
            loop {
                self.drain();
                if ok(&self.state) {
                    return true;
                }
                let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                    return false;
                };
                if let Ok(r) = self.rx.recv_timeout(left.min(Duration::from_millis(100))) {
                    self.absorb(&r);
                }
            }
        }
    }

    fn view<'a>(data: &'a WorktreeStateData, wt: &Path) -> Option<&'a WorktreeView> {
        data.worktrees
            .iter()
            .find(|w| Path::new(w.path.raw()) == wt)
    }

    fn area_of(data: &WorktreeStateData, wt: &Path, file: &str) -> Option<ChangeAreaView> {
        match &view(data, wt)?.status {
            WorktreeStatus::Ready { changes, .. } => changes
                .iter()
                .find(|c| c.path.raw() == file)
                .map(|c| c.area),
            WorktreeStatus::Unavailable { .. } => None,
        }
    }

    fn is_clean(data: &WorktreeStateData, wt: &Path) -> bool {
        matches!(view(data, wt).map(|w| &w.status),
            Some(WorktreeStatus::Ready { counts, .. }) if counts.is_clean())
    }

    fn on_branch(data: &WorktreeStateData, wt: &Path, branch: &str) -> bool {
        matches!(view(data, wt).map(|w| &w.status),
            Some(WorktreeStatus::Ready { head: HeadView::Branch { name }, .. }) if name.raw() == branch)
    }

    fn untracked(w: &WorktreeView) -> Option<u32> {
        match &w.status {
            WorktreeStatus::Ready { counts, .. } => Some(counts.untracked),
            WorktreeStatus::Unavailable { .. } => None,
        }
    }

    // ------------------------------------------------------------ footprint of the daemon

    #[derive(Debug, Clone, Copy)]
    struct ProcStat {
        rss_mib: f64,
        cpu_s: f64,
    }

    #[cfg(target_os = "linux")]
    fn proc_stat(pid: u32) -> Option<ProcStat> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let rss_kib: f64 = status
            .lines()
            .find(|l| l.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Fields after the command name, which may contain spaces.
        let rest = &stat[stat.rfind(')')? + 2..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        let ticks: f64 = f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?;
        Some(ProcStat {
            rss_mib: rss_kib / 1024.0,
            cpu_s: ticks / clk_tck(),
        })
    }

    #[cfg(target_os = "linux")]
    fn clk_tck() -> f64 {
        static TCK: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
        *TCK.get_or_init(|| {
            Command::new("getconf")
                .arg("CLK_TCK")
                .output()
                .ok()
                .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
                .unwrap_or(100.0)
        })
    }

    /// `ps` is the only reader of another process's CPU time without `unsafe`.
    #[cfg(not(target_os = "linux"))]
    fn proc_stat(pid: u32) -> Option<ProcStat> {
        let out = Command::new("/bin/ps")
            .args(["-o", "rss=,time=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut it = text.split_whitespace();
        let rss_kib: f64 = it.next()?.parse().ok()?;
        // [[dd-]hh:]mm:ss.cc
        let time = it.next()?;
        let (days, time) = match time.split_once('-') {
            Some((d, t)) => (d.parse::<f64>().ok()?, t),
            None => (0.0, time),
        };
        let mut cpu_s = days * 86_400.0;
        for part in time.split(':') {
            cpu_s = cpu_s * 60.0 + part.parse::<f64>().ok()?;
        }
        Some(ProcStat {
            rss_mib: rss_kib / 1024.0,
            cpu_s,
        })
    }

    /// Open descriptors and, on Linux, inotify watches.
    fn descriptors(pid: u32) -> (f64, Option<u64>) {
        #[cfg(target_os = "linux")]
        {
            let fds = std::fs::read_dir(format!("/proc/{pid}/fd"))
                .map(|d| d.count() as f64)
                .unwrap_or(f64::NAN);
            let mut watches = 0;
            if let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/fdinfo")) {
                for e in dir.flatten() {
                    if let Ok(s) = std::fs::read_to_string(e.path()) {
                        watches +=
                            s.lines().filter(|l| l.starts_with("inotify wd:")).count() as u64;
                    }
                }
            }
            (fds, Some(watches))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let fds = Command::new("/usr/sbin/lsof")
                .args(["-n", "-P", "-p", &pid.to_string()])
                .output()
                .map(|o| {
                    let lines = String::from_utf8_lossy(&o.stdout).lines().count();
                    lines.saturating_sub(1) as f64
                })
                .unwrap_or(f64::NAN);
            // FSEvents streams have no descriptor per watch.
            (fds, None)
        }
    }

    /// Peak RSS of the daemon while it runs, polled every 100 ms.
    struct RssPeak {
        peak_kib: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
    }

    impl RssPeak {
        fn start(pid: u32) -> Self {
            let peak_kib = Arc::new(AtomicU64::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (p, s) = (peak_kib.clone(), stop.clone());
            std::thread::spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    if let Some(st) = proc_stat(pid) {
                        p.fetch_max((st.rss_mib * 1024.0) as u64, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            Self { peak_kib, stop }
        }

        fn finish(self) -> f64 {
            self.stop.store(true, Ordering::Relaxed);
            self.peak_kib.load(Ordering::Relaxed) as f64 / 1024.0
        }
    }

    // ------------------------------------------------------------ the TUI

    /// The Cockpit's `App` on `TestBackend`, connected to the isolated daemon (never starts
    /// one).
    struct Tui {
        app: App<TestBackend>,
        channel: Option<ClientThread>,
    }

    impl Tui {
        fn open(dirs: &ProfileDirs, cwd: &Path) -> Self {
            let mut options = ClientOptions::new(dirs.clone(), ClientKind::Cli);
            options.launcher = Launcher::Never;
            let (inbox, _input, engine) = gitraptor_cli::queue::inbox();
            let size = Size {
                width: 120,
                height: 40,
            };
            let mut app = App::new(
                Terminal::new(TestBackend::new(size.width, size.height)).unwrap(),
                Model::new(Lang::En, size),
                inbox,
            );
            let channel = tui_client::spawn(
                EngineConnector::new(options.connect().unwrap(), Box::new(options.launcher())),
                Some(cwd.into()),
                engine,
            );
            app.attach(channel.cmds.clone());
            Self {
                app,
                channel: Some(channel),
            }
        }

        /// Steps the TUI until a painted frame shows a model that satisfies `ok`.
        fn until(&mut self, timeout: Duration, ok: impl Fn(&Model) -> bool) -> bool {
            let deadline = Instant::now() + timeout;
            loop {
                let _ = self.app.step(Duration::from_millis(10));
                if !self.app.model.dirty && ok(&self.app.model) {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
            }
        }
    }

    impl Drop for Tui {
        fn drop(&mut self) {
            if let Some(channel) = self.channel.take() {
                channel.shutdown();
            }
        }
    }

    fn worktrees(model: &Model) -> usize {
        model
            .engine
            .repo
            .as_ref()
            .and_then(|r| r.data.as_ref())
            .map_or(0, |d| d.worktrees.len())
    }

    /// Changed files the fleet shows for the worktree at `path`.
    fn changes(model: &Model, path: &str) -> Option<u64> {
        let data = model.engine.repo.as_ref()?.data.as_ref()?;
        let row = data.worktrees.iter().find(|w| w.path.as_str() == path)?;
        match row.state {
            WorktreeState::Ready { changes, .. } => Some(changes),
            WorktreeState::Unavailable(_) => None,
        }
    }

    // ------------------------------------------------------------ the bench

    struct BurstStats {
        peak_mib: f64,
        cpu_pct: f64,
        /// Seconds until the RSS was back under the idle limit, `None` after 60 s.
        back_s: Option<f64>,
    }

    struct Bench {
        opts: Opts,
        /// Excess timer slack of this machine (see `Scenario::slack_excess_ms`).
        slack_excess_ms: f64,
        repo: PathBuf,
        /// Linked worktrees `wt-1`..`wt-n`, canonical.
        wts: Vec<PathBuf>,
        stream: Stream,
        daemon: Daemon,
        scenarios: Vec<Scenario>,
        findings: Vec<Finding>,
        report: serde_json::Map<String, Value>,
        /// Next content of the touched file.
        counter: u64,
        /// Worktrees whose touched file is modified, its committed and last written content.
        dirty: std::collections::HashSet<PathBuf>,
        committed: HashMap<PathBuf, String>,
        written: HashMap<PathBuf, String>,
        /// Attempt being measured: 1, or a confirmation of a regression.
        attempt: usize,
        /// Attempts of every scenario at the regression gate, in the order measured.
        regressions: Vec<(String, Vec<Attempt>)>,
        /// The branches of the checkout scenario exist.
        checkout_ready: bool,
    }

    impl Bench {
        fn platform(&self) -> Platform {
            Platform::detect(self.opts.gate)
        }

        /// Attempts at the regression gate of `name` so far.
        fn attempts(&self, name: &str) -> &[Attempt] {
            self.regressions
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, a)| a.as_slice())
                .unwrap_or_default()
        }

        /// Verdict of the confirmation for `name`; a scenario not measured has nothing to
        /// confirm.
        fn verdict(&self, name: &str) -> Verdict {
            let a = self.attempts(name);
            if a.is_empty() {
                return Verdict::Pass;
            }
            confirm(&a.iter().map(|x| !x.findings.is_empty()).collect::<Vec<_>>())
        }

        fn run_group(&mut self, g: Group) {
            match g {
                Group::WriteCycle => self.write_cycle(),
                Group::Checkout => self.checkout(),
                Group::Worktrees => self.worktree_add_remove(),
                Group::Burst1k => {
                    self.burst(BURST_1K, self.opts.scale_files);
                }
                Group::Burst10k => {
                    self.burst(BURST_10K, self.opts.burst_files);
                }
                Group::Tui => self.tui_modify(),
            }
        }

        /// Measures again every step with a scenario over its regression ceiling, until two
        /// attempts agree (two of three). A burst waits first for the daemon to settle.
        fn confirm_regressions(&mut self) {
            for g in Group::ALL {
                for attempt in 2..=MAX_ATTEMPTS {
                    let pending: Vec<&str> = g
                        .scenarios()
                        .iter()
                        .copied()
                        .filter(|n| self.verdict(n) == Verdict::Retry)
                        .collect();
                    if pending.is_empty() {
                        break;
                    }
                    println!(
                        "\n{} over a regression ceiling: measuring again, attempt {attempt} of {MAX_ATTEMPTS}",
                        pending.join(", ")
                    );
                    if matches!(g, Group::Burst1k | Group::Burst10k) {
                        std::thread::sleep(Duration::from_secs(10));
                        self.stream.drain();
                    }
                    self.attempt = attempt;
                    self.run_group(g);
                }
            }
            self.attempt = 1;
        }

        /// The findings of the regression gate once confirmed: a regression in two of the
        /// attempts fails; one that was not confirmed warns. Out of calibration, everything
        /// warns (the ceilings do not apply to that runner).
        fn regression_findings(&self, out_of_calibration: bool) -> Vec<Finding> {
            let mut out = Vec::new();
            for (name, attempts) in &self.regressions {
                let bad: Vec<&Attempt> =
                    attempts.iter().filter(|a| !a.findings.is_empty()).collect();
                let note = format!(" ({} of {} attempts)", bad.len(), attempts.len());
                let (level, label) = match self.verdict(name) {
                    Verdict::Pass if bad.is_empty() => continue,
                    Verdict::Pass => (Level::Warn, "not confirmed"),
                    Verdict::Fail | Verdict::Retry => (Level::Fail, "confirmed"),
                };
                let level = if out_of_calibration {
                    Level::Warn
                } else {
                    level
                };
                for f in &bad.last().unwrap().findings {
                    out.push(Finding {
                        level,
                        what: format!("{}, {label}{note}", f.what),
                        ..f.clone()
                    });
                }
            }
            out
        }

        fn wants(&self, name: &str) -> bool {
            self.opts
                .only
                .as_ref()
                .is_none_or(|o| o.iter().any(|x| x == name))
        }

        fn record(
            &mut self,
            name: &str,
            isolates_detection: bool,
            samples: Vec<Sample>,
            lost: usize,
        ) {
            let kept: Vec<Sample> = samples.into_iter().skip(WARMUP).collect();
            let s = Scenario {
                name: name.into(),
                isolates_detection,
                ceiling_ms: self.platform().burst_ceiling_ms(name),
                slack_excess_ms: self.slack_excess_ms,
                samples: kept,
            };
            let mut j = s.to_json();
            j["lost"] = lost.into();
            j["attempt"] = self.attempt.into();
            let mut fs = evaluate_latency(&s);
            if self.opts.gate == GateMode::SharedCi || self.slack_excess_ms > MAX_SLACK_EXCESS_MS {
                // A shared runner, or an unfit machine, reports the NFR-04 budget but cannot
                // gate it: the reference machine does (INF-GRP-002, Enmienda 2026-10-05).
                for f in &mut fs {
                    f.level = Level::Warn;
                }
            }
            if self.attempt > 1 {
                // A confirmation only feeds the regression gate.
                fs.clear();
            }
            if self.opts.gate == GateMode::SharedCi
                && (self.attempt == 1 || self.verdict(name) == Verdict::Retry)
            {
                let ceiling = self.platform().regression_ceiling(name);
                let findings = evaluate_regression(&s, ceiling);
                let total = s.summary(Stage::Total);
                j["regression"] = json!({
                    "p50_ceiling_ms": ceiling.map(|c| c.p50_ms),
                    "p95_ceiling_ms": ceiling.and_then(|c| c.p95_ms),
                    "regressed": !findings.is_empty(),
                });
                let attempt = Attempt {
                    attempt: self.attempt,
                    p50: total.map_or(f64::NAN, |t| t.p50),
                    p95: total.map_or(f64::NAN, |t| t.p95),
                    findings,
                };
                match self.regressions.iter_mut().find(|(n, _)| n == name) {
                    Some((_, a)) => a.push(attempt),
                    None => self.regressions.push((name.into(), vec![attempt])),
                }
            }
            // Wiring: every stage of the scenario was extracted, with finite values.
            for stage in Stage::ALL {
                if stage == Stage::Detection && !isolates_detection {
                    continue;
                }
                let ok = s
                    .summary(stage)
                    .is_some_and(|x| x.p95.is_finite() && x.max.is_finite());
                if !ok && !s.samples.is_empty() {
                    self.findings.push(Finding {
                        level: Level::Fail,
                        scenario: name.into(),
                        what: format!("stage {} missing from the report", stage.name()),
                        measured: 0.0,
                        limit: 0.0,
                        unit: "",
                    });
                }
            }
            print_scenario(&s, lost, self.attempt);
            if lost > 0 {
                // A change that never showed up within the deadline is a correctness failure.
                self.findings.push(Finding {
                    level: Level::Fail,
                    scenario: name.into(),
                    what: format!("samples without an event within {SAMPLE_DEADLINE:?}"),
                    measured: lost as f64,
                    limit: 0.0,
                    unit: "",
                });
            }
            self.findings.extend(fs);
            self.report
                .entry("latency")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(j);
            self.scenarios.push(s);
        }

        /// Writes the next content of the touched file in `wt` and returns `t0`.
        /// Writes the touched file of `wt`: back to its committed content when it is modified,
        /// new content otherwise. Returns `t0` and whether the file is now modified. Alternating
        /// means no earlier state can pass for the one this write produces.
        fn touch(&mut self, wt: &Path) -> (u64, bool) {
            let dirty = self.dirty.contains(wt);
            let content = if dirty {
                self.committed
                    .get(wt)
                    .cloned()
                    .unwrap_or_else(|| "0\n".into())
            } else {
                self.counter += 1;
                format!("{}\n", self.counter)
            };
            std::fs::write(wt.join(TOUCHED), &content).unwrap();
            let t0 = monotonic_ns();
            if dirty {
                self.dirty.remove(wt);
            } else {
                self.dirty.insert(wt.to_path_buf());
            }
            self.written.insert(wt.to_path_buf(), content);
            (t0, !dirty)
        }

        fn settle(&mut self) {
            std::thread::sleep(SETTLE);
            self.stream.drain();
        }

        /// One "modify a file" sample in `wt`: the touched file becomes modified, or clean
        /// again, in the published state.
        fn modify_sample(&mut self, wt: &Path) -> Option<Sample> {
            self.stream.drain();
            let since = monotonic_ns();
            let (t0, modified) = self.touch(wt);
            let s = self.stream.wait(since, t0, |d| {
                let area = area_of(d, wt, TOUCHED);
                let ready = matches!(
                    view(d, wt).map(|w| &w.status),
                    Some(WorktreeStatus::Ready { .. })
                );
                ready && (area == Some(ChangeAreaView::Unstaged)) == modified
            });
            self.settle();
            s
        }

        /// Modify, `git add` and commit, `n` times on `wt-1`.
        fn write_cycle(&mut self) {
            let wt = self.wts[0].clone();
            let n = self.opts.samples;
            let (mut modify, mut add, mut commit) = (Vec::new(), Vec::new(), Vec::new());
            let mut lost = [0; 3];
            for _ in 0..n {
                // Two writes per cycle at most: the one that leaves the file modified is kept.
                if self.dirty.contains(&wt) {
                    let _ = self.modify_sample(&wt);
                }
                match self.modify_sample(&wt) {
                    Some(s) => modify.push(s),
                    None => lost[0] += 1,
                }
                self.stream.drain();
                let since = monotonic_ns();
                git(&wt, &["add", TOUCHED]);
                let t0 = monotonic_ns();
                match self.stream.wait(since, t0, |d| {
                    area_of(d, &wt, TOUCHED) == Some(ChangeAreaView::Staged)
                }) {
                    Some(s) => add.push(s),
                    None => lost[1] += 1,
                }
                self.settle();
                let since = monotonic_ns();
                git(&wt, &["commit", "-qm", "bench"]);
                let t0 = monotonic_ns();
                match self.stream.wait(since, t0, |d| is_clean(d, &wt)) {
                    Some(s) => commit.push(s),
                    None => lost[2] += 1,
                }
                self.dirty.remove(&wt);
                if let Some(c) = self.written.get(&wt).cloned() {
                    self.committed.insert(wt.clone(), c);
                }
                self.settle();
            }
            self.record("modify", true, modify, lost[0]);
            self.record("git-add", false, add, lost[1]);
            self.record("commit", false, commit, lost[2]);
        }

        /// Checkout between two branches one commit apart, on `wt-2`.
        fn checkout(&mut self) {
            let wt = self.wts[1].clone();
            if !self.checkout_ready {
                git(&wt, &["switch", "-q", "-c", "co-a"]);
                std::fs::write(wt.join(TOUCHED), "checkout\n").unwrap();
                git(&wt, &["commit", "-qam", "co-b"]);
                git(&wt, &["branch", "co-b"]);
                git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
                self.checkout_ready = true;
            } else if self.dirty.remove(&wt) {
                // A confirmation runs after the bursts, which modify the touched file here too.
                git(&wt, &["restore", TOUCHED]);
            }
            self.settle();
            let mut samples = Vec::new();
            let mut lost = 0;
            for i in 0..self.opts.samples {
                let to = if i % 2 == 0 { "co-b" } else { "co-a" };
                self.stream.drain();
                let since = monotonic_ns();
                git(&wt, &["switch", "-q", to]);
                let t0 = monotonic_ns();
                match self
                    .stream
                    .wait(since, t0, |d| on_branch(d, &wt, to) && is_clean(d, &wt))
                {
                    Some(s) => samples.push(s),
                    None => lost += 1,
                }
                self.settle();
            }
            self.record("checkout", false, samples, lost);
        }

        /// Create and delete a worktree (N/4 samples, at least 20, plus the warm-up).
        fn worktree_add_remove(&mut self) {
            let n = (self.opts.samples / 4).max(20) + WARMUP;
            let base = self.repo.parent().unwrap().join("wt-tmp");
            let (mut add, mut remove) = (Vec::new(), Vec::new());
            let mut lost = [0; 2];
            for _ in 0..n {
                self.stream.drain();
                let since = monotonic_ns();
                git(
                    &self.repo,
                    &["worktree", "add", "-q", "--detach", base.to_str().unwrap()],
                );
                let t0 = monotonic_ns();
                let wt = base.canonicalize().unwrap();
                match self.stream.wait(since, t0, |d| is_clean(d, &wt)) {
                    Some(s) => add.push(s),
                    None => lost[0] += 1,
                }
                self.settle();
                let since = monotonic_ns();
                git(
                    &self.repo,
                    &["worktree", "remove", "--force", wt.to_str().unwrap()],
                );
                let t0 = monotonic_ns();
                match self.stream.wait(since, t0, |d| view(d, &wt).is_none()) {
                    Some(s) => remove.push(s),
                    None => lost[1] += 1,
                }
                self.settle();
            }
            self.record("worktree-create", false, add, lost[0]);
            self.record("worktree-delete", false, remove, lost[1]);
        }

        /// "Modify a file" seen by the real TUI (US-CKP-001, M1 criterion 6; NFR-04): the
        /// `App` of the Cockpit (queues, channel thread, `update`, `view`) on `TestBackend`,
        /// connected to the isolated daemon through the real channel, as `raptor` is. Each
        /// sample writes the touched file of `wt-3` (`t0`) and steps the TUI until the frame it
        /// painted shows the new count of changed files (`t_render`). The TUI's own stage
        /// (`t_client_recv` → `t_render`) comes from its metrics.
        fn tui_modify(&mut self) {
            let wt = self.wts[2].clone();
            let key = wt.to_string_lossy().into_owned();
            let mut tui = Tui::open(&self.daemon.dirs, &self.repo);
            let listed = self.wts.len() + 1;
            let ready = tui.until(Duration::from_secs(30), |m| {
                m.conn == ConnState::Live && changes(m, &key).is_some() && worktrees(m) >= listed
            });
            assert!(
                ready,
                "the TUI never showed the fleet: {:?}",
                tui.app.model.conn
            );
            let clean =
                changes(&tui.app.model, &key).unwrap() - u64::from(self.dirty.contains(&wt));
            // Only this scenario's messages in the Cockpit stage.
            tui.app.metrics = Default::default();
            let (mut samples, mut lost) = (Vec::new(), 0);
            for _ in 0..self.opts.samples {
                self.stream.drain();
                let (t0, modified) = self.touch(&wt);
                let expected = clean + u64::from(modified);
                if tui.until(SAMPLE_DEADLINE, |m| changes(m, &key) == Some(expected)) {
                    let t_render = tui.app.metrics.last_render_ns;
                    samples.push(Sample {
                        t0,
                        t_recv: t0,
                        t_flush: t0,
                        t_computed: t0,
                        t_persisted: t0,
                        t_published: t0,
                        t_client_recv: t_render.max(t0),
                    });
                } else {
                    lost += 1;
                }
                // The TUI keeps applying the stream while the bench settles.
                let settle = Instant::now() + SETTLE;
                while Instant::now() < settle {
                    let _ = tui.app.step(Duration::from_millis(10));
                }
                self.stream.drain();
            }
            let cockpit = tui.app.metrics.total.p95().map(|ns| ns as f64 / 1e6);
            let slowest = tui.app.metrics.slowest_stage();
            drop(tui);
            self.record_tui(samples, lost, cockpit, slowest.map(|s| s.to_string()));
        }

        /// Gates of `tui-modify`: the end-to-end budget (500 ms p95 plus the excess timer slack)
        /// fails on the reference machine and is reported on a shared runner, where the
        /// calibrated regression ceiling gates it with the confirmation of two of three; the
        /// Cockpit's own 100 ms p95 is CPU work on `TestBackend` and fails anywhere (Decisión
        /// del orquestador 2026-10-06, validada por Arquitecto).
        fn record_tui(
            &mut self,
            samples: Vec<Sample>,
            lost: usize,
            cockpit_p95_ms: Option<f64>,
            slowest: Option<String>,
        ) {
            let kept: Vec<Sample> = samples.into_iter().skip(WARMUP).collect();
            let s = Scenario {
                name: TUI_MODIFY.into(),
                isolates_detection: false,
                ceiling_ms: None,
                slack_excess_ms: self.slack_excess_ms,
                samples: kept,
            };
            let e2e = s.summary(Stage::Total);
            let budget = E2E_BUDGET_MS + self.slack_excess_ms.max(0.0);
            let mut fs = Vec::new();
            if let Some(e) = e2e
                && e.p95 > budget
            {
                let shared = self.opts.gate == GateMode::SharedCi
                    || self.slack_excess_ms > MAX_SLACK_EXCESS_MS;
                fs.push(Finding {
                    level: if shared { Level::Warn } else { Level::Fail },
                    scenario: TUI_MODIFY.into(),
                    what: "p95 end to end (t0 → t_render), NFR-04".into(),
                    measured: e.p95,
                    limit: budget,
                    unit: "ms",
                });
            }
            match cockpit_p95_ms {
                Some(p95) if p95 > COCKPIT_P95_MS => fs.push(Finding {
                    level: Level::Fail,
                    scenario: TUI_MODIFY.into(),
                    what: format!(
                        "p95 Cockpit (t_client_recv → t_render), slowest stage {}",
                        slowest.as_deref().unwrap_or("?")
                    ),
                    measured: p95,
                    limit: COCKPIT_P95_MS,
                    unit: "ms",
                }),
                None if !s.samples.is_empty() => fs.push(Finding {
                    level: Level::Fail,
                    scenario: TUI_MODIFY.into(),
                    what: "the Cockpit stage is missing from the report".into(),
                    measured: 0.0,
                    limit: 0.0,
                    unit: "",
                }),
                _ => {}
            }
            if self.attempt > 1 {
                fs.clear();
            }
            let mut j = json!({
                "scenario": TUI_MODIFY,
                "attempt": self.attempt,
                "lost": lost,
                "slack_excess_ms": self.slack_excess_ms,
                "end_to_end": e2e.map(|x| x.to_json()),
                "end_to_end_budget_p95_ms": budget,
                "cockpit_p95_ms": cockpit_p95_ms,
                "cockpit_budget_p95_ms": COCKPIT_P95_MS,
            });
            if self.opts.gate == GateMode::SharedCi
                && (self.attempt == 1 || self.verdict(TUI_MODIFY) == Verdict::Retry)
            {
                let ceiling = self.platform().regression_ceiling(TUI_MODIFY);
                let findings = evaluate_regression(&s, ceiling);
                j["regression"] = json!({
                    "p50_ceiling_ms": ceiling.map(|c| c.p50_ms),
                    "p95_ceiling_ms": ceiling.and_then(|c| c.p95_ms),
                    "regressed": !findings.is_empty(),
                });
                let attempt = Attempt {
                    attempt: self.attempt,
                    p50: e2e.map_or(f64::NAN, |t| t.p50),
                    p95: e2e.map_or(f64::NAN, |t| t.p95),
                    findings,
                };
                match self.regressions.iter_mut().find(|(n, _)| n == TUI_MODIFY) {
                    Some((_, a)) => a.push(attempt),
                    None => self.regressions.push((TUI_MODIFY.into(), vec![attempt])),
                }
            }
            println!(
                "\n{TUI_MODIFY} (attempt {}): end to end {} · Cockpit p95 {} · lost {lost}",
                self.attempt,
                e2e.map_or("-".into(), |x| format!(
                    "p50 {:.1} ms, p95 {:.1} ms (budget {budget:.0}), max {:.1} ms",
                    x.p50, x.p95, x.max
                )),
                cockpit_p95_ms.map_or("-".into(), |v| format!(
                    "{v:.1} ms (budget {COCKPIT_P95_MS:.0})"
                )),
            );
            if lost > 0 {
                self.findings.push(Finding {
                    level: Level::Fail,
                    scenario: TUI_MODIFY.into(),
                    what: format!("samples the TUI never showed within {SAMPLE_DEADLINE:?}"),
                    measured: lost as f64,
                    limit: 0.0,
                    unit: "",
                });
            }
            self.findings.extend(fs);
            self.report
                .entry("tui")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(j);
        }

        /// Bursts of `files` new files in the last worktree, created and then deleted, while
        /// "modify a file" is measured in turn in each of the other nine. Also the footprint
        /// during the bursts, the time to the final state of each burst (the worktree clean
        /// again) and how long the daemon takes to give the memory back, without a gate.
        fn burst(&mut self, name: &str, files: usize) -> BurstStats {
            let target = self.wts[self.wts.len() - 1].clone();
            let others: Vec<PathBuf> = std::iter::once(self.repo.clone())
                .chain(self.wts[..self.wts.len() - 1].iter().cloned())
                .collect();
            let pid = self.daemon.pid();
            let peak = RssPeak::start(pid);
            let cpu0 = proc_stat(pid).map(|s| s.cpu_s);
            let wall0 = Instant::now();
            let mut samples = Vec::new();
            let mut lost = 0;
            let mut finals = Vec::new();
            let mut next = 0usize;
            while samples.len() < self.opts.samples {
                let running = Arc::new(AtomicBool::new(true));
                let (r, dir) = (running.clone(), target.join("burst"));
                let writer = std::thread::spawn(move || {
                    std::fs::create_dir_all(&dir).unwrap();
                    for i in 0..files {
                        std::fs::write(dir.join(format!("f{i}.txt")), i.to_string()).unwrap();
                    }
                    std::fs::remove_dir_all(&dir).unwrap();
                    let t_last = monotonic_ns();
                    r.store(false, Ordering::Relaxed);
                    t_last
                });
                // At least one sample per burst, however short it is.
                loop {
                    let wt = others[next % others.len()].clone();
                    next += 1;
                    match self.modify_sample(&wt) {
                        Some(s) => samples.push(s),
                        None => lost += 1,
                    }
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                }
                let t_last = writer.join().unwrap();
                // Final state of the burst: the target worktree clean again.
                let ok = self.stream.until(Duration::from_secs(10), |st| {
                    st.get(&target).and_then(untracked) == Some(0)
                });
                if ok {
                    finals.push(monotonic_ns().saturating_sub(t_last) as f64 / 1e6);
                }
                self.settle();
            }
            let cpu_pct = match (cpu0, proc_stat(pid)) {
                (Some(a), Some(b)) => (b.cpu_s - a) / wall0.elapsed().as_secs_f64() * 100.0,
                _ => f64::NAN,
            };
            let peak_mib = peak.finish();
            // Retention: seconds until the RSS is back under the idle limit (at most 60 s).
            let back = Instant::now();
            let mut back_s = None;
            while back.elapsed() < Duration::from_secs(60) {
                if proc_stat(pid).is_some_and(|s| s.rss_mib < FOOTPRINT_LIMITS.idle_rss_mib) {
                    back_s = Some(back.elapsed().as_secs_f64());
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            let after_mib = proc_stat(pid).map(|s| s.rss_mib);
            println!(
                "{name}: peak RSS {peak_mib:.1} MiB, back under {} MiB after {back_s:?} s (now {after_mib:?} MiB)",
                FOOTPRINT_LIMITS.idle_rss_mib
            );
            self.record(name, true, samples, lost);
            let finals = Summary::of(&finals).map(Summary::to_json);
            let key = if self.attempt > 1 {
                format!("{name}#{}", self.attempt)
            } else {
                name.into()
            };
            self.report.insert(
                key,
                json!({"files": files, "final_state_ms": finals, "peak_rss_mib": peak_mib,
                       "cpu_pct": cpu_pct, "rss_back_under_idle_limit_s": back_s,
                       "rss_mib_after": after_mib}),
            );
            BurstStats {
                peak_mib,
                cpu_pct,
                back_s,
            }
        }

        /// 40 creations and deletions of a worktree while a writer adds files to the others:
        /// once reconciled, every file is visible (correctness gate, no latency gate).
        fn recreation(&mut self) {
            let others: Vec<PathBuf> = self.wts[2..].to_vec();
            let written = Arc::new(Mutex::new(HashMap::<PathBuf, u32>::new()));
            let running = Arc::new(AtomicBool::new(true));
            let (r, w, o) = (running.clone(), written.clone(), others.clone());
            let writer = std::thread::spawn(move || {
                let mut i = 0usize;
                while r.load(Ordering::Relaxed) {
                    let wt = &o[i % o.len()];
                    std::fs::write(wt.join(format!("rc-{i}.txt")), "x").unwrap();
                    *w.lock().unwrap().entry(wt.clone()).or_default() += 1;
                    i += 1;
                    std::thread::sleep(Duration::from_millis(15));
                }
            });
            let base = self.repo.parent().unwrap().join("wt-rc");
            for _ in 0..self.opts.recreations {
                git(
                    &self.repo,
                    &["worktree", "add", "-q", "--detach", base.to_str().unwrap()],
                );
                std::thread::sleep(Duration::from_millis(50));
                git(
                    &self.repo,
                    &["worktree", "remove", "--force", base.to_str().unwrap()],
                );
                self.stream.drain();
            }
            running.store(false, Ordering::Relaxed);
            writer.join().unwrap();
            let expected = written.lock().unwrap().clone();
            let total: u32 = expected.values().sum();
            let ok = self.stream.until(Duration::from_secs(20), |st| {
                expected
                    .iter()
                    .all(|(wt, n)| st.get(wt).and_then(untracked) == Some(*n))
            });
            let seen: u32 = expected
                .keys()
                .map(|wt| self.stream.state.get(wt).and_then(untracked).unwrap_or(0))
                .sum();
            println!(
                "stream recreation: {} creations and deletions, {seen}/{total} files visible",
                self.opts.recreations
            );
            if !ok {
                self.findings.push(Finding {
                    level: Level::Fail,
                    scenario: "stream-recreation".into(),
                    what: "files not published after reconciling".into(),
                    measured: f64::from(total - seen.min(total)),
                    limit: 0.0,
                    unit: "",
                });
            }
            self.report.insert(
                "stream_recreation".into(),
                json!({"cycles": self.opts.recreations, "written": total, "visible": seen}),
            );
        }

        /// CPU and RSS of the daemon at rest with every worktree observed.
        fn idle(&mut self) -> (f64, f64, f64, Option<u64>) {
            let pid = self.daemon.pid();
            std::thread::sleep(Duration::from_secs(3));
            self.stream.drain();
            let a = proc_stat(pid);
            let t = Instant::now();
            let peak = RssPeak::start(pid);
            std::thread::sleep(Duration::from_secs(self.opts.idle_secs));
            let b = proc_stat(pid);
            let rss = peak.finish();
            let cpu = match (a, b) {
                (Some(a), Some(b)) => (b.cpu_s - a.cpu_s) / t.elapsed().as_secs_f64() * 100.0,
                _ => f64::NAN,
            };
            let (fds, watches) = descriptors(pid);
            (cpu, rss, fds, watches)
        }
    }

    /// The scenarios one measuring step produces, so a regression is confirmed by measuring the
    /// whole step again, with its warm-up and its samples.
    #[derive(Debug, Clone, Copy)]
    enum Group {
        WriteCycle,
        Checkout,
        Worktrees,
        Burst1k,
        Burst10k,
        Tui,
    }

    impl Group {
        const ALL: [Group; 6] = [
            Group::WriteCycle,
            Group::Checkout,
            Group::Worktrees,
            Group::Tui,
            Group::Burst1k,
            Group::Burst10k,
        ];

        fn scenarios(self) -> &'static [&'static str] {
            match self {
                Group::WriteCycle => &["modify", "git-add", "commit"],
                Group::Checkout => &["checkout"],
                Group::Worktrees => &["worktree-create", "worktree-delete"],
                Group::Burst1k => &[BURST_1K],
                Group::Burst10k => &[BURST_10K],
                Group::Tui => &[TUI_MODIFY],
            }
        }
    }

    /// One attempt of a scenario at the regression gate.
    struct Attempt {
        attempt: usize,
        p50: f64,
        p95: f64,
        findings: Vec<Finding>,
    }

    /// Bytes of every file under `dir`.
    fn dir_bytes(dir: &Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => dir_bytes(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    }

    fn print_scenario(s: &Scenario, lost: usize, attempt: usize) {
        let attempt = if attempt > 1 {
            format!(", attempt {attempt}")
        } else {
            String::new()
        };
        println!(
            "\n{} ({} samples, {lost} lost{attempt})",
            s.name,
            s.samples.len()
        );
        println!(
            "  {:<10} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "stage", "p50", "p95", "p99", "max", "budget"
        );
        for stage in Stage::ALL {
            if let Some(x) = s.summary(stage) {
                let budget = s
                    .budget_ms(stage)
                    .map(|b| format!("{b:.0}"))
                    .unwrap_or_else(|| "-".into());
                println!(
                    "  {:<10} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8}",
                    stage.name(),
                    x.p50,
                    x.p95,
                    x.p99,
                    x.max,
                    budget
                );
            }
        }
    }

    /// How late `recv_timeout` wakes up, the primitive of the debounce window (ADR-GRP-010 § 3).
    fn timer_slack(samples: usize) -> Value {
        let (_tx, rx) = channel::<()>();
        let asked = Duration::from_millis(65);
        let late: Vec<f64> = (0..samples)
            .map(|_| {
                let t = Instant::now();
                let _ = rx.recv_timeout(asked);
                (t.elapsed().saturating_sub(asked)).as_secs_f64() * 1e3
            })
            .collect();
        let s = Summary::of(&late).unwrap();
        let configured = WatchConfig::default().timer_slack.as_secs_f64() * 1e3;
        println!(
            "timer slack: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms (configured {configured:.0} ms)",
            s.p50, s.p95, s.max
        );
        json!({"late_ms": s.to_json(), "configured_ms": configured})
    }

    /// Ahead/behind of a branch 50K commits from its base: `crates/git` in process against
    /// `git rev-list`, with and without `commit-graph` (ADR-GRP-010 § 4). Reported only.
    fn ahead_behind(repo: &Path, commits: usize) -> Value {
        let back = (commits / 2).to_string();
        git(repo, &["branch", "-f", "ab-base", &format!("main~{back}")]);
        let tree = git(repo, &["rev-parse", "ab-base^{tree}"]);
        let tip = git(
            repo,
            &["commit-tree", &tree, "-p", "ab-base", "-m", "base side"],
        );
        git(repo, &["update-ref", "refs/heads/ab-base", &tip]);
        let graph = repo.join(".git/objects/info/commit-graph");
        let mut out = serde_json::Map::new();
        for with_graph in [true, false] {
            if with_graph {
                git(repo, &["commit-graph", "write", "--reachable"]);
            } else {
                let _ = std::fs::remove_file(&graph);
                let _ = std::fs::remove_dir_all(repo.join(".git/objects/info/commit-graphs"));
            }
            let reader = RepoReader::open(repo, &ReaderOptions::default()).unwrap();
            let (a, b) = (
                RefName::new("main").unwrap(),
                RefName::new("ab-base").unwrap(),
            );
            let time = |f: &mut dyn FnMut() -> String| {
                let mut ms = Vec::new();
                let mut result = String::new();
                for _ in 0..5 {
                    let t = Instant::now();
                    result = f();
                    ms.push(t.elapsed().as_secs_f64() * 1e3);
                }
                json!({"result": result, "ms": Summary::of(&ms).unwrap().to_json()})
            };
            let gix = time(&mut || format!("{:?}", reader.ahead_behind(&a, &b, u64::MAX).unwrap()));
            let cli = time(&mut || {
                git(
                    repo,
                    &["rev-list", "--count", "--left-right", "main...ab-base"],
                )
            });
            let key = if with_graph {
                "with_commit_graph"
            } else {
                "without_commit_graph"
            };
            println!(
                "ahead/behind {key}: gix p50 {} ms, git rev-list p50 {} ms",
                gix["ms"]["p50"], cli["ms"]["p50"]
            );
            out.insert(key.into(), json!({"gix": gix, "git_rev_list": cli}));
        }
        git(repo, &["commit-graph", "write", "--reachable"]);
        Value::Object(out)
    }

    pub fn run() -> i32 {
        let opts = opts();
        let tmp = tempfile::tempdir().unwrap();
        let root = opts
            .root
            .clone()
            .unwrap_or_else(|| tmp.path().to_path_buf());
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let reference = reference(&root, &opts.profile);
        let commits = repogen::profile(&opts.profile).unwrap().commits;

        // A fresh run directory: clone, worktrees and profile.
        let run = root.join(format!("run-{}", std::process::id()));
        std::fs::create_dir_all(&run).unwrap();
        let repo = run.join("repo");
        git(
            &run,
            &[
                "clone",
                "-q",
                "--local",
                reference.to_str().unwrap(),
                "repo",
            ],
        );
        let repo = repo.canonicalize().unwrap();

        let mut report = serde_json::Map::new();
        report.insert("os".into(), os().into());
        report.insert("conditions".into(), conditions());
        report.insert("profile".into(), opts.profile.clone().into());
        report.insert("commits".into(), commits.into());
        report.insert("worktrees".into(), opts.worktrees.into());
        report.insert("samples".into(), opts.samples.into());
        report.insert("warmup".into(), WARMUP.into());
        let slack = timer_slack(200);
        let configured_slack = WatchConfig::default().timer_slack.as_secs_f64() * 1e3;
        let slack_excess_ms =
            (slack["late_ms"]["p95"].as_f64().unwrap_or(0.0) - configured_slack).max(0.0);
        let unfit = slack_excess_ms > MAX_SLACK_EXCESS_MS;
        println!(
            "budget of debounce and total widened by {slack_excess_ms:.1} ms of excess timer slack{}",
            if unfit {
                " — UNFIT for the latency gate, reported as warnings"
            } else {
                ""
            }
        );
        report.insert("timer_slack".into(), slack);
        report.insert(
            "slack_excess_ms".into(),
            json!({"value": slack_excess_ms, "max": MAX_SLACK_EXCESS_MS, "unfit": unfit, "platform": format!("{:?}", Platform::detect(opts.gate)), "gate": format!("{:?}", opts.gate)}),
        );
        if opts
            .only
            .as_ref()
            .is_none_or(|o| o.iter().any(|x| x == "ahead-behind"))
        {
            report.insert("ahead_behind".into(), ahead_behind(&repo, commits));
        }

        let mut wts = Vec::new();
        for i in 1..opts.worktrees {
            let path = run.join(format!("wt-{i}"));
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("b{i}"),
                    path.to_str().unwrap(),
                ],
            );
            wts.push(path.canonicalize().unwrap());
        }

        let setup = Instant::now();
        // The profile lives in a short temporary folder: the channel socket path must fit in
        // `sun_path` (104 bytes on macOS) whatever the bench root.
        let profile = tempfile::Builder::new().prefix("eb").tempdir().unwrap();
        let daemon = Daemon::start(profile.path(), &repo.join(".git"));
        let stop = Arc::new(AtomicBool::new(false));
        let sizes = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let rx = subscribe(&daemon, stop.clone(), sizes.clone());
        let mut stream = Stream {
            rx,
            state: HashMap::new(),
            received: 0,
        };
        let all: Vec<PathBuf> = std::iter::once(repo.clone())
            .chain(wts.iter().cloned())
            .collect();
        // The first state is published by the start-up reconciliation, outside NFR-04.
        let mut client = daemon.connect().unwrap();
        let mut ready = || {
            let snap: gitraptor_api::messages::Snapshot =
                client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
            let n = snap.repos.first().map(|r| r.worktrees.len()).unwrap_or(0);
            n == all.len()
        };
        let start = Instant::now();
        while !ready() {
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "repo never observed"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        drop(ready);
        stream.drain();
        println!(
            "observing {} worktrees of a {commits}-commit repo after {:.1} s",
            all.len(),
            setup.elapsed().as_secs_f64()
        );

        let profile_start = dir_bytes(profile.path());
        let mut b = Bench {
            opts,
            slack_excess_ms,
            repo,
            wts,
            stream,
            daemon,
            scenarios: Vec::new(),
            findings: Vec::new(),
            report,
            counter: 0,
            dirty: Default::default(),
            committed: HashMap::new(),
            written: HashMap::new(),
            attempt: 1,
            regressions: Vec::new(),
            checkout_ready: false,
        };

        let mut footprint = Footprint {
            idle_cpu_pct: f64::NAN,
            idle_rss_mib: f64::NAN,
            fds: f64::NAN,
            burst_rss_mib: f64::NAN,
            burst_cpu_pct: f64::NAN,
            burst_back_s: None,
        };
        let gate_footprint = b.wants("footprint") && b.wants("burst");
        if b.wants("footprint") {
            let (cpu, rss, fds, watches) = b.idle();
            footprint.idle_cpu_pct = cpu;
            footprint.idle_rss_mib = rss;
            footprint.fds = fds;
            b.report.insert("inotify_watches".into(), watches.into());
        }
        if b.wants("latency") {
            b.write_cycle();
            b.checkout();
            b.worktree_add_remove();
        }
        if b.wants("latency") || b.wants(TUI_MODIFY) {
            b.tui_modify();
        }
        if b.wants("burst") {
            let scale = b.burst(BURST_1K, b.opts.scale_files);
            let stress = b.burst(BURST_10K, b.opts.burst_files);
            footprint.burst_rss_mib = scale.peak_mib.max(stress.peak_mib);
            footprint.burst_cpu_pct = stress.cpu_pct;
            footprint.burst_back_s = stress.back_s;
        }
        // Regression gate of the shared runners (INF-GRP-002, Enmienda 2026-10-05). Before the
        // recreation, which leaves untracked files in the worktree of the bursts.
        if b.opts.gate == GateMode::SharedCi {
            b.confirm_regressions();
            let calibrated = b.platform().calibrated_slack_ms().unwrap_or(f64::INFINITY);
            let out_of_calibration = b.slack_excess_ms > calibrated;
            if out_of_calibration {
                println!(
                    "\nexcess timer slack {:.1} ms above the {calibrated:.0} ms the ceilings were calibrated with: the regression gate only warns",
                    b.slack_excess_ms
                );
            }
            let findings = b.regression_findings(out_of_calibration);
            let scenarios: serde_json::Map<String, Value> = b
                .regressions
                .iter()
                .map(|(name, attempts)| {
                    let a: Vec<Value> = attempts
                        .iter()
                        .map(|a| json!({"attempt": a.attempt, "p50": a.p50, "p95": a.p95, "regressed": !a.findings.is_empty()}))
                        .collect();
                    let ceiling = b.platform().regression_ceiling(name);
                    (
                        name.clone(),
                        json!({"attempts": a, "verdict": format!("{:?}", b.verdict(name)),
                               "p50_ceiling_ms": ceiling.map(|c| c.p50_ms),
                               "p95_ceiling_ms": ceiling.and_then(|c| c.p95_ms)}),
                    )
                })
                .collect();
            let confirmations = b.regressions.iter().filter(|(_, a)| a.len() > 1).count();
            b.report.insert(
                "regression".into(),
                json!({"platform": format!("{:?}", b.platform()), "calibrated_slack_ms": calibrated,
                       "out_of_calibration": out_of_calibration, "confirmations": confirmations,
                       "scenarios": scenarios}),
            );
            b.findings.extend(findings);
        }
        if b.wants("recreation") {
            b.recreation();
        }

        if gate_footprint {
            println!(
                "\nfootprint: idle CPU {:.2}% · idle RSS {:.1} MiB · burst peak RSS {:.1} MiB · burst CPU {:.0}% · descriptors {:.0}",
                footprint.idle_cpu_pct,
                footprint.idle_rss_mib,
                footprint.burst_rss_mib,
                footprint.burst_cpu_pct,
                footprint.fds
            );
            b.findings
                .extend(evaluate_footprint(&footprint, &FOOTPRINT_LIMITS));
            b.report.insert(
                "footprint".into(),
                json!({
                    "idle_cpu_pct": footprint.idle_cpu_pct,
                    "idle_rss_mib": footprint.idle_rss_mib,
                    "burst_peak_rss_mib": footprint.burst_rss_mib,
                    "burst_cpu_pct": footprint.burst_cpu_pct,
                    "burst_back_s": footprint.burst_back_s,
                    "descriptors": footprint.fds,
                    "limits": {
                        "idle_cpu_pct": FOOTPRINT_LIMITS.idle_cpu_pct,
                        "idle_rss_mib": FOOTPRINT_LIMITS.idle_rss_mib,
                        "burst_peak_rss_target_mib": FOOTPRINT_LIMITS.burst_rss_target_mib,
                        "burst_back_target_s": FOOTPRINT_LIMITS.burst_back_target_s,
                        "descriptors": FOOTPRINT_LIMITS.fds,
                    },
                }),
            );
        }

        stop.store(true, Ordering::Relaxed);
        // Growth of the profile (ADR-GRP-006): reported only.
        let grown = dir_bytes(profile.path()).saturating_sub(profile_start);
        let events = b.stream.received;
        println!(
            "profile: +{:.1} MiB over {events} published states ({:.0} B each)",
            grown as f64 / 1_048_576.0,
            grown as f64 / events.max(1) as f64
        );
        b.report.insert(
            "profile_growth".into(),
            json!({"bytes": grown, "worktree_state_events": events}),
        );
        // What the daemon's replay buffer holds at the end, serialized (ADR-GRP-005): the last
        // `REPLAY_EVENTS` events of the stream.
        let (replay_bytes, largest) = {
            let ring = sizes.lock().unwrap();
            (
                ring.iter().sum::<usize>(),
                ring.iter().max().copied().unwrap_or(0),
            )
        };
        println!(
            "replay buffer: last {REPLAY_EVENTS} events = {:.1} MiB serialized (largest {:.1} KiB)",
            replay_bytes as f64 / 1_048_576.0,
            largest as f64 / 1024.0
        );
        b.report.insert(
            "replay_buffer".into(),
            json!({"events": REPLAY_EVENTS, "serialized_bytes": replay_bytes, "largest_event_bytes": largest}),
        );
        let findings = std::mem::take(&mut b.findings);
        b.report.insert(
            "findings".into(),
            findings
                .iter()
                .map(|f| Value::from(f.to_string()))
                .collect(),
        );
        let out = b
            .opts
            .out
            .clone()
            .unwrap_or_else(|| root.join(format!("engine-bench-{}.json", os())));
        std::fs::write(
            &out,
            serde_json::to_string_pretty(&Value::Object(b.report.clone())).unwrap(),
        )
        .unwrap();
        write_step_summary(&b, &footprint, &findings);

        println!("\nreport: {}", out.display());
        let ci = std::env::var_os("GITHUB_ACTIONS").is_some();
        for f in &findings {
            match (f.level, ci) {
                (Level::Fail, true) => println!("::error::{f}"),
                (Level::Warn, true) => println!("::warning::{f}"),
                _ => println!("{f}"),
            }
        }
        let keep = b.opts.keep;
        drop(b);
        if !keep {
            let _ = std::fs::remove_dir_all(&run);
        }
        drop(tmp);
        if findings.iter().any(|f| f.level == Level::Fail) {
            println!("engine bench: FAILED");
            1
        } else {
            println!("engine bench: passed");
            0
        }
    }

    /// The figures as a Markdown table in the job summary of GitHub Actions.
    fn write_step_summary(b: &Bench, f: &Footprint, findings: &[Finding]) {
        let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
            return;
        };
        let mut md = format!(
            "### Engine bench ({}, profile {}, {} worktrees, commit {})\n\n| scenario | stage | p50 | p95 | p99 | max | budget p95 |\n|---|---|---|---|---|---|---|\n",
            os(),
            b.opts.profile,
            b.opts.worktrees,
            b.report["conditions"]["commit"].as_str().unwrap_or("?")
        );
        for s in &b.scenarios {
            for stage in Stage::ALL {
                if let Some(x) = s.summary(stage) {
                    let budget = s
                        .budget_ms(stage)
                        .map(|v| format!("{v:.0}"))
                        .unwrap_or_default();
                    md += &format!(
                        "| {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {budget} |\n",
                        s.name,
                        stage.name(),
                        x.p50,
                        x.p95,
                        x.p99,
                        x.max
                    );
                }
            }
        }
        if let Some(tui) = b.report.get("tui").and_then(Value::as_array) {
            md += "\n**Cockpit end to end** (`tui-modify`: t0 → t_render of the TUI, US-CKP-001)\n\n| attempt | p50 | p95 | max | budget p95 | Cockpit p95 | budget |\n|---|---|---|---|---|---|---|\n";
            for t in tui {
                let e = &t["end_to_end"];
                let ms = |v: &Value| v.as_f64().map_or("-".into(), |x| format!("{x:.1}"));
                md += &format!(
                    "| {} | {} | {} | {} | {} | {} | {} |\n",
                    t["attempt"],
                    ms(&e["p50"]),
                    ms(&e["p95"]),
                    ms(&e["max"]),
                    ms(&t["end_to_end_budget_p95_ms"]),
                    ms(&t["cockpit_p95_ms"]),
                    ms(&t["cockpit_budget_p95_ms"]),
                );
            }
        }
        if !b.regressions.is_empty() {
            md += &format!(
                "\n**Regression gate** ({:?}; the NFR-04 budget above is only reported on a shared runner)\n\n| scenario | attempt | p50 | p50 ceiling | p95 | p95 ceiling | regressed |\n|---|---|---|---|---|---|---|\n",
                b.platform()
            );
            for (name, attempts) in &b.regressions {
                let c = b.platform().regression_ceiling(name);
                let p50c = c.map(|c| format!("{:.0}", c.p50_ms)).unwrap_or("-".into());
                let p95c = c
                    .and_then(|c| c.p95_ms)
                    .map(|v| format!("{v:.0}"))
                    .unwrap_or("reported".into());
                for a in attempts {
                    md += &format!(
                        "| {name} | {} | {:.1} | {p50c} | {:.1} | {p95c} | {} |\n",
                        a.attempt,
                        a.p50,
                        a.p95,
                        if a.findings.is_empty() {
                            "no"
                        } else {
                            "**yes**"
                        }
                    );
                }
            }
        }
        md += &format!(
            "\n**Footprint**: idle CPU {:.2}% · idle RSS {:.1} MiB · burst peak RSS {:.1} MiB · burst CPU {:.0}% · descriptors {:.0}\n",
            f.idle_cpu_pct, f.idle_rss_mib, f.burst_rss_mib, f.burst_cpu_pct, f.fds
        );
        for x in findings {
            md += &format!("\n- {x}");
        }
        md.push('\n');
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
            let _ = file.write_all(md.as_bytes());
        }
    }
}
