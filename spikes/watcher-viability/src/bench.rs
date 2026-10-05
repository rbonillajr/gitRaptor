//! Experiments of SPIKE-GRP-002. Each one returns a JSON value with its raw summary.

use crate::engine::{Engine, EngineCfg, Pub, WtDesc, degraded_steady, discover, poll_fingerprint};
use crate::repo::{Synthetic, file_path, git, run};
use crate::util::{
    blob_oid, cpu_ms, ms, now_ns, open_fds, pct, rss_mib, sig, summarize, thread_cpu_ms,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

pub struct Ctx {
    pub syn: Synthetic,
    pub profile: PathBuf,
    pub samples: usize,
    pub quick: bool,
    pub dirs: usize,
    pub files_per_dir: usize,
    pub commits: usize,
    /// Latest publication seen per worktree.
    pub last: HashMap<String, Pub>,
}

pub fn cfg(ctx: &Ctx, window_ms: u64, sliding: bool, poll: Option<u64>) -> EngineCfg {
    EngineCfg {
        window_ms,
        sliding,
        fullfsync: true,
        poll_backup_ms: poll,
        base_branch: "main".into(),
        profile_dir: ctx.profile.join(format!("p-{}", now_ns())),
    }
}

impl Ctx {
    pub fn start(&mut self, c: EngineCfg) -> Result<Engine> {
        let eng = Engine::start(&self.syn.main, c)?;
        let n = eng.worktree_names().len();
        let mut ready = 0;
        while ready < n {
            match eng.rx.recv_timeout(Duration::from_secs(30)) {
                Ok(p) => {
                    if p.kind == "added" {
                        ready += 1;
                    }
                    self.last.insert(p.wt.clone(), p);
                }
                Err(_) => anyhow::bail!("engine not ready"),
            }
        }
        // Let the FSEvents stream settle before measuring.
        std::thread::sleep(Duration::from_millis(500));
        self.drain(&eng);
        Ok(eng)
    }

    pub fn drain(&mut self, eng: &Engine) {
        while let Ok(p) = eng.rx.try_recv() {
            self.last.insert(p.wt.clone(), p);
        }
    }

    pub fn wait_for(
        &mut self,
        eng: &Engine,
        timeout_ms: u64,
        pred: impl Fn(&Pub) -> bool,
    ) -> Option<Pub> {
        let deadline = now_ns() + timeout_ms * 1_000_000;
        while now_ns() < deadline {
            if let Ok(p) = eng.rx.recv_timeout(Duration::from_millis(5)) {
                self.last.insert(p.wt.clone(), p.clone());
                if pred(&p) {
                    return Some(p);
                }
            }
        }
        None
    }

    pub fn root(&self, wt: &str) -> PathBuf {
        std::fs::canonicalize(self.syn.wt_root(wt)).unwrap()
    }

    pub fn gitdir(&self, wt: &str) -> PathBuf {
        let c = std::fs::canonicalize(self.syn.main.join(".git")).unwrap();
        if wt == "main" {
            c
        } else {
            c.join("worktrees").join(wt)
        }
    }

    pub fn index_sig(&self, wt: &str) -> String {
        let m = std::fs::metadata(self.gitdir(wt).join("index")).unwrap();
        let s = sig(&m);
        format!("{}:{}.{}", s.len, s.mtime_s, s.mtime_ns)
    }

    pub fn names(&self) -> Vec<String> {
        self.syn.all_names()
    }

    pub fn file(&self, i: usize) -> String {
        let total = self.dirs * self.files_per_dir;
        let idx = (i * 7919) % total;
        file_path(idx / self.files_per_dir, idx % self.files_per_dir)
    }

    /// Reset every worktree to a clean state (bench-side writes, untimed).
    pub fn clean_all(&mut self, eng: Option<&Engine>) -> Result<()> {
        for n in self.names() {
            let r = self.root(&n);
            let home = if n == "main" { "main" } else { n.as_str() };
            run(git(&r).args(["reset", "-q", "--hard"]))?;
            run(git(&r).args(["checkout", "-q", home]))?;
            run(git(&r).args(["clean", "-q", "-fdx"]))?;
        }
        if let Some(eng) = eng {
            for n in self.names() {
                let ok = |p: &Pub| p.wt == n && p.dirty == 0;
                if !self.last.get(&n).map(ok).unwrap_or(false) {
                    let _ = self.wait_for(eng, 5000, ok);
                }
            }
            std::thread::sleep(Duration::from_millis(200));
            self.drain(eng);
        }
        Ok(())
    }
}

#[derive(Default)]
struct Stages {
    total: Vec<f64>,
    detection: Vec<f64>,
    debounce: Vec<f64>,
    compute_persist: Vec<f64>,
    compute: Vec<f64>,
    persist: Vec<f64>,
    publish: Vec<f64>,
    negative_detection: usize,
    full_recompute: usize,
    timeouts: usize,
}

impl Stages {
    fn add(&mut self, t0: u64, p: &Pub) {
        let total = p.t_client_recv.saturating_sub(t0);
        self.total.push(ms(total));
        if p.t_recv < t0 {
            self.negative_detection += 1;
        }
        self.detection.push(ms(p.t_recv.saturating_sub(t0)));
        self.debounce.push(ms(p.t_flush - p.t_recv.min(p.t_flush)));
        self.compute.push(ms(p.t_computed - p.t_flush));
        self.persist.push(ms(p.t_persisted - p.t_computed));
        self.compute_persist.push(ms(p.t_persisted - p.t_flush));
        self.publish.push(ms(p.t_client_recv - p.t_persisted));
        if p.full_recompute {
            self.full_recompute += 1;
        }
    }
    fn json(&self) -> Value {
        json!({
            "total_t0_to_client": summarize(&self.total),
            "detection": summarize(&self.detection),
            "debounce": summarize(&self.debounce),
            "compute": summarize(&self.compute),
            "persist": summarize(&self.persist),
            "compute_plus_persist": summarize(&self.compute_persist),
            "publish": summarize(&self.publish),
            "events_before_t0": self.negative_detection,
            "full_recompute_batches": self.full_recompute,
            "timeouts": self.timeouts,
            "over_300ms": self.total.iter().filter(|v| **v > 300.0).count(),
        })
    }
}

fn pause(ctx: &mut Ctx, eng: &Engine) {
    std::thread::sleep(Duration::from_millis(120));
    ctx.drain(eng);
}

// ---------------------------------------------------------------- latency

pub fn latency(ctx: &mut Ctx) -> Result<Value> {
    let eng = ctx.start(cfg(ctx, 75, false, Some(30_000)))?;
    let names = ctx.names();
    let s = ctx.samples;
    let mut out = serde_json::Map::new();

    // 1. Modify a tracked file.
    let mut st = Stages::default();
    for i in 0..s {
        let wt = names[i % names.len()].clone();
        let f = ctx.file(i);
        let body = format!("sample {i} {}\n", now_ns());
        let expect = blob_oid(body.as_bytes()).to_string();
        std::fs::write(ctx.root(&wt).join(&f), &body)?;
        let t0 = now_ns();
        match ctx.wait_for(&eng, 5000, |p| {
            p.wt == wt
                && p.changed
                    .iter()
                    .any(|(path, _, o)| *path == f && *o == expect)
        }) {
            Some(p) => st.add(t0, &p),
            None => st.timeouts += 1,
        }
        pause(ctx, &eng);
    }
    out.insert("modify".into(), st.json());
    let modify_detection = st.detection.clone();

    // 2. git add.
    let mut st = Stages::default();
    for i in 0..s {
        let wt = names[i % names.len()].clone();
        let f = ctx.file(10_000 + i);
        let body = format!("add {i} {}\n", now_ns());
        let expect = blob_oid(body.as_bytes()).to_string();
        std::fs::write(ctx.root(&wt).join(&f), &body)?;
        ctx.wait_for(&eng, 5000, |p| {
            p.wt == wt
                && p.changed
                    .iter()
                    .any(|(path, _, o)| *path == f && *o == expect)
        });
        pause(ctx, &eng);
        run(git(&ctx.root(&wt)).args(["add", &f]))?;
        let t0 = now_ns();
        let sig = ctx.index_sig(&wt);
        match ctx.wait_for(&eng, 5000, |p| p.wt == wt && p.index_sig == sig) {
            Some(p) => st.add(t0, &p),
            None => st.timeouts += 1,
        }
        pause(ctx, &eng);
    }
    out.insert("git_add".into(), st.json());

    // 3. Commit (one staged file each time).
    let mut st = Stages::default();
    for i in 0..s {
        let wt = names[i % names.len()].clone();
        let f = ctx.file(20_000 + i);
        std::fs::write(ctx.root(&wt).join(&f), format!("commit {i} {}\n", now_ns()))?;
        run(git(&ctx.root(&wt)).args(["add", &f]))?;
        let sig = ctx.index_sig(&wt);
        ctx.wait_for(&eng, 5000, |p| p.wt == wt && p.index_sig == sig);
        pause(ctx, &eng);
        run(git(&ctx.root(&wt)).args([
            "commit",
            "-q",
            "--no-verify",
            "-m",
            &format!("bench {i}"),
        ]))?;
        let t0 = now_ns();
        let head = run(git(&ctx.root(&wt)).args(["rev-parse", "HEAD"]))?;
        match ctx.wait_for(&eng, 5000, |p| p.wt == wt && p.head_oid == head) {
            Some(p) => st.add(t0, &p),
            None => st.timeouts += 1,
        }
        pause(ctx, &eng);
    }
    out.insert("commit".into(), st.json());

    // 4. Checkout between the worktree branch and a branch 50 commits behind main.
    ctx.clean_all(Some(&eng))?;
    let mut st = Stages::default();
    let mut on_alt: HashMap<String, bool> = HashMap::new();
    for i in 0..s {
        let wt = names[i % names.len()].clone();
        let alt = if wt == "main" {
            "alt-main".to_string()
        } else {
            format!("alt-{}", &wt[3..])
        };
        let home = if wt == "main" {
            "main".to_string()
        } else {
            wt.clone()
        };
        let to_alt = !on_alt.get(&wt).copied().unwrap_or(false);
        let target = if to_alt { alt } else { home };
        run(git(&ctx.root(&wt)).args(["checkout", "-q", &target]))?;
        let t0 = now_ns();
        on_alt.insert(wt.clone(), to_alt);
        let head = run(git(&ctx.root(&wt)).args(["rev-parse", "HEAD"]))?;
        let sig = ctx.index_sig(&wt);
        let r = format!("refs/heads/{target}");
        match ctx.wait_for(&eng, 5000, |p| {
            p.wt == wt && p.head_oid == head && p.head_ref == r && p.index_sig == sig
        }) {
            Some(p) => st.add(t0, &p),
            None => st.timeouts += 1,
        }
        pause(ctx, &eng);
    }
    for wt in &names {
        let home = if wt == "main" { "main" } else { wt.as_str() };
        run(git(&ctx.root(wt)).args(["checkout", "-q", home]))?;
    }
    out.insert("checkout".into(), st.json());
    pause(ctx, &eng);

    // 5/6. Worktree add and remove.
    let n_wt = (s / 4).max(20);
    let mut add = Stages::default();
    let mut rem = Stages::default();
    for i in 0..n_wt {
        let name = format!("tmp-{i:03}");
        let path = ctx.syn.base.join(&name);
        run(git(&ctx.syn.main).args([
            "worktree",
            "add",
            "-q",
            "-b",
            &name,
            path.to_str().unwrap(),
            "main",
        ]))?;
        let t0 = now_ns();
        let head = run(git(&path).args(["rev-parse", "HEAD"]))?;
        let sig = ctx.index_sig(&name);
        match ctx.wait_for(&eng, 15000, |p| {
            p.wt == name && p.head_oid == head && p.index_sig == sig && p.dirty == 0
        }) {
            Some(p) => add.add(t0, &p),
            None => add.timeouts += 1,
        }
        pause(ctx, &eng);
        run(git(&ctx.syn.main).args(["worktree", "remove", "--force", path.to_str().unwrap()]))?;
        let t0 = now_ns();
        match ctx.wait_for(&eng, 15000, |p| p.wt == name && p.kind == "removed") {
            Some(p) => rem.add(t0, &p),
            None => rem.timeouts += 1,
        }
        run(git(&ctx.syn.main).args(["branch", "-q", "-D", &name]))?;
        pause(ctx, &eng);
    }
    out.insert("worktree_add".into(), add.json());
    out.insert("worktree_remove".into(), rem.json());

    // FSEvents raw detection floor (modify scenario only, where t0 is exact).
    out.insert(
        "detection_floor_ms".into(),
        json!({"min": modify_detection.iter().cloned().fold(f64::MAX, f64::min), "p50": pct(&modify_detection, 50.0)}),
    );
    out.insert("counters".into(), serde_json::to_value(eng.counters())?);
    eng.stop();
    ctx.clean_all(None)?;
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------- debounce

/// Writes `n` new files; paced at one file per `every_us` (in steps of 10 files, sleeping, not spinning).
/// Returns (first write, last write, writer thread CPU ms).
fn write_burst(dir: &Path, n: usize, every_us: u64) -> (u64, u64, f64) {
    let cpu0 = thread_cpu_ms();
    let mut first = 0;
    let mut last = 0;
    for i in 0..n {
        let d = dir.join(format!("d{:03}", i / 100));
        if i % 100 == 0 {
            let _ = std::fs::create_dir_all(&d);
        }
        let _ = std::fs::write(d.join(format!("f{i:05}.txt")), format!("burst {i}\n"));
        last = now_ns();
        if i == 0 {
            first = last;
        }
        if every_us > 0 && i % 10 == 9 {
            let target = first + (i as u64 + 1) * every_us * 1000;
            let now = now_ns();
            if target > now {
                std::thread::sleep(Duration::from_nanos(target - now));
            }
        }
    }
    (first, last, thread_cpu_ms() - cpu0)
}

pub fn debounce(ctx: &mut Ctx) -> Result<Value> {
    let n = if ctx.quick { 1000 } else { 2000 };
    let configs: Vec<(u64, bool)> = vec![
        (0, false),
        (25, false),
        (50, false),
        (75, false),
        (100, false),
        (150, false),
        (75, true),
    ];
    let wt = "wt-01".to_string();
    let mut rows = Vec::new();
    for (w, sliding) in configs {
        let eng = ctx.start(cfg(ctx, w, sliding, None))?;
        let dir = ctx.root(&wt).join("burst");
        let cpu0 = cpu_ms();
        let wall0 = now_ns();
        let h = {
            let dir = dir.clone();
            std::thread::spawn(move || write_burst(&dir, n, 1000))
        };
        let mut pubs: Vec<Pub> = Vec::new();
        let deadline = now_ns() + 30_000_000_000;
        while now_ns() < deadline {
            if let Ok(p) = eng.rx.recv_timeout(Duration::from_millis(5))
                && p.wt == wt
            {
                let done = p.dirty >= n;
                pubs.push(p);
                if done {
                    break;
                }
            }
        }
        let (first, last, writer_cpu) = h.join().unwrap();
        let cpu = cpu_ms() - cpu0 - writer_cpu;
        let wall = ms(now_ns() - wall0);
        let first_vis = pubs
            .iter()
            .find(|p| p.dirty > 0)
            .map(|p| ms(p.t_client_recv.saturating_sub(first)));
        let fin = pubs
            .iter()
            .find(|p| p.dirty >= n)
            .map(|p| ms(p.t_client_recv.saturating_sub(last)));
        // Staleness during the burst: longest interval without a publication while writes keep arriving.
        let mut gaps = Vec::new();
        let mut prev = first;
        for p in pubs.iter().filter(|p| p.t_client_recv <= last + 1) {
            gaps.push(ms(p.t_client_recv.saturating_sub(prev)));
            prev = p.t_client_recv;
        }
        gaps.push(ms(last.saturating_sub(prev)));
        rows.push(json!({
            "window_ms": w, "mode": if sliding {"sliding"} else {"fixed"},
            "files": n, "burst_ms": ms(last - first),
            "first_visible_ms": first_vis, "final_after_last_write_ms": fin,
            "recomputes": pubs.len(), "max_staleness_during_burst_ms": gaps.iter().cloned().fold(0.0, f64::max),
            "engine_cpu_ms": cpu, "writer_cpu_ms": writer_cpu, "wall_ms": wall,
            "compute_p95_ms": pct(&pubs.iter().map(|p| ms(p.t_computed - p.t_flush)).collect::<Vec<_>>(), 95.0),
            "persist_p95_ms": pct(&pubs.iter().map(|p| ms(p.t_persisted - p.t_computed)).collect::<Vec<_>>(), 95.0),
        }));
        eng.stop();
        let _ = std::fs::remove_dir_all(&dir);
        ctx.last.clear();
    }
    Ok(json!({"rows": rows}))
}

// ---------------------------------------------------------------- scale

pub fn scale(ctx: &mut Ctx) -> Result<Value> {
    let fds_before = open_fds();
    let rss_before = rss_mib();
    let eng = ctx.start(cfg(ctx, 75, false, Some(30_000)))?;
    let fds_engine = open_fds();
    let rss_engine = rss_mib();
    // Idle CPU: no writes, backup poll active.
    let idle_secs = if ctx.quick { 5 } else { 31 };
    let c0 = cpu_ms();
    std::thread::sleep(Duration::from_secs(idle_secs));
    let idle_cpu = cpu_ms() - c0;
    let polls = eng.counters().poll_cycles;
    ctx.drain(&eng);

    let n = 10_000;
    let burst_wt = "wt-01".to_string();
    let dir = ctx.root(&burst_wt).join("scale");
    let others: Vec<String> = ctx.names().into_iter().filter(|w| *w != burst_wt).collect();
    let c_before = eng.counters();
    let cpu0 = cpu_ms();
    let h = {
        let dir = dir.clone();
        std::thread::spawn(move || write_burst(&dir, n, 0))
    };
    let mut probe_lat = Vec::new();
    let mut probe_timeouts = 0;
    let mut burst_pubs = 0;
    let mut first_vis = None;
    let mut fin: Option<u64> = None;
    let mut rss_peak = rss_engine;
    let mut fds_peak = fds_engine;
    let mut i = 0usize;
    let mut probe: Option<(String, String, String, u64)> = None;
    let deadline = now_ns() + 120_000_000_000;
    let mut last_sample = 0;
    let t_start = now_ns();
    while now_ns() < deadline && (fin.is_none() || !h.is_finished()) {
        if probe.is_none() && !h.is_finished() {
            let wt = others[i % others.len()].clone();
            let f = ctx.file(30_000 + i);
            let body = format!("probe {i} {}\n", now_ns());
            std::fs::write(ctx.root(&wt).join(&f), &body)?;
            probe = Some((wt, f, blob_oid(body.as_bytes()).to_string(), now_ns()));
            i += 1;
        }
        if now_ns() - last_sample > 200_000_000 {
            rss_peak = rss_peak.max(rss_mib());
            fds_peak = fds_peak.max(open_fds());
            last_sample = now_ns();
        }
        if let Ok(p) = eng.rx.recv_timeout(Duration::from_millis(5)) {
            if p.wt == burst_wt {
                burst_pubs += 1;
                if first_vis.is_none() && p.dirty > 0 {
                    first_vis = Some(ms(p.t_client_recv - t_start));
                }
                if p.dirty >= n {
                    fin = Some(p.t_client_recv);
                }
            }
            if let Some((wt, f, o, t0)) = &probe
                && p.wt == *wt
                && p.changed.iter().any(|(path, _, oid)| path == f && oid == o)
            {
                probe_lat.push(ms(p.t_client_recv - t0));
                probe = None;
            }
        }
        if let Some((_, _, _, t0)) = &probe
            && now_ns() - t0 > 3_000_000_000
        {
            probe_timeouts += 1;
            probe = None;
        }
    }
    let (_, last_write, writer_cpu) = h.join().unwrap();
    let burst_cpu = cpu_ms() - cpu0 - writer_cpu;
    let c_after = eng.counters();
    let burst = json!({
        "files": n,
        "write_ms": ms(last_write - t_start),
        "first_visible_ms_from_start": first_vis,
        "final_after_last_write_ms": fin.map(|f| ms(f.saturating_sub(last_write))),
        "burst_worktree_publications": burst_pubs,
        "other_worktrees_probe_latency": summarize(&probe_lat),
        "probe_timeouts": probe_timeouts,
        "notify_events": c_after.notify_events - c_before.notify_events,
        "rescans": c_after.rescans - c_before.rescans,
        "engine_and_bench_cpu_ms": burst_cpu, "writer_cpu_ms": writer_cpu,
        "rss_peak_mib": rss_peak, "fds_peak": fds_peak,
    });
    let _ = std::fs::remove_dir_all(&dir);
    let wt = burst_wt.clone();
    ctx.wait_for(&eng, 20_000, |p| p.wt == wt && p.dirty == 0);
    pause(ctx, &eng);

    // Burst inside an ignored directory: must be filtered before the debounce.
    let ign = ctx.root(&burst_wt).join("target").join("scale");
    let c_before = eng.counters();
    let cpu0 = cpu_ms();
    let (_, _, writer_cpu) = write_burst(&ign, n, 0);
    std::thread::sleep(Duration::from_secs(2));
    let mut pubs_ignored = 0;
    while let Ok(p) = eng.rx.try_recv() {
        if p.wt == burst_wt {
            pubs_ignored += 1;
        }
    }
    let c_after = eng.counters();
    let ignored = json!({
        "files": n,
        "notify_events": c_after.notify_events - c_before.notify_events,
        "filtered_paths": c_after.filtered - c_before.filtered,
        "publications": pubs_ignored,
        "engine_cpu_ms": cpu_ms() - cpu0 - writer_cpu,
        "rescans": c_after.rescans - c_before.rescans,
    });
    let _ = std::fs::remove_dir_all(ctx.root(&burst_wt).join("target"));
    eng.stop();
    ctx.clean_all(None)?;
    Ok(json!({
        "worktrees": ctx.names().len(),
        "fds_before_engine": fds_before, "fds_with_engine": fds_engine,
        "rss_before_mib": rss_before, "rss_with_engine_mib": rss_engine,
        "idle_secs": idle_secs, "idle_cpu_ms": idle_cpu, "idle_cpu_pct": idle_cpu / (idle_secs as f64 * 10.0),
        "idle_poll_cycles": polls,
        "burst_10k": burst,
        "ignored_burst_10k": ignored,
    }))
}

// ---------------------------------------------------------------- polling

pub fn polling(ctx: &mut Ctx) -> Result<Value> {
    let descs: Vec<WtDesc> = discover(&ctx.syn.main);
    let common = std::fs::canonicalize(ctx.syn.main.join(".git"))?;
    let iters = if ctx.quick { 50 } else { 200 };
    let mut poll = Vec::new();
    for _ in 0..iters {
        let t = now_ns();
        for d in &descs {
            std::hint::black_box(poll_fingerprint(&common, &d.gitdir));
        }
        poll.push(ms(now_ns() - t));
    }
    let deg = degraded_steady(&descs[1], "main", if ctx.quick { 5 } else { 20 })?;
    let poll_s = summarize(&poll);
    let deg_s = summarize(&deg);
    Ok(json!({
        "worktrees": descs.len(),
        "files_per_worktree": ctx.dirs * ctx.files_per_dir,
        "backup_poll_cycle_all_worktrees_ms": poll_s,
        "backup_poll_cpu_pct_at_30s": poll_s.p50 / 30_000.0 * 100.0,
        "degraded_cycle_one_worktree_ms": deg_s,
        "degraded_cpu_pct_10_worktrees_at_2s": deg_s.p50 * descs.len() as f64 / 2_000.0 * 100.0,
    }))
}

// ---------------------------------------------------------------- gaps

/// Ground truth from Git itself (read-only: `GIT_OPTIONAL_LOCKS=0`).
fn truth(root: &Path) -> Result<(String, usize, usize)> {
    let head = run(git(root).args(["rev-parse", "HEAD"]))?;
    let out = std::process::Command::new("git")
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_GLOBAL", crate::repo::NULL_DEVICE)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()?;
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let mut dirty = 0;
    let mut staged = 0;
    for e in s.split('\0').filter(|e| e.len() > 3) {
        let (x, y) = (e.as_bytes()[0], e.as_bytes()[1]);
        if e.starts_with("??") {
            dirty += 1;
        } else {
            if y != b' ' {
                dirty += 1;
            }
            if x != b' ' {
                staged += 1;
            }
        }
    }
    Ok((head, dirty, staged))
}

/// Apply a fixed set of changes across worktrees; returns the worktrees touched.
fn change_set(ctx: &Ctx, tag: &str) -> Result<Vec<String>> {
    let w = |n: &str| ctx.root(n);
    std::fs::write(w("wt-02").join(ctx.file(40_001)), format!("gap {tag}\n"))?;
    std::fs::write(w("wt-03").join(format!("new-{tag}.txt")), "untracked\n")?;
    let f = ctx.file(40_004);
    std::fs::write(w("wt-04").join(&f), format!("gap commit {tag}\n"))?;
    run(git(&w("wt-04")).args(["add", &f]))?;
    run(git(&w("wt-04")).args(["commit", "-q", "--no-verify", "-m", tag]))?;
    std::fs::remove_file(w("wt-05").join(ctx.file(40_005)))?;
    run(git(&w("wt-06")).args(["checkout", "-q", "alt-06"]))?;
    std::fs::create_dir_all(w("main").join(format!("newdir-{tag}")))?;
    std::fs::write(w("main").join(format!("newdir-{tag}/a.txt")), "a\n")?;
    let f7 = ctx.file(40_007);
    std::fs::write(w("wt-07").join(&f7), format!("staged {tag}\n"))?;
    run(git(&w("wt-07")).args(["add", &f7]))?;
    Ok(
        ["wt-02", "wt-03", "wt-04", "wt-05", "wt-06", "main", "wt-07"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
}

fn matches_truth(ctx: &Ctx, touched: &[String]) -> Result<(usize, Vec<String>)> {
    let mut ok = 0;
    let mut bad = Vec::new();
    for wt in touched {
        let (head, dirty, staged) = truth(&ctx.root(wt))?;
        match ctx.last.get(wt) {
            Some(p) if p.head_oid == head && p.dirty == dirty && p.staged == staged => ok += 1,
            Some(p) => bad.push(format!("{wt}: engine head={} dirty={} staged={} / git head={head} dirty={dirty} staged={staged}", p.head_oid.get(..8).unwrap_or(""), p.dirty, p.staged)),
            None => bad.push(format!("{wt}: no state")),
        }
    }
    Ok((ok, bad))
}

fn settle(ctx: &mut Ctx, eng: &Engine, ms_: u64) {
    let deadline = now_ns() + ms_ * 1_000_000;
    while now_ns() < deadline {
        if let Ok(p) = eng.rx.recv_timeout(Duration::from_millis(10)) {
            ctx.last.insert(p.wt.clone(), p);
        }
    }
}

pub fn gaps(ctx: &mut Ctx) -> Result<Value> {
    let mut out = serde_json::Map::new();
    let eng = ctx.start(cfg(ctx, 75, false, None))?;

    // A. Dropped events (stands in for queue overflow / suspension where the OS loses events).
    ctx.clean_all(Some(&eng))?;
    eng.shared.drop_events.store(true, Relaxed);
    let touched = change_set(ctx, "drop")?;
    std::thread::sleep(Duration::from_millis(300));
    eng.shared.drop_events.store(false, Relaxed);
    settle(ctx, &eng, 1500);
    let (ok_none, _) = matches_truth(ctx, &touched)?;
    eng.poll_once();
    settle(ctx, &eng, 2000);
    let (ok_poll, bad_poll) = matches_truth(ctx, &touched)?;
    eng.reconcile_all();
    settle(ctx, &eng, 3000);
    let (ok_rec, bad_rec) = matches_truth(ctx, &touched)?;
    out.insert(
        "dropped_events".into(),
        json!({"changed_worktrees": touched.len(), "correct_without_recovery": ok_none,
               "correct_after_backup_poll": ok_poll, "missed_by_backup_poll": bad_poll,
               "correct_after_reconciliation": ok_rec, "mismatches_after_reconciliation": bad_rec}),
    );

    // B. Watcher killed and restarted.
    ctx.clean_all(Some(&eng))?;
    eng.kill_watcher();
    std::thread::sleep(Duration::from_millis(200));
    let touched = change_set(ctx, "restart")?;
    eng.restart_watcher()?;
    settle(ctx, &eng, 1500);
    let (ok_no_rec, _) = matches_truth(ctx, &touched)?;
    eng.reconcile_all();
    settle(ctx, &eng, 3000);
    let (ok_rec, bad_rec) = matches_truth(ctx, &touched)?;
    out.insert(
        "watcher_restart".into(),
        json!({"changed_worktrees": touched.len(), "correct_without_reconciliation": ok_no_rec,
               "correct_after_reconciliation": ok_rec, "mismatches": bad_rec}),
    );

    // C. FSEvents stream recreation (every watch/unwatch in notify restarts the stream "since now").
    ctx.clean_all(Some(&eng))?;
    let extra = ctx.profile.join("extra-watch");
    std::fs::create_dir_all(&extra)?;
    let mut runs = Vec::new();
    let reps = if ctx.quick { 2 } else { 5 };
    for (churn, label) in [(false, "control"), (true, "watch_churn")] {
        for r in 0..reps {
            let dir = ctx.root("wt-01").join(format!("probe-{label}-{r}"));
            std::fs::create_dir_all(&dir)?;
            std::thread::sleep(Duration::from_millis(200));
            *eng.shared.probe.lock().unwrap() = Some((dir.clone(), Default::default(), 0));
            let h = {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let mut files = Vec::new();
                    let t_end = now_ns() + 3_000_000_000;
                    let mut i = 0;
                    while now_ns() < t_end {
                        let p = dir.join(format!("f{i:05}"));
                        let _ = std::fs::write(&p, b"x");
                        files.push(p);
                        i += 1;
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    files
                })
            };
            let mut restarts = 0;
            if churn {
                for _ in 0..4 {
                    std::thread::sleep(Duration::from_millis(600));
                    eng.churn_watch(&extra)?;
                    restarts += 2;
                }
            }
            let files = h.join().unwrap();
            std::thread::sleep(Duration::from_millis(1000));
            let (_, seen, _) = eng.shared.probe.lock().unwrap().take().unwrap();
            let lost = files.iter().filter(|f| !seen.contains(*f)).count();
            runs.push(json!({"mode": label, "files": files.len(), "stream_restarts": restarts, "files_without_event": lost}));
            ctx.drain(&eng);
        }
    }
    out.insert("fsevents_stream_restart".into(), Value::Array(runs));
    eng.stop();
    ctx.clean_all(None)?;
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------- fsevents coalescing

pub fn coalescing(ctx: &mut Ctx) -> Result<Value> {
    let eng = ctx.start(cfg(ctx, 75, false, None))?;
    let mut rows = Vec::new();
    for (label, sleep_us) in [
        ("tight_loop", 0u64),
        ("every_1ms", 1000),
        ("every_10ms", 10_000),
    ] {
        let f = ctx.root("wt-02").join(ctx.file(50_000 + rows.len()));
        *eng.shared.probe.lock().unwrap() = Some((f.clone(), Default::default(), 0));
        let writes = 200;
        for i in 0..writes {
            std::fs::write(&f, format!("w{i}\n"))?;
            if sleep_us > 0 {
                std::thread::sleep(Duration::from_micros(sleep_us));
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let (_, _, count) = eng.shared.probe.lock().unwrap().take().unwrap();
        let mut pubs = 0;
        while let Ok(p) = eng.rx.try_recv() {
            if p.wt == "wt-02" {
                pubs += 1;
            }
        }
        rows.push(json!({"pattern": label, "writes": writes, "notify_events_for_file": count, "publications": pubs}));
    }
    eng.stop();
    ctx.clean_all(None)?;
    Ok(Value::Array(rows))
}

// ---------------------------------------------------------------- persistence

pub fn persistence(ctx: &mut Ctx) -> Result<Value> {
    let iters = if ctx.quick { 30 } else { 100 };
    let mut rows = Vec::new();
    for (label, pragmas) in [
        (
            "wal_full_fullfsync_on",
            "PRAGMA synchronous=FULL; PRAGMA fullfsync=ON;",
        ),
        (
            "wal_full_fullfsync_off",
            "PRAGMA synchronous=FULL; PRAGMA fullfsync=OFF;",
        ),
        ("wal_normal", "PRAGMA synchronous=NORMAL;"),
    ] {
        let dir = ctx.profile.join(format!("persist-{label}"));
        std::fs::create_dir_all(&dir)?;
        let mut db = rusqlite::Connection::open(dir.join("p.sqlite"))?;
        db.execute_batch(&format!(
            "PRAGMA journal_mode=WAL; {pragmas}
             CREATE TABLE events(id INTEGER PRIMARY KEY, wt TEXT, batch INTEGER, path TEXT, status TEXT, t INTEGER);"
        ))?;
        for batch in [1usize, 10, 100, 1000] {
            let mut v = Vec::new();
            for it in 0..iters {
                let t = now_ns();
                let tx = db.transaction()?;
                {
                    let mut st = tx.prepare_cached("INSERT INTO events(wt, batch, path, status, t) VALUES (?1, ?2, ?3, ?4, ?5)")?;
                    for k in 0..batch {
                        st.execute(rusqlite::params![
                            "wt-01",
                            it as i64,
                            format!("src/d000/f{k:04}.txt"),
                            "M",
                            t as i64
                        ])?;
                    }
                }
                tx.commit()?;
                v.push(ms(now_ns() - t));
            }
            rows.push(json!({"mode": label, "rows_per_tx": batch, "ms": summarize(&v)}));
        }
    }
    Ok(Value::Array(rows))
}

// ---------------------------------------------------------------- ahead/behind at 100K commits

pub fn ahead_behind(ctx: &mut Ctx) -> Result<Value> {
    let main = ctx.syn.main.clone();
    let far = (ctx.commits / 2).max(1);
    run(git(&main).args(["branch", "-f", "bench-far", &format!("main~{far}")]))?;
    let mut rows = Vec::new();
    for (label, cg) in [("commit_graph", "true"), ("no_commit_graph", "false")] {
        for (case, range) in [
            ("near_tip", "main...wt-01".to_string()),
            ("forked_far", "main...bench-far".to_string()),
        ] {
            let mut v = Vec::new();
            for _ in 0..10 {
                let t = now_ns();
                let out = std::process::Command::new("git")
                    .current_dir(&main)
                    .env("GIT_OPTIONAL_LOCKS", "0")
                    .env("GIT_CONFIG_GLOBAL", crate::repo::NULL_DEVICE)
                    .args([
                        "-c",
                        &format!("core.commitGraph={cg}"),
                        "rev-list",
                        "--left-right",
                        "--count",
                        &range,
                    ])
                    .output()?;
                std::hint::black_box(out);
                v.push(ms(now_ns() - t));
            }
            rows.push(json!({"mode": label, "case": case, "ms": summarize(&v)}));
        }
    }
    run(git(&main).args(["branch", "-q", "-D", "bench-far"]))?;
    Ok(json!({"commits": ctx.commits, "far_distance": far, "rows": rows}))
}

// ---------------------------------------------------------------- repo intact

pub fn intact(ctx: &mut Ctx) -> Result<Value> {
    let before = crate::repo::fingerprint(&ctx.syn.base);
    let eng = ctx.start(cfg(ctx, 75, false, Some(500)))?;
    eng.reconcile_all();
    std::thread::sleep(Duration::from_millis(1500));
    eng.poll_once();
    eng.reconcile_all();
    std::thread::sleep(Duration::from_millis(1500));
    let polls = eng.counters().poll_cycles;
    eng.stop();
    let after = crate::repo::fingerprint(&ctx.syn.base);
    let mut diffs = Vec::new();
    for (k, v) in &before {
        match after.get(k) {
            Some(v2) if v2 == v => {}
            Some(_) => diffs.push(format!("changed: {k}")),
            None => diffs.push(format!("removed: {k}")),
        }
    }
    for k in after.keys() {
        if !before.contains_key(k) {
            diffs.push(format!("added: {k}"));
        }
    }
    Ok(json!({"entries": before.len(), "poll_cycles": polls, "differences": diffs}))
}

// ---------------------------------------------------------------- timer slack

/// How late a 75 ms `recv_timeout` (the debounce primitive) actually wakes up.
pub fn timer(ctx: &mut Ctx) -> Result<Value> {
    let (_tx, rx) = std::sync::mpsc::channel::<()>();
    let mut rows = Vec::new();
    for w in [25u64, 75] {
        let mut v = Vec::new();
        for _ in 0..if ctx.quick { 20 } else { 100 } {
            let t = now_ns();
            let _ = rx.recv_timeout(Duration::from_millis(w));
            v.push(ms(now_ns() - t) - w as f64);
        }
        rows.push(json!({"timeout_ms": w, "overshoot_ms": summarize(&v)}));
    }
    Ok(Value::Array(rows))
}

// ---------------------------------------------------------------- handles (Windows-oriented, portable)

/// With the watcher active: `git worktree remove`, deleting and renaming a worktree root,
/// and deleting/renaming files inside it. Counts operations that fail (on Windows, an open
/// directory handle without FILE_SHARE_DELETE would make them fail).
pub fn handles(ctx: &mut Ctx) -> Result<Value> {
    let eng = ctx.start(cfg(ctx, 75, false, None))?;
    let reps = if ctx.quick { 3 } else { 10 };
    let mut fail = serde_json::Map::new();
    let mut count = |k: &str, ok: bool| {
        let e = fail.entry(k.to_string()).or_insert(json!([0, 0]));
        let a = e.as_array_mut().unwrap();
        a[0] = json!(a[0].as_u64().unwrap() + 1);
        if !ok {
            a[1] = json!(a[1].as_u64().unwrap() + 1);
        }
    };
    for i in 0..reps {
        for (op, name) in [
            ("worktree_remove", format!("h-rm-{i}")),
            ("root_delete", format!("h-del-{i}")),
            ("root_rename", format!("h-mv-{i}")),
        ] {
            let path = ctx.syn.base.join(&name);
            run(git(&ctx.syn.main).args([
                "worktree",
                "add",
                "-q",
                "-b",
                &name,
                path.to_str().unwrap(),
                "main",
            ]))?;
            let n = name.clone();
            ctx.wait_for(&eng, 15000, |p| p.wt == n && p.kind == "added");
            pause(ctx, &eng);
            // Files inside the watched root first.
            let f = path.join(ctx.file(60_000 + i));
            count(
                "file_rename",
                std::fs::rename(&f, f.with_extension("moved")).is_ok(),
            );
            count(
                "file_delete",
                std::fs::remove_file(f.with_extension("moved")).is_ok(),
            );
            let ok = match op {
                "worktree_remove" => run(git(&ctx.syn.main).args([
                    "worktree",
                    "remove",
                    "--force",
                    path.to_str().unwrap(),
                ]))
                .is_ok(),
                "root_delete" => std::fs::remove_dir_all(&path).is_ok(),
                _ => {
                    let to = path.with_extension("renamed");
                    let ok = std::fs::rename(&path, &to).is_ok();
                    let _ = std::fs::remove_dir_all(&to);
                    ok
                }
            };
            count(op, ok);
            let n = name.clone();
            let removed = ctx
                .wait_for(&eng, 15000, |p| p.wt == n && p.kind == "removed")
                .is_some();
            count(&format!("{op}_reported_removed"), removed);
            let _ = run(git(&ctx.syn.main).args(["worktree", "prune"]));
            let _ = run(git(&ctx.syn.main).args(["branch", "-q", "-D", &name]));
            pause(ctx, &eng);
        }
    }
    eng.stop();
    let out: serde_json::Map<String, Value> = fail
        .into_iter()
        .map(|(k, v)| (k, json!({"attempts": v[0], "failures": v[1]})))
        .collect();
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------- stream isolation (US-GRP-002)

/// ADR-GRP-010 § 1 candidate: one `notify` watcher per worktree on macOS. A writer creates a
/// file per millisecond in `wt-01` while another set of watches is added and removed. With a
/// shared watcher (`shared`) every add or remove recreates the stream that also covers
/// `wt-01`; with one watcher per worktree (`per_worktree`) only the other watcher's stream is
/// recreated. Counts the files of `wt-01` that never got an event.
pub fn stream_isolation(ctx: &mut Ctx) -> Result<Value> {
    use notify::{RecursiveMode, Watcher};
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    let extra = ctx.profile.join("isolation-extra");
    std::fs::create_dir_all(&extra)?;
    let other = ctx.root("wt-02");
    let reps = if ctx.quick { 5 } else { 10 };
    let churns = 8;
    let mut runs = Vec::new();
    for mode in ["control", "shared", "per_worktree"] {
        for r in 0..reps {
            let dir = ctx.root("wt-01").join(format!("iso-{mode}-{r}"));
            std::fs::create_dir_all(&dir)?;
            let seen: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
            let sink = |seen: Arc<Mutex<HashSet<PathBuf>>>| {
                move |res: notify::Result<notify::Event>| {
                    if let Ok(e) = res {
                        seen.lock().unwrap().extend(e.paths);
                    }
                }
            };
            let mut probe = notify::recommended_watcher(sink(seen.clone()))?;
            probe.watch(&ctx.root("wt-01"), RecursiveMode::Recursive)?;
            let mut neighbour = notify::recommended_watcher(sink(Arc::default()))?;
            neighbour.watch(&other, RecursiveMode::Recursive)?;
            if mode == "shared" {
                probe.watch(&other, RecursiveMode::Recursive)?;
            }
            std::thread::sleep(Duration::from_millis(300));
            let writer = {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let mut files = Vec::new();
                    let t_end = now_ns() + 3_000_000_000;
                    let mut i = 0;
                    while now_ns() < t_end {
                        let p = dir.join(format!("f{i:05}"));
                        let _ = std::fs::write(&p, b"x");
                        files.push(p);
                        i += 1;
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    files
                })
            };
            let mut restarts = 0;
            if mode != "control" {
                for _ in 0..churns / 2 {
                    std::thread::sleep(Duration::from_millis(600));
                    let w: &mut dyn Watcher = if mode == "shared" {
                        &mut probe
                    } else {
                        &mut neighbour
                    };
                    w.watch(&extra, RecursiveMode::Recursive)?;
                    w.unwatch(&extra)?;
                    restarts += 2;
                }
            }
            let files = writer.join().unwrap();
            std::thread::sleep(Duration::from_millis(1000));
            drop(probe);
            drop(neighbour);
            let seen = seen.lock().unwrap();
            let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            let seen: HashSet<PathBuf> = seen.iter().map(|p| canon(p)).collect();
            let lost = files.iter().filter(|f| !seen.contains(&canon(f))).count();
            runs.push(json!({"mode": mode, "files": files.len(), "stream_restarts": restarts,
                             "files_without_event": lost}));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    let total = |mode: &str, key: &str| -> u64 {
        runs.iter()
            .filter(|r| r["mode"] == mode)
            .map(|r| r[key].as_u64().unwrap_or(0))
            .sum()
    };
    let summary = json!({
        "control": {"files": total("control", "files"), "lost": total("control", "files_without_event")},
        "shared": {"files": total("shared", "files"), "restarts": total("shared", "stream_restarts"),
                   "lost": total("shared", "files_without_event")},
        "per_worktree": {"files": total("per_worktree", "files"),
                         "restarts": total("per_worktree", "stream_restarts"),
                         "lost": total("per_worktree", "files_without_event")},
    });
    Ok(json!({"runs": runs, "summary": summary}))
}
