//! SPIKE-GRP-002 — change-watcher viability at scale (isolated prototype).
//!
//! Usage: `cargo run --release -- [--quick] [--samples N] [--commits N] [--only a,b] [--out DIR] [--keep]`
//! Experiments: intact, latency, debounce, scale, polling, gaps, coalescing, persistence, ahead_behind, timer, handles.

mod bench;
mod engine;
mod repo;
mod util;

use anyhow::Result;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Args {
    quick: bool,
    samples: Option<usize>,
    commits: Option<usize>,
    only: Option<Vec<String>>,
    out: PathBuf,
    keep: bool,
}

fn parse() -> Args {
    let mut a = Args {
        quick: false,
        samples: None,
        commits: None,
        only: None,
        out: PathBuf::from("results"),
        keep: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--quick" => a.quick = true,
            "--keep" => a.keep = true,
            "--samples" => a.samples = it.next().and_then(|v| v.parse().ok()),
            "--commits" => a.commits = it.next().and_then(|v| v.parse().ok()),
            "--only" => a.only = it.next().map(|v| v.split(',').map(String::from).collect()),
            "--out" => a.out = it.next().map(PathBuf::from).unwrap_or(a.out),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    a
}

fn sh(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn environment() -> Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "os_version": if cfg!(target_os = "macos") { sh("sw_vers", &["-productVersion"]) } else { sh("uname", &["-r"]) },
        "cpu": if cfg!(target_os = "macos") { sh("sysctl", &["-n", "machdep.cpu.brand_string"]) } else { sh("sh", &["-c", "grep -m1 'model name' /proc/cpuinfo"]) },
        "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "mem": if cfg!(target_os = "macos") { sh("sysctl", &["-n", "hw.memsize"]) } else { sh("sh", &["-c", "grep MemTotal /proc/meminfo"]) },
        "git": sh("git", &["--version"]),
        "notify": "8.2.0",
        "temp_dir": std::env::temp_dir(),
    })
}

fn main() -> Result<()> {
    let a = parse();
    let commits = a.commits.unwrap_or(if a.quick { 20_000 } else { 100_000 });
    let samples = a.samples.unwrap_or(if a.quick { 40 } else { 200 });
    let gen_cfg = repo::GenCfg {
        commits,
        dirs: 250,
        files_per_dir: 20,
        worktrees: 10,
        commit_graph: true,
    };

    let base = std::env::temp_dir().join(format!("gitraptor-spike-grp-002-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    let repos = base.join("repos");
    let profile = base.join("profile");
    std::fs::create_dir_all(&profile)?;
    eprintln!(
        "[gen] {} commits, {} files, {} worktrees under {}",
        commits,
        gen_cfg.dirs * gen_cfg.files_per_dir,
        gen_cfg.worktrees,
        repos.display()
    );
    let t = util::now_ns();
    let syn = repo::generate(&repos, &gen_cfg)?;
    let gen_ms = util::ms(util::now_ns() - t);

    let mut ctx = bench::Ctx {
        syn,
        profile,
        samples,
        quick: a.quick,
        dirs: gen_cfg.dirs,
        files_per_dir: gen_cfg.files_per_dir,
        commits,
        last: Default::default(),
    };
    type Exp = fn(&mut bench::Ctx) -> Result<Value>;
    // `intact` runs first: later experiments mutate the repos on purpose.
    let all: Vec<(&str, Exp)> = vec![
        ("intact", bench::intact),
        ("latency", bench::latency),
        ("debounce", bench::debounce),
        ("scale", bench::scale),
        ("polling", bench::polling),
        ("gaps", bench::gaps),
        ("coalescing", bench::coalescing),
        ("persistence", bench::persistence),
        ("ahead_behind", bench::ahead_behind),
        ("timer", bench::timer),
        ("handles", bench::handles),
        ("stream_isolation", bench::stream_isolation),
    ];
    let mut results = serde_json::Map::new();
    results.insert("environment".into(), environment());
    results.insert(
        "generator".into(),
        json!({"cfg": gen_cfg, "gen_ms": gen_ms, "samples": samples, "quick": a.quick}),
    );
    for (name, f) in all {
        if let Some(only) = &a.only
            && !only.iter().any(|o| o == name)
        {
            continue;
        }
        eprintln!("[run] {name}");
        let t = util::now_ns();
        let v = match f(&mut ctx) {
            Ok(v) => v,
            Err(e) => json!({"error": e.to_string()}),
        };
        eprintln!(
            "[done] {name} in {:.1}s",
            util::ms(util::now_ns() - t) / 1000.0
        );
        results.insert(name.into(), v);
        // Write after each experiment so a late failure keeps earlier results.
        std::fs::create_dir_all(&a.out)?;
        std::fs::write(
            a.out.join(format!("results-{}.json", std::env::consts::OS)),
            serde_json::to_string_pretty(&Value::Object(results.clone()))?,
        )?;
    }
    if !a.keep {
        let _ = std::fs::remove_dir_all(&base);
    } else {
        eprintln!("[keep] {}", base.display());
    }
    println!(
        "{}",
        a.out
            .join(format!("results-{}.json", std::env::consts::OS))
            .display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::bench::{Ctx, cfg};
    use crate::util::{blob_oid, now_ns, pct};
    use std::time::Duration;

    #[test]
    fn blob_oid_matches_git() {
        assert_eq!(
            blob_oid(b"hello\n").to_string(),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }

    #[test]
    fn nearest_rank_percentile() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(pct(&v, 95.0), 95.0);
        assert_eq!(pct(&v, 100.0), 100.0);
        assert_eq!(pct(&[3.0], 50.0), 3.0);
    }

    /// End to end on a tiny synthetic repo in a temp dir: a modification is published
    /// with the content oid, and observing leaves the repo untouched.
    #[test]
    fn engine_publishes_modification_and_keeps_repo_intact() {
        let base =
            std::env::temp_dir().join(format!("wv-test-{}-{}", std::process::id(), now_ns()));
        let gen_cfg = crate::repo::GenCfg {
            commits: 60,
            dirs: 2,
            files_per_dir: 3,
            worktrees: 2,
            commit_graph: false,
        };
        let syn = crate::repo::generate(&base.join("repos"), &gen_cfg).unwrap();
        let mut ctx = Ctx {
            syn,
            profile: base.join("profile"),
            samples: 1,
            quick: true,
            dirs: 2,
            files_per_dir: 3,
            commits: 60,
            last: Default::default(),
        };
        let before = crate::repo::fingerprint(&ctx.syn.base);
        let eng = ctx.start(cfg(&ctx, 75, false, None)).unwrap();
        assert_eq!(eng.worktree_names().len(), 2);
        let after = crate::repo::fingerprint(&ctx.syn.base);
        assert_eq!(before, after, "observing must not write into the repo");

        let f = crate::repo::file_path(1, 2);
        let body = b"changed\n";
        std::fs::write(ctx.root("wt-01").join(&f), body).unwrap();
        let oid = blob_oid(body).to_string();
        let p = ctx
            .wait_for(&eng, 5000, |p| {
                p.wt == "wt-01"
                    && p.changed
                        .iter()
                        .any(|(path, c, o)| *path == f && *c == 'M' && *o == oid)
            })
            .expect("modification published");
        assert_eq!(p.dirty, 1);
        assert!(p.t_flush >= p.t_recv && p.t_persisted >= p.t_computed);
        eng.stop();
        std::thread::sleep(Duration::from_millis(50));
        let _ = std::fs::remove_dir_all(&base);
    }
}
