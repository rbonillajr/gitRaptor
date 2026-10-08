//! CPU of the daemon at rest with agent sessions present (RES-01, M1 criterion 5, INF-GRP-002).
//!
//! The footprint of `engine` measures the daemon at rest with no session at all; the dogfooding
//! log (2026-10-08) measured 1.59 % with 9 Claude Code sessions. What a session adds at rest is
//! the S1 scan of the detector (ADR-GRP-012), which reads the whole process table of the user
//! every second, so this bench reproduces both: `N` agent sessions over `M` worktrees and `P`
//! extra processes of the user on top of whatever the machine already runs.
//!
//! An isolated daemon over a temporary profile (NFR-01): this binary re-executed with
//! `--daemon-root`, as `engine` does. Each session is a copy of this binary named after the
//! simulated agent ([`AGENT`]) and run with `--agent`, started inside a worktree (a copy of a
//! system binary such as `/bin/sleep` is killed by the macOS code signing); the bench waits until the daemon
//! lists the `N` sessions, lets it settle and reads its CPU time over the window.
//!
//! ```sh
//! cargo bench -p gitraptor-cli --bench idle                                  # N=10, M=10, 60 s
//! cargo bench -p gitraptor-cli --bench idle -- --sessions 10 --worktrees 10 --extra-procs 500
//! cargo bench -p gitraptor-cli --bench idle -- --idle-secs 600 --json idle.json
//! cargo bench -p gitraptor-cli --bench idle -- --churn 2000                  # build churn
//! cargo bench -p gitraptor-cli --bench idle -- --protected                   # hooks installed
//! ```
//!
//! `--churn F` writes `F` files per second, spread over the worktrees, under their ignored
//! `target/` (what a `cargo build` of an agent does): Git sees no change, so the daemon is still
//! at rest by RES-01, but its watchers receive every event. Without a CPU gate: it reports the
//! cost of discarding ignored events (ADR-GRP-010 § 2) next to the true rest.
//!
//! `--protected` installs the Guardrails hook layer in the repo before the window (a temporary
//! repo and profile, like everything here), so the periodic check that the protection is still
//! active (US-GRD-004: the fingerprint of a few `stat`s every minute, the full check only when it
//! moves) is part of the rest that is measured. Compare it with the same run without the flag.
//! The install is a reserved command: it runs through `raptor guard install --yes` in a terminal
//! (a pty through `script`), with the **debug** `raptor` named by `GITRAPTOR_BENCH_RAPTOR`, the
//! only build that honors the temporary profile (`GITRAPTOR_PROFILE_DIR`, NFR-01; a release one
//! would install in the real profile, so the bench refuses it).
//!
//! ```sh
//! cargo build -p gitraptor-cli --bin raptor --bin raptor-hook
//! GITRAPTOR_BENCH_RAPTOR=$PWD/target/debug/raptor cargo bench -p gitraptor-cli --bench idle -- --protected
//! ```
//!
//! **Fails** (exit code 1) when the mean CPU over the window reaches the idle limit of the
//! footprint (`FOOTPRINT_LIMITS.idle_cpu_pct`, RES-01) or a session is not detected.

#[cfg(not(unix))]
fn main() {
    // The channel client only exists on Unix (TS-GRP-004).
    // Pendiente: etapa de validación multiplataforma.
    println!("idle bench: skipped, the channel client is Unix-only for now");
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--agent") {
        // A simulated session: alive until the bench kills it.
        loop {
            std::thread::park();
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--daemon-root") {
        std::process::exit(unix::daemon_main(std::path::Path::new(&args[i + 1])));
    }
    std::process::exit(unix::run());
}

#[cfg(unix)]
mod unix {
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use gitraptor_api::PROTOCOL_VERSION;
    use gitraptor_api::messages::{ClientKind, SessionsListParams, SessionsListResult};
    use gitraptor_api::methods;
    use gitraptor_core::channel::AgentMatcher;
    use gitraptor_core::client::Client;
    use gitraptor_core::daemon::{self, DaemonConfig};
    use gitraptor_core::profile::{Profile, ProfileDirs};
    use gitraptor_testkit::freshness::FOOTPRINT_LIMITS;
    use serde_json::json;

    /// Executable name of the simulated agent sessions: only it is classified as an agent.
    const AGENT: &str = "raptor-bench-agent";
    /// Longest wait for the daemon to list every session.
    const DETECT_DEADLINE: Duration = Duration::from_secs(30);
    /// Pause after the sessions are listed, so the window starts at rest.
    const SETTLE: Duration = Duration::from_secs(5);

    struct Opts {
        sessions: usize,
        worktrees: usize,
        extra_procs: usize,
        idle_secs: u64,
        churn: u32,
        /// Install the hook layer in the repo before the window (US-GRD-004).
        protected: bool,
        json: Option<PathBuf>,
    }

    fn opts() -> Opts {
        let mut o = Opts {
            sessions: 10,
            worktrees: 10,
            extra_procs: 0,
            idle_secs: 60,
            churn: 0,
            protected: false,
            json: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            let mut next = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
            match a.as_str() {
                "--sessions" => o.sessions = next().parse().expect("--sessions N"),
                "--worktrees" => o.worktrees = next().parse().expect("--worktrees M"),
                "--extra-procs" => o.extra_procs = next().parse().expect("--extra-procs P"),
                "--idle-secs" => o.idle_secs = next().parse().expect("--idle-secs S"),
                "--churn" => o.churn = next().parse().expect("--churn F"),
                "--protected" => o.protected = true,
                "--json" => o.json = Some(PathBuf::from(next())),
                // `cargo bench` passes `--bench`.
                _ => {}
            }
        }
        assert!(o.worktrees >= 1, "--worktrees must be at least 1");
        o
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn private_dir(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn now_ms() -> i64 {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }

    /// A small repo with `m` worktrees: the main one and `m - 1` linked ones.
    fn repo(root: &Path, m: usize) -> Vec<PathBuf> {
        let main = root.join("repo");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.name", "bench"]);
        git(&main, &["config", "user.email", "bench@example.invalid"]);
        for i in 0..50 {
            std::fs::write(main.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
        }
        std::fs::write(main.join(".gitignore"), "target/\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-qm", "init"]);
        let mut all = vec![main.clone()];
        for i in 1..m {
            let wt = root.join(format!("wt{i}"));
            git(
                &main,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("b{i}"),
                    wt.to_str().unwrap(),
                ],
            );
            all.push(wt);
        }
        all.into_iter().map(|p| p.canonicalize().unwrap()).collect()
    }

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
            let mut d = Self {
                child: Command::new(std::env::current_exe().unwrap())
                    .arg("--daemon-root")
                    .arg(profile_root)
                    .env_clear()
                    .env("HOME", profile_root)
                    .env("PATH", "/usr/bin:/bin:/usr/local/bin")
                    .envs(
                        std::env::var_os("GITRAPTOR_BENCH_RAPTOR")
                            .map(|v| ("GITRAPTOR_BENCH_RAPTOR", v)),
                    )
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap(),
                dirs,
            };
            let start = Instant::now();
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

        fn present_sessions(&self) -> usize {
            let Some(mut client) = self.connect() else {
                return 0;
            };
            client
                .call::<_, SessionsListResult>(
                    methods::SESSIONS_LIST,
                    SessionsListParams::default(),
                )
                .map(|r| {
                    r.sessions
                        .iter()
                        .filter(|s| s.ended_utc_ms.is_none())
                        .count()
                })
                .unwrap_or(0)
        }
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = Command::new("/bin/kill")
                .arg(self.child.id().to_string())
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

    /// The daemon process: the configuration of `raptor daemon`, on the temporary profile, with
    /// only the simulated agent classified as one.
    pub fn daemon_main(root: &Path) -> i32 {
        let mut config = match DaemonConfig::for_current_user() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("bench daemon: {e}");
                return 1;
            }
        };
        config.dirs = ProfileDirs::under_root(root);
        config.channel.agents = AgentMatcher::only(vec![AGENT.into()]);
        if let Some(raptor) = std::env::var_os("GITRAPTOR_BENCH_RAPTOR") {
            // The installed `raptor` the dispatchers start (`--protected`).
            config.channel.launch_exe = Some(PathBuf::from(raptor));
        }
        match daemon::run_process(config) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("bench daemon: {e}");
                1
            }
        }
    }

    /// Children killed when the bench ends, whatever way it ends.
    struct Procs(Vec<Child>);

    impl Drop for Procs {
        fn drop(&mut self) {
            for c in &mut self.0 {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    /// Writes `per_s` files per second under the ignored `target/` of the worktrees until
    /// dropped, rewriting the same 256 names per worktree so the disk stays bounded.
    struct Churn {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<u64>>,
    }

    impl Churn {
        fn start(worktrees: &[PathBuf], per_s: u32) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let dirs: Vec<PathBuf> = worktrees
                .iter()
                .map(|w| w.join("target/debug/deps"))
                .collect();
            for d in &dirs {
                std::fs::create_dir_all(d).unwrap();
            }
            let flag = Arc::clone(&stop);
            let thread = std::thread::spawn(move || {
                // Ticks of 10 ms, each with its share of the rate.
                let tick = Duration::from_millis(10);
                let per_tick = per_s.div_ceil(100).max(1);
                let mut written = 0u64;
                let mut next = Instant::now();
                while !flag.load(Ordering::Relaxed) {
                    for _ in 0..per_tick {
                        let d = &dirs[(written as usize) % dirs.len()];
                        let name =
                            d.join(format!("lib{}.rmeta", (written / dirs.len() as u64) % 256));
                        std::fs::write(name, written.to_ne_bytes()).unwrap();
                        written += 1;
                    }
                    next += tick;
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
                written
            });
            Self {
                stop,
                thread: Some(thread),
            }
        }

        fn finish(mut self) -> u64 {
            self.stop.store(true, Ordering::Relaxed);
            self.thread.take().map_or(0, |t| t.join().unwrap())
        }
    }

    /// `raptor guard install --yes` of the repo, in a terminal (the command is reserved to the
    /// developer), against the temporary profile of the bench.
    fn install_hooks(raptor: &Path, profile: &Path, repo: &Path) -> Result<(), String> {
        let out = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null"])
            .arg(raptor)
            .args(["guard", "install", "--yes"])
            .arg(repo)
            .env_clear()
            .env("GITRAPTOR_PROFILE_DIR", profile)
            .env("HOME", profile)
            .env("PATH", "/usr/bin:/bin:/usr/local/bin")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stdout).into_owned())
        }
    }

    /// CPU time of `pid` in seconds, from `ps` (no `unsafe`; 10 ms resolution).
    fn cpu_s(pid: u32) -> Option<f64> {
        let out = Command::new("/bin/ps")
            .args(["-o", "time=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let time = text.trim();
        let (days, time) = match time.split_once('-') {
            Some((d, t)) => (d.parse::<f64>().ok()?, t),
            None => (0.0, time),
        };
        let mut s = days * 86_400.0;
        for part in time.split(':') {
            s = s * 60.0 + part.parse::<f64>().ok()?;
        }
        Some(s)
    }

    /// Processes of the current user, as the detector's table sees them.
    fn user_procs() -> usize {
        Command::new("/bin/ps")
            .args(["-U", &user_id(), "-o", "pid="])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0)
    }

    fn user_id() -> String {
        let out = Command::new("/usr/bin/id").arg("-u").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    pub fn run() -> i32 {
        let o = opts();
        let tmp = tempfile::Builder::new()
            .prefix("raptor-idle-bench-")
            .tempdir()
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let worktrees = repo(&root, o.worktrees);
        let raptor = std::env::var_os("GITRAPTOR_BENCH_RAPTOR").map(PathBuf::from);
        if o.protected && raptor.is_none() {
            eprintln!("idle: --protected needs GITRAPTOR_BENCH_RAPTOR (a debug build of raptor)");
            return 1;
        }
        let daemon = Daemon::start(&root.join("profile"), &worktrees[0].join(".git"));
        if o.protected {
            let raptor = raptor.unwrap_or_default();
            if let Err(why) = install_hooks(&raptor, &root.join("profile"), &worktrees[0]) {
                eprintln!("idle: the hook layer was not installed: {why}");
                return 1;
            }
        }

        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let agent = bin.join(AGENT);
        std::fs::copy(std::env::current_exe().unwrap(), &agent).unwrap();
        let mut procs = Procs(Vec::new());
        for i in 0..o.sessions {
            procs.0.push(
                Command::new(&agent)
                    .arg("--agent")
                    .current_dir(&worktrees[i % worktrees.len()])
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
        }
        for _ in 0..o.extra_procs {
            procs.0.push(
                Command::new("/bin/sleep")
                    .arg("100000")
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
        }
        let start = Instant::now();
        let mut seen = daemon.present_sessions();
        while seen < o.sessions && start.elapsed() < DETECT_DEADLINE {
            std::thread::sleep(Duration::from_millis(200));
            seen = daemon.present_sessions();
        }
        std::thread::sleep(SETTLE);

        let pid = daemon.child.id();
        let table = user_procs();
        let churn = (o.churn > 0).then(|| Churn::start(&worktrees, o.churn));
        if churn.is_some() {
            std::thread::sleep(SETTLE);
        }
        let a = cpu_s(pid);
        let t = Instant::now();
        std::thread::sleep(Duration::from_secs(o.idle_secs));
        let b = cpu_s(pid);
        let window = t.elapsed().as_secs_f64();
        let churned = churn.map_or(0, Churn::finish);
        let cpu_pct = match (a, b) {
            (Some(a), Some(b)) => (b - a) / window * 100.0,
            _ => f64::NAN,
        };
        let limit = FOOTPRINT_LIMITS.idle_cpu_pct;
        let protected = if o.protected {
            " · hooks installed"
        } else {
            ""
        };
        let mode = if o.churn > 0 {
            format!(
                " · churn {churned} files in ignored target/ (asked {} /s)",
                o.churn
            )
        } else {
            String::new()
        };
        println!(
            "idle: CPU {cpu_pct:.3} % over {window:.0} s · {seen}/{} sessions · {} worktrees · {table} processes of the user ({} extra){protected}{mode} · limit < {limit} %",
            o.sessions,
            worktrees.len(),
            o.extra_procs,
        );
        if let Some(path) = &o.json {
            let report = json!({
                "os": std::env::consts::OS,
                "idle_cpu_pct": cpu_pct,
                "window_s": window,
                "sessions": o.sessions,
                "sessions_detected": seen,
                "worktrees": worktrees.len(),
                "extra_procs": o.extra_procs,
                "user_procs": table,
                "protected": o.protected,
                "churn_per_s": o.churn,
                "churn_files": churned,
                "limit_idle_cpu_pct": limit,
            });
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        drop(procs);
        drop(daemon);
        let mut failed = false;
        if seen < o.sessions {
            eprintln!("idle: only {seen} of {} sessions detected", o.sessions);
            failed = true;
        }
        // Churn is reported, not gated: it measures the cost of discarding ignored events.
        if o.churn == 0 && (cpu_pct.is_nan() || cpu_pct >= limit) {
            eprintln!("idle: CPU at rest {cpu_pct:.3} % reaches the limit < {limit} % (RES-01)");
            failed = true;
        }
        i32::from(failed)
    }
}
