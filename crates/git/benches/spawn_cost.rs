//! Cost of losing `posix_spawn` (DS-TS-GRP-005 T011, D9): `git --version` p50/p95 with and
//! without the `pre_exec` of `gitraptor_macsys::qos::lower_child`, launched from a process
//! holding ~150 MiB of resident memory (the RES-02 target), where a `fork` has the most pages
//! to map. Fails when the p95 overhead per child exceeds `SPAWN_COST_BUDGET`.
//!
//! `cargo bench -p gitraptor-git --bench spawn_cost`. XP-45: macOS only on purpose; elsewhere
//! it says so and exits cleanly (no class is applied there, D4).

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("spawn_cost: macOS only (no child class is applied on this OS)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::process::{Command, ExitCode, Stdio};
    use std::time::{Duration, Instant};

    use gitraptor_macsys::qos::{self, QosClass};

    /// Most a lowered child may add to the p95 of a spawn (⚠️ ASSUMPTION of T011).
    const SPAWN_COST_BUDGET: Duration = Duration::from_millis(5);
    /// Resident memory the parent holds while it spawns (RES-02).
    const RSS_BYTES: usize = 150 * 1024 * 1024;
    const WARMUP: usize = 10;
    const SAMPLES: usize = 200;

    pub(super) fn run() -> ExitCode {
        let git = gitraptor_testkit::fixture::git_from_path();
        // Touch every page so the memory is resident, not just reserved.
        let mut ballast = vec![0u8; RSS_BYTES];
        for page in ballast.chunks_mut(4096) {
            page[0] = 1;
        }
        std::hint::black_box(&ballast);

        let spawn = |lowered: bool| -> Duration {
            let mut command = Command::new(&git);
            command
                .arg("--version")
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if lowered {
                qos::lower_child(&mut command, QosClass::Utility);
            }
            let started = Instant::now();
            match command.status() {
                Ok(status) if status.success() => started.elapsed(),
                Ok(status) => panic!("git --version exited with {status}"),
                Err(e) => panic!("could not start git: {e}"),
            }
        };

        for _ in 0..WARMUP {
            spawn(false);
            spawn(true);
        }
        // Interleaved, so a drift of the machine weighs on both series alike.
        let mut plain = Vec::with_capacity(SAMPLES);
        let mut lowered = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            plain.push(spawn(false));
            lowered.push(spawn(true));
        }
        std::hint::black_box(&ballast);

        let (p50_plain, p95_plain) = percentiles(&mut plain);
        let (p50_low, p95_low) = percentiles(&mut lowered);
        let overhead = p95_low.saturating_sub(p95_plain);
        println!(
            "spawn_cost: rss_held_mib={} samples={SAMPLES}",
            RSS_BYTES / (1024 * 1024)
        );
        println!(
            "spawn_cost: posix_spawn  p50={:.2}ms p95={:.2}ms",
            ms(p50_plain),
            ms(p95_plain)
        );
        println!(
            "spawn_cost: pre_exec     p50={:.2}ms p95={:.2}ms",
            ms(p50_low),
            ms(p95_low)
        );
        println!(
            "spawn_cost: p95 overhead={:.2}ms budget={:.2}ms",
            ms(overhead),
            ms(SPAWN_COST_BUDGET)
        );
        if overhead > SPAWN_COST_BUDGET {
            eprintln!(
                "spawn_cost: FAIL, the p95 overhead of lower_child exceeds SPAWN_COST_BUDGET"
            );
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    }

    /// (p50, p95) by nearest rank.
    fn percentiles(samples: &mut [Duration]) -> (Duration, Duration) {
        samples.sort_unstable();
        let at = |q: usize| samples[(samples.len() * q).div_ceil(100).saturating_sub(1)];
        (at(50), at(95))
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }
}
