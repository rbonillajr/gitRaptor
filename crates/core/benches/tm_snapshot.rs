//! Bench of the guaranteed prior snapshot (TS-TMC-001, ADR-TMC-006 § 4, NFR-04).
//!
//! Generates the reference repo (profile `M`, D-TMC-21) with the deterministic generator of
//! SPIKE-TMC-001, seeds the store, and measures the prior with the deltas of ADR-TMC-006 § 2
//! (the reference delta is 100 files with ~20 MB of new content), with 1 worktree and with 10
//! active worktrees. **Fails** (exit code 1) if the p95 of the reference delta reaches 200 ms;
//! a stage above its budget only warns.
//!
//! ```sh
//! cargo bench -p gitraptor-core --bench tm_snapshot                 # profile M
//! cargo bench -p gitraptor-core --bench tm_snapshot -- --profile S --iters 20
//! TM_BENCH_ROOT=/scratch cargo bench -p gitraptor-core --bench tm_snapshot -- --profile L
//! ```
//!
//! Everything lives under a root outside any Git repo (NFR-01): `TM_BENCH_ROOT` or a temporary
//! folder. The generated repo is kept there and reused by later runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{Oplog, SnapshotLevel};
use gitraptor_core::timemachine::store::{
    CaptureError, CaptureRequest, ChangeHint, SnapshotStore, StageTimings, WorktreeScope,
};
use gitraptor_testkit::fingerprint::{Scope, Snapshot};
use gitraptor_testkit::repogen::{self, Rng};

const REPO_ID: &str = "be0c0000-0000-4000-8000-00000000be0c";
/// NFR-04 limit on the p95 of the overhead (ADR-TMC-006 § 2).
const LIMIT_MS: f64 = 200.0;

struct Opts {
    profile: String,
    iters: usize,
    worktrees: usize,
    root: Option<PathBuf>,
}

fn opts() -> Opts {
    let mut o = Opts {
        profile: "M".into(),
        iters: 100,
        worktrees: 10,
        root: std::env::var_os("TM_BENCH_ROOT").map(PathBuf::from),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let next = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--profile" => o.profile = next,
            "--iters" => o.iters = next.parse().expect("--iters N"),
            "--worktrees" => o.worktrees = next.parse().expect("--worktrees N"),
            "--root" => o.root = Some(PathBuf::from(next)),
            // `cargo bench` passes `--bench`; nothing else is expected.
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

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = ((p / 100.0) * s.len() as f64).ceil() as usize;
    s[rank.clamp(1, s.len()) - 1]
}

struct Report(String);

impl Report {
    fn line(&mut self, s: impl AsRef<str>) {
        println!("{}", s.as_ref());
        self.0.push_str(s.as_ref());
        self.0.push('\n');
    }
}

/// Files the bench edits as "the agent", with their original bytes.
struct Editor {
    rng: Rng,
    text: Vec<String>,
    assets: Vec<(String, u64)>,
    originals: HashMap<(PathBuf, String), Vec<u8>>,
}

impl Editor {
    fn new(repo: &Path) -> Self {
        let mut text = Vec::new();
        let mut assets = Vec::new();
        for p in git(repo, &["ls-files"]).lines() {
            if p.starts_with("assets/") {
                assets.push((p.to_owned(), repo.join(p).metadata().unwrap().len()));
            } else if p.starts_with("src/") {
                text.push(p.to_owned());
            }
        }
        Self {
            rng: Rng::new(7),
            text,
            assets,
            originals: HashMap::new(),
        }
    }

    fn pick_text(&self, n: usize, offset: usize) -> Vec<String> {
        (0..n)
            .map(|i| self.text[(offset + i * 7919) % self.text.len()].clone())
            .collect()
    }

    /// `n` files with about `bytes` of content: assets first, text to complete the count.
    fn pick_heavy(&self, n: usize, bytes: u64) -> Vec<String> {
        let mut out = Vec::new();
        let mut sum = 0;
        for (p, len) in &self.assets {
            if sum >= bytes || out.len() >= n {
                break;
            }
            out.push(p.clone());
            sum += len;
        }
        let rest = n - out.len();
        out.extend(self.pick_text(rest, 31));
        out
    }

    fn mutate(&mut self, wt: &Path, paths: &[String]) {
        for p in paths {
            let full = wt.join(p);
            let mut content = std::fs::read(&full).unwrap();
            self.originals
                .entry((wt.to_path_buf(), p.clone()))
                .or_insert_with(|| content.clone());
            if p.starts_with("assets/") {
                self.rng.fill(&mut content);
            } else {
                repogen::edit_text(&mut self.rng, &mut content);
            }
            std::fs::write(&full, &content).unwrap();
        }
    }

    fn restore(&mut self, wt: &Path) -> Vec<String> {
        let keys: Vec<_> = self
            .originals
            .keys()
            .filter(|(w, _)| w == wt)
            .cloned()
            .collect();
        let mut paths = Vec::new();
        for k in keys {
            let content = self.originals.remove(&k).unwrap();
            std::fs::write(wt.join(&k.1), content).unwrap();
            paths.push(k.1);
        }
        paths
    }
}

/// Captures of one worktree with continuous engine hints.
struct Wt {
    key: String,
    path: PathBuf,
    mark: i64,
}

impl Wt {
    fn request(&mut self, level: SnapshotLevel, repo: &Path, paths: Vec<String>) -> CaptureRequest {
        let since = self.mark;
        self.mark += 1;
        CaptureRequest {
            level,
            repo: repo.to_path_buf(),
            worktrees: vec![WorktreeScope {
                key: self.key.clone(),
                path: self.path.clone(),
                hint: Some(ChangeHint {
                    since,
                    mark: self.mark,
                    paths,
                    continuous: true,
                }),
            }],
            engine_mark: Some(self.mark),
            cause_operation: None,
            cause_event_seq: None,
        }
    }
}

/// One stage of [`StageTimings`].
type Stage = fn(&StageTimings) -> Duration;

#[derive(Default)]
struct Series(Vec<StageTimings>);

impl Series {
    fn col(&self, f: fn(&StageTimings) -> Duration) -> Vec<f64> {
        self.0.iter().map(|t| ms(f(t))).collect()
    }

    fn p95_total(&self) -> f64 {
        pct(&self.col(|t| t.total), 95.0)
    }

    fn row(&self, r: &mut Report, scenario: &str) {
        let p95 = |f| pct(&self.col(f), 95.0);
        let mb: Vec<f64> = self
            .0
            .iter()
            .map(|t| t.bytes_read as f64 / 1_048_576.0)
            .collect();
        r.line(format!(
            "| {scenario} | {} | **{:.1}** | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
            self.0.len(),
            p95(|t| t.total),
            pct(&self.col(|t| t.total), 50.0),
            pct(&self.col(|t| t.total), 100.0),
            p95(|t| t.queue),
            p95(|t| t.detect),
            p95(|t| t.anchor),
            p95(|t| t.blobs),
            p95(|t| t.trees),
            p95(|t| t.ref_oplog),
            pct(&mb, 50.0),
        ));
    }

    /// Stage budgets of ADR-TMC-006 § 2: a warning per stage over its budget.
    fn warnings(&self, scenario: &str) -> Vec<String> {
        let budgets: [(&str, Stage, f64); 5] = [
            ("detección", |t| t.detect, 5.0),
            ("anclaje", |t| t.anchor, 5.0),
            ("blobs", |t| t.blobs, 90.0),
            ("árboles + commit", |t| t.trees, 45.0),
            ("ref + oplog", |t| t.ref_oplog, 25.0),
        ];
        budgets
            .iter()
            .filter_map(|(name, f, budget)| {
                let v = pct(&self.col(*f), 95.0);
                (v > *budget).then(|| {
                    format!("⚠️ {scenario}: etapa {name} p95 {v:.1} ms > presupuesto {budget} ms")
                })
            })
            .collect()
    }
}

fn table_header(r: &mut Report) {
    r.line("| escenario | n | p95 total (ms) | p50 | máx | cola p95 | detección p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB leídos (p50) |");
    r.line("|---|---|---|---|---|---|---|---|---|---|---|---|");
}

/// Fingerprint of the user's `.git` (worktrees' Git folders included): must not change.
fn git_fingerprint(repo: &Path) -> Snapshot {
    Snapshot::take(
        &[Scope::system("git", repo.join(".git"))],
        &Default::default(),
    )
}

fn main() {
    let o = opts();
    let profile = repogen::profile(&o.profile).expect("profile S|P50|M|L");
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
    let desc = if marker.exists() {
        std::fs::read_to_string(&marker).unwrap()
    } else {
        let _ = std::fs::remove_dir_all(&repo);
        println!("generando el perfil {} en {} …", profile.name, repo.display());
        let g = repogen::generate(&git_bin(), &profile, &repo, 42).unwrap();
        let desc = format!(
            "{} HEAD={} archivos={} working tree={} MB commits={} ({:.0} s)",
            profile.name,
            g.head,
            g.tracked_files,
            g.wt_bytes >> 20,
            g.commits,
            g.secs
        );
        std::fs::write(&marker, &desc).unwrap();
        desc
    };
    let mut wts = vec![repo.clone()];
    for i in 1..o.worktrees {
        let p = root.join(format!("repo-{}-wt{i}", profile.name));
        if !p.exists() {
            git(&repo, &["worktree", "add", "-q", "-b", &format!("wt{i}"), p.to_str().unwrap()]);
        }
        wts.push(p);
    }

    let mut r = Report(String::new());
    r.line(format!("# Banco del snapshot previo (TS-TMC-001) — perfil {}\n", profile.name));
    r.line(format!("- Repo: {desc}"));
    r.line(format!(
        "- Máquina: {} {} · {} núcleos · Git {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        git(&root, &["--version"])
    ));
    r.line(format!("- Iteraciones por escenario: {} (+3 de calentamiento)\n", o.iters));

    let guard_before = git_fingerprint(&repo);

    // ---- profile, store and seeding --------------------------------------------------------
    let profile_root = root.join(format!(
        "profile-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let dirs = ProfileDirs::under_root(&profile_root);
    let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
    let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
    let oplog = Mutex::new(oplog);
    let t = Instant::now();
    let seed = store.seed(&repo).unwrap();
    r.line(format!(
        "- Siembra: {:.2} s · {} packs clonados, {} copiados, {} objetos, {} MiB, {} omitidos",
        t.elapsed().as_secs_f64(),
        seed.cloned,
        seed.copied,
        seed.objects,
        seed.bytes >> 20,
        seed.skipped.len()
    ));

    let mut w0 = Wt {
        key: "w0".into(),
        path: repo.clone(),
        mark: 0,
    };
    let t = Instant::now();
    let first = store
        .capture(&oplog, &w0.request(SnapshotLevel::Observation, &repo, Vec::new()))
        .unwrap();
    r.line(format!(
        "- Primera captura (detección completa, índice reflejado, caché de stat): {:.0} ms, {} archivos leídos\n",
        ms(t.elapsed()),
        first.timings.files_read
    ));

    // ---- 1 worktree -----------------------------------------------------------------------
    r.line("## Snapshot previo, 1 worktree, almacén sembrado\n");
    table_header(&mut r);
    let mut ed = Editor::new(&repo);
    let mut warnings = Vec::new();
    let mut gate: Vec<(String, f64)> = Vec::new();
    let scenarios: Vec<(&str, usize, Option<u64>)> = vec![
        ("sin cambios (camino rápido)", 0, None),
        ("delta 1 archivo", 1, None),
        ("delta 10 archivos", 10, None),
        ("delta 100 archivos", 100, None),
        ("delta de referencia: 100 archivos / ~20 MB", 100, Some(20 << 20)),
        ("delta 1.000 archivos (fuera de referencia)", 1000, None),
    ];
    for (name, n, heavy) in scenarios {
        let paths = match heavy {
            Some(b) => ed.pick_heavy(n, b),
            None => ed.pick_text(n, 0),
        };
        let iters = if n >= 1000 { (o.iters / 4).max(10) } else { o.iters };
        let mut series = Series::default();
        for i in 0..iters + 3 {
            ed.mutate(&repo, &paths);
            let req = w0.request(SnapshotLevel::GuaranteedPrior, &repo, paths.clone());
            let out = store.capture(&oplog, &req).unwrap();
            if i >= 3 {
                series.0.push(out.timings);
            }
        }
        series.row(&mut r, name);
        if heavy.is_some() {
            gate.push((format!("1 worktree, {name}"), series.p95_total()));
            warnings.extend(series.warnings(name));
        }
        let restored = ed.restore(&repo);
        store
            .capture(&oplog, &w0.request(SnapshotLevel::Observation, &repo, restored))
            .unwrap();
    }

    // ---- 10 worktrees: 9 capture by observation while the prior of w0 is measured ----------
    if wts.len() > 1 {
        r.line(format!(
            "\n## Snapshot previo de w0 con {} worktrees capturando por observación (una captura por segundo cada uno, 10 archivos)\n",
            wts.len() - 1
        ));
        table_header(&mut r);
        let store = Arc::new(store);
        let oplog = Arc::new(oplog);
        let stop = Arc::new(AtomicBool::new(false));
        let yielded = Arc::new(Mutex::new(0usize));
        let mut threads = Vec::new();
        for (i, path) in wts.iter().enumerate().skip(1) {
            let (store, oplog, stop, yielded, path, repo) = (
                Arc::clone(&store),
                Arc::clone(&oplog),
                Arc::clone(&stop),
                Arc::clone(&yielded),
                path.clone(),
                repo.clone(),
            );
            threads.push(std::thread::spawn(move || {
                let mut ed = Editor::new(&path);
                ed.rng = Rng::new(100 + i as u64);
                let mut wt = Wt {
                    key: format!("w{i}"),
                    path: path.clone(),
                    mark: 0,
                };
                let _ = store.capture(&oplog, &wt.request(SnapshotLevel::Observation, &repo, Vec::new()));
                let mut k = 0;
                while !stop.load(Ordering::Acquire) {
                    let paths = ed.pick_text(10, k * 13 % 2000);
                    k += 1;
                    ed.mutate(&path, &paths);
                    let req = wt.request(SnapshotLevel::Observation, &repo, paths);
                    match store.capture(&oplog, &req) {
                        Ok(_) => {}
                        Err(CaptureError::Yielded) => {
                            *yielded.lock().unwrap() += 1;
                            // The next capture starts over with a full detection.
                            wt.mark = -1;
                        }
                        Err(e) => panic!("observation w{i}: {e}"),
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                }
                ed.restore(&path)
            }));
        }
        std::thread::sleep(Duration::from_millis(1500));
        for (name, n, heavy) in [
            ("delta 100 archivos", 100usize, None),
            ("delta de referencia: 100 archivos / ~20 MB", 100, Some(20u64 << 20)),
        ] {
            let paths = match heavy {
                Some(b) => ed.pick_heavy(n, b),
                None => ed.pick_text(n, 0),
            };
            let mut series = Series::default();
            for i in 0..o.iters + 3 {
                ed.mutate(&repo, &paths);
                let req = w0.request(SnapshotLevel::GuaranteedPrior, &repo, paths.clone());
                let out = store.capture(&oplog, &req).unwrap();
                if i >= 3 {
                    series.0.push(out.timings);
                }
                std::thread::sleep(Duration::from_millis(97));
            }
            let label = format!("{name}, {} worktrees", wts.len());
            series.row(&mut r, &label);
            gate.push((label.clone(), series.p95_total()));
            warnings.extend(series.warnings(&label));
            let queue = pct(&series.col(|t| t.queue), 95.0);
            if queue > 10.0 {
                warnings.push(format!(
                    "⚠️ {label}: espera del escritor p95 {queue:.1} ms > 10 ms (ADR-TMC-004 § 2)"
                ));
            }
            let restored = ed.restore(&repo);
            store
                .capture(&oplog, &w0.request(SnapshotLevel::Observation, &repo, restored))
                .unwrap();
        }
        stop.store(true, Ordering::Release);
        for t in threads {
            t.join().unwrap();
        }
        r.line(format!(
            "\nCapturas por observación que cedieron al previo: {}",
            yielded.lock().unwrap()
        ));
        r.line(format!(
            "Tamaño del almacén al final: {} MiB",
            store.size_bytes().unwrap() >> 20
        ));
    }

    // ---- guard and gate --------------------------------------------------------------------
    let guard_after = git_fingerprint(&repo);
    let diffs = gitraptor_testkit::diff(&guard_before, &guard_after);
    r.line(format!(
        "\n## Repo intacto\n\nHuella de `.git` (y de los Git de sus worktrees) antes y después: {}",
        if diffs.is_empty() {
            "idéntica ✅".to_owned()
        } else {
            format!("**{} diferencias** ❌", diffs.len())
        }
    ));
    for d in diffs.iter().take(20) {
        r.line(format!("- {d}"));
    }

    r.line("\n## Gate NFR-04 (p95 < 200 ms)\n");
    let mut failed = !diffs.is_empty();
    for (label, p95) in &gate {
        let ok = *p95 < LIMIT_MS;
        failed |= !ok;
        r.line(format!(
            "- {label}: p95 {p95:.1} ms {}",
            if ok { "✅" } else { "❌" }
        ));
    }
    for w in &warnings {
        r.line(format!("- {w}"));
    }
    let report_path = root.join(format!("results-{}.md", profile.name));
    std::fs::write(&report_path, &r.0).unwrap();
    println!("\ninforme: {}", report_path.display());
    if failed {
        std::process::exit(1);
    }
}
