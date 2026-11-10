//! Bench of `diff.commit` and `diff.index` on the history of NFR-05 (DS-TS-CKP-006 T006,
//! INF-GRP-002).
//!
//! Generates the 100K-commit repo (profile `H`) with the deterministic generator of
//! `gitraptor_testkit::repogen` and measures, with the caps of US-CKP-012:
//!
//! - `RepoReader::commit_diff` on a typical commit (`HEAD`), budget p95 <= 1,000 ms;
//! - `RepoReader::commit_diff` on a nonexistent oid (a full walk, or the deadline), p95 <= 2,000 ms;
//! - `RepoReader::index_diff` with 20 changed files, p95 <= 500 ms.
//!
//! It prints p50 and p95 of each and says whether the budget of § Valores numéricos holds; the
//! gate is read by a person on a release build, so it never fails the run.
//!
//! ```sh
//! cargo bench -p gitraptor-core --bench diff_commit                     # profile H
//! cargo bench -p gitraptor-core --bench diff_commit -- --profile S --iters 5
//! DIFF_BENCH_ROOT=/scratch cargo bench -p gitraptor-core --bench diff_commit
//! ```
//!
//! Everything lives under a root outside any Git repo (NFR-01): `DIFF_BENCH_ROOT` or a temporary
//! folder. The generated repo is kept there and reused by later runs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_api::diff::{
    DIFF_DEADLINE_MS, MAX_DIFF_BLOB_BYTES, MAX_DIFF_FILES, MAX_DIFF_LINES_PER_FILE,
    MAX_DIFF_RESPONSE_BYTES,
};
use gitraptor_git::diff::{DiffLimits, Lookup};
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::repogen;

/// Files changed in the worktree for the `diff.index` case.
const CHANGED: usize = 20;

struct Opts {
    profile: String,
    iters: usize,
    root: Option<PathBuf>,
}

fn opts() -> Opts {
    let mut o = Opts {
        profile: "H".into(),
        iters: 20,
        root: std::env::var_os("DIFF_BENCH_ROOT").map(PathBuf::from),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let next = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--profile" => o.profile = next,
            "--iters" => o.iters = next.parse().expect("--iters N"),
            "--root" => o.root = Some(PathBuf::from(next)),
            // `cargo bench` passes `--bench`; anything else unknown is ignored too.
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    o
}

fn git_bin() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = repogen::git_command(&git_bin(), dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn limits() -> DiffLimits {
    DiffLimits {
        max_files: MAX_DIFF_FILES,
        max_lines_per_file: MAX_DIFF_LINES_PER_FILE,
        max_blob_bytes: MAX_DIFF_BLOB_BYTES,
        max_bytes: MAX_DIFF_RESPONSE_BYTES,
        deadline: Instant::now() + Duration::from_millis(DIFF_DEADLINE_MS),
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn measure(name: &str, iters: usize, budget_ms: f64, mut run: impl FnMut()) {
    let mut samples: Vec<f64> = (0..iters)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    let (p50, p95) = (percentile(&samples, 50.0), percentile(&samples, 95.0));
    let verdict = if p95 <= budget_ms {
        "ok"
    } else {
        "SOBRE EL PRESUPUESTO"
    };
    println!(
        "{name}: p50 {p50:.1} ms · p95 {p95:.1} ms (presupuesto p95 <= {budget_ms:.0} ms: {verdict})"
    );
}

fn main() {
    let o = opts();
    let profile = repogen::profile(&o.profile).expect("profile S|P50|M|L|H");
    let tmp;
    let root = match &o.root {
        Some(r) => {
            std::fs::create_dir_all(r).unwrap();
            r.canonicalize().unwrap()
        }
        None => {
            tmp = tempfile::tempdir().unwrap();
            tmp.path().canonicalize().unwrap()
        }
    };
    // NFR-01: never inside a Git repository (this one included).
    let inside = std::process::Command::new(git_bin())
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(!inside, "{} is inside a Git repository", root.display());

    let repo = root.join(format!("repo-{}", profile.name));
    let marker = root.join(format!("repo-{}.generated", profile.name));
    if !marker.exists() {
        let _ = std::fs::remove_dir_all(&repo);
        println!(
            "generando el perfil {} en {} …",
            profile.name,
            repo.display()
        );
        let g = repogen::generate(&git_bin(), &profile, &repo, 42).unwrap();
        std::fs::write(
            &marker,
            format!("{} HEAD={} commits={}", profile.name, g.head, g.commits),
        )
        .unwrap();
    }
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let reader = RepoReader::open(&repo, &ReaderOptions::default()).expect("open the repo");

    measure("diff.commit típico (HEAD)", o.iters, 1_000.0, || {
        let found = reader.commit_diff(&head, &limits(), &|| false);
        assert!(matches!(found, Ok(Lookup::Found(_))), "HEAD must be found");
    });
    let ghost = "1".repeat(head.len());
    measure("diff.commit oid inexistente", o.iters, 2_000.0, || {
        let missing = reader.commit_diff(&ghost, &limits(), &|| false);
        assert!(
            matches!(missing, Ok(Lookup::Missing)),
            "a ghost must be Missing"
        );
    });

    // 20 tracked files changed in the working tree, then the index diff.
    let tracked = git(&repo, &["ls-files"]);
    for path in tracked.lines().take(CHANGED) {
        let file = repo.join(path);
        let mut bytes = std::fs::read(&file).unwrap_or_default();
        bytes.extend_from_slice(b"\nbench change\n");
        std::fs::write(&file, bytes).unwrap();
    }
    let reader = RepoReader::open(&repo, &ReaderOptions::default()).expect("open the repo");
    measure(
        &format!("diff.index con {CHANGED} archivos"),
        o.iters,
        500.0,
        || {
            reader.index_diff(&limits(), &|| false).expect("index_diff");
        },
    );
    git(&repo, &["checkout", "--", "."]);
}
