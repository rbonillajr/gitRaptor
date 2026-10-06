// Wall-clock benchmark of one command, without a shell around the timed run
// (SPIKE-GRD-001, suite 07). Portable replacement of `bench.py` for machines
// without Python (the Windows runner): built with `rustc -O` into the sandbox.
//
// usage: bench <iterations> <cwd> <setup-sh|-> <program> [args...]
// Runs <setup-sh> with `sh -c` (untimed, if not '-') and then <program>
// (timed, spawned directly) <iterations> times in <cwd>, after BENCH_WARMUP
// (default 3) warm-up runs. Prints one TSV line: n p50_ms p95_ms max_ms failures
use std::process::{Command, Stdio};
use std::time::Instant;

fn quiet(mut c: Command, cwd: &str) -> i32 {
    c.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c.status().map(|s| s.code().unwrap_or(-1)).unwrap_or(-2)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: bench <n> <cwd> <setup-sh|-> <program> [args...]");
        std::process::exit(2);
    }
    let n: usize = a[1].parse().expect("iterations");
    let (cwd, setup) = (&a[2], &a[3]);
    let warmup: usize = std::env::var("BENCH_WARMUP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let mut samples = Vec::with_capacity(n);
    let mut failures = 0;
    for i in 0..n + warmup {
        if setup != "-" {
            let mut c = Command::new("sh");
            c.args(["-c", setup]);
            quiet(c, cwd);
        }
        let mut c = Command::new(&a[4]);
        c.args(&a[5..]);
        let t0 = Instant::now();
        let rc = quiet(c, cwd);
        let dt = t0.elapsed().as_secs_f64() * 1000.0;
        if i < warmup {
            continue;
        }
        failures += usize::from(rc != 0);
        samples.push(dt);
    }
    samples.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let p = |q: f64| samples[((q * (samples.len() - 1) as f64).round() as usize).min(samples.len() - 1)];
    println!(
        "{n}\t{:.2}\t{:.2}\t{:.2}\t{failures}",
        p(0.50),
        p(0.95),
        samples[samples.len() - 1]
    );
}
