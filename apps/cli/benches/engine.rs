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
//! ENGINE_BENCH_ROOT=/scratch cargo bench -p gitraptor-cli --bench engine -- --keep
//! ```
//!
//! Gates (exit code 1): the p95 of the engine total (`t0` → `t_client_recv`) above 300 ms in any
//! scenario; the footprint of the isolated daemon above its limits; a change lost by the stream
//! recreation. A stage over its budget with the total within only warns, naming the stage.
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
    use gitraptor_core::channel::AgentMatcher;
    use gitraptor_core::client::Client;
    use gitraptor_core::daemon::{self, DaemonConfig};
    use gitraptor_core::profile::{Profile, ProfileDirs};
    use gitraptor_core::watch::WatchConfig;
    use gitraptor_git::{ReaderOptions, RefName, RepoReader};
    use gitraptor_testkit::fixture::git_from_path;
    use gitraptor_testkit::freshness::{
        FOOTPRINT_LIMITS, Finding, Footprint, Level, Sample, Scenario, Stage, Summary,
        evaluate_footprint, evaluate_latency,
    };
    use gitraptor_testkit::repogen;
    use serde_json::{Value, json};

    /// Executable name no process of the bench has: no client is classified as an agent, as
    /// with `GITRAPTOR_AGENT_EXECUTABLES` in the tests.
    const NO_AGENT: &str = "raptor-fake-agent";
    const SEED: u64 = 0x1f_9002;
    /// Samples dropped at the start of every scenario (ADR-GRP-011 § 4).
    const WARMUP: usize = 10;
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
        root: Option<PathBuf>,
        out: Option<PathBuf>,
        only: Option<Vec<String>>,
        keep: bool,
    }

    fn opts() -> Opts {
        let mut o = Opts {
            profile: "H".into(),
            samples: 200,
            worktrees: 10,
            idle_secs: 30,
            recreations: 40,
            burst_files: 10_000,
            root: std::env::var_os("ENGINE_BENCH_ROOT").map(PathBuf::from),
            out: None,
            only: None,
            keep: false,
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
                "--root" => o.root = Some(PathBuf::from(next)),
                "--out" => o.out = Some(PathBuf::from(next)),
                "--only" => o.only = Some(next.split(',').map(str::to_owned).collect()),
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
    fn subscribe(daemon: &Daemon, stop: Arc<AtomicBool>) -> Receiver<Received> {
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
    }

    impl Stream {
        fn absorb(&mut self, r: &Received) {
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

    // ------------------------------------------------------------ the bench

    struct Bench {
        opts: Opts,
        repo: PathBuf,
        /// Linked worktrees `wt-1`..`wt-n`, canonical.
        wts: Vec<PathBuf>,
        stream: Stream,
        daemon: Daemon,
        scenarios: Vec<Scenario>,
        findings: Vec<Finding>,
        report: serde_json::Map<String, Value>,
        /// Next content of the touched file, per worktree.
        counter: u64,
    }

    impl Bench {
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
                samples: kept,
            };
            let mut j = s.to_json();
            j["lost"] = lost.into();
            let fs = evaluate_latency(&s);
            print_scenario(&s, lost);
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
        fn touch(&mut self, wt: &Path) -> u64 {
            self.counter += 1;
            std::fs::write(wt.join(TOUCHED), format!("{}\n", self.counter)).unwrap();
            monotonic_ns()
        }

        fn settle(&mut self) {
            std::thread::sleep(SETTLE);
            self.stream.drain();
        }

        /// One "modify a file" sample in `wt`: the change becomes visible as unstaged.
        fn modify_sample(&mut self, wt: &Path) -> Option<Sample> {
            self.stream.drain();
            let since = monotonic_ns();
            let t0 = self.touch(wt);
            let s = self.stream.wait(since, t0, |d| {
                area_of(d, wt, TOUCHED) == Some(ChangeAreaView::Unstaged)
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
                self.settle();
            }
            self.record("modify", true, modify, lost[0]);
            self.record("git-add", false, add, lost[1]);
            self.record("commit", false, commit, lost[2]);
        }

        /// Checkout between two branches one commit apart, on `wt-2`.
        fn checkout(&mut self) {
            let wt = self.wts[1].clone();
            git(&wt, &["switch", "-q", "-c", "co-a"]);
            std::fs::write(wt.join(TOUCHED), "checkout\n").unwrap();
            git(&wt, &["commit", "-qam", "co-b"]);
            git(&wt, &["branch", "co-b"]);
            git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
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

        /// Bursts of `burst_files` new files in the last worktree while "modify a file" is
        /// measured in `wt-1`. Also the footprint during the bursts and the time to the final
        /// state of each burst (clean again after deleting them), reported without a gate.
        fn burst(&mut self) -> (f64, f64) {
            let wt = self.wts[0].clone();
            let target = self.wts[self.wts.len() - 1].clone();
            let files = self.opts.burst_files;
            let pid = self.daemon.pid();
            let peak = RssPeak::start(pid);
            let cpu0 = proc_stat(pid).map(|s| s.cpu_s);
            let wall0 = Instant::now();
            let mut samples = Vec::new();
            let mut lost = 0;
            let mut finals = Vec::new();
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
                while running.load(Ordering::Relaxed) {
                    match self.modify_sample(&wt) {
                        Some(s) => samples.push(s),
                        None => lost += 1,
                    }
                }
                let t_last = writer.join().unwrap();
                // Final state of the burst: the target worktree clean again.
                let ok = self.stream.until(Duration::from_secs(10), |st| {
                    st.get(&target).and_then(untracked) == Some(0)
                });
                if ok {
                    finals.push((monotonic_ns() - t_last) as f64 / 1e6);
                }
                self.settle();
            }
            let cpu_pct = match (cpu0, proc_stat(pid)) {
                (Some(a), Some(b)) => (b.cpu_s - a) / wall0.elapsed().as_secs_f64() * 100.0,
                _ => f64::NAN,
            };
            let peak_mib = peak.finish();
            // What the daemon keeps once the bursts are over: retention, not a gate.
            std::thread::sleep(Duration::from_secs(10));
            let after_mib = proc_stat(pid).map(|s| s.rss_mib);
            println!("after the bursts: RSS {after_mib:?} MiB");
            self.record("burst-other-worktree", true, samples, lost);
            let finals = Summary::of(&finals).map(Summary::to_json);
            self.report.insert(
                "burst".into(),
                json!({"files": files, "final_state_ms": finals, "worktree": "last",
                       "rss_mib_10s_after": after_mib}),
            );
            (peak_mib, cpu_pct)
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

    fn print_scenario(s: &Scenario, lost: usize) {
        println!("\n{} ({} samples, {lost} lost)", s.name, s.samples.len());
        println!(
            "  {:<10} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "stage", "p50", "p95", "p99", "max", "budget"
        );
        for stage in Stage::ALL {
            if let Some(x) = s.summary(stage) {
                let budget = stage
                    .budget_ms()
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
        report.insert("profile".into(), opts.profile.clone().into());
        report.insert("commits".into(), commits.into());
        report.insert("worktrees".into(), opts.worktrees.into());
        report.insert("samples".into(), opts.samples.into());
        report.insert("warmup".into(), WARMUP.into());
        report.insert("timer_slack".into(), timer_slack(200));
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
        let rx = subscribe(&daemon, stop.clone());
        let mut stream = Stream {
            rx,
            state: HashMap::new(),
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

        let mut b = Bench {
            opts,
            repo,
            wts,
            stream,
            daemon,
            scenarios: Vec::new(),
            findings: Vec::new(),
            report,
            counter: 0,
        };

        let mut footprint = Footprint {
            idle_cpu_pct: f64::NAN,
            idle_rss_mib: f64::NAN,
            burst_rss_mib: f64::NAN,
            burst_cpu_pct: f64::NAN,
            fds: f64::NAN,
        };
        let mut gate_footprint = false;
        if b.wants("footprint") {
            let (cpu, rss, fds, watches) = b.idle();
            footprint.idle_cpu_pct = cpu;
            footprint.idle_rss_mib = rss;
            footprint.fds = fds;
            b.report.insert("inotify_watches".into(), watches.into());
            gate_footprint = true;
        }
        if b.wants("latency") {
            b.write_cycle();
            b.checkout();
            b.worktree_add_remove();
        }
        if b.wants("burst") {
            let (peak, cpu) = b.burst();
            footprint.burst_rss_mib = peak;
            footprint.burst_cpu_pct = cpu;
        } else {
            footprint.burst_rss_mib = 0.0;
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
                    "descriptors": footprint.fds,
                    "limits": {
                        "idle_cpu_pct": FOOTPRINT_LIMITS.idle_cpu_pct,
                        "idle_rss_mib": FOOTPRINT_LIMITS.idle_rss_mib,
                        "burst_peak_rss_mib": FOOTPRINT_LIMITS.burst_rss_mib,
                        "descriptors": FOOTPRINT_LIMITS.fds,
                    },
                }),
            );
        }

        stop.store(true, Ordering::Relaxed);
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
            "### Engine bench ({}, profile {}, {} worktrees)\n\n| scenario | stage | p50 | p95 | p99 | max | budget p95 |\n|---|---|---|---|---|---|---|\n",
            os(),
            b.opts.profile,
            b.opts.worktrees
        );
        for s in &b.scenarios {
            for stage in Stage::ALL {
                if let Some(x) = s.summary(stage) {
                    let budget = stage
                        .budget_ms()
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
