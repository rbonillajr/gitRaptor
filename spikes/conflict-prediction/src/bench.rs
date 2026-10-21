//! E8–E11 on a fleet made by `suites/lib/mk-fleet.sh` (`<fleet>/repo`, `<fleet>/wt-<i>`,
//! `<fleet>/pairs.tsv`). Timings follow ADR-GRP-011 § 4: the first 10 samples are discarded.
//!
//!   bench pairs     <fleet> [--engine a2|a2w] [--samples N] --pairs FILE   per-pair time
//!   bench recompute <fleet> [--engine a2|a2w] [--samples N] [--jobs J] [--prefilter]
//!   bench initial   <fleet> [--engine a2|a2w] [--samples N] [--jobs J]
//!   bench burst     <fleet> [--engine a2|a2w] [--duration S] [--jobs J] [--prefilter]
//!   bench hostile   <repo>  --pairs FILE [--samples N] (+ the A2 limit options)
//!   bench overlap   -       [--samples N] [--burst-files N]

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{Opts, a2, worker};

const WARMUP: usize = 10;

struct B {
    engine: String,
    samples: usize,
    jobs: usize,
    prefilter: bool,
    duration: Duration,
    pairs: Option<PathBuf>,
    burst_files: usize,
}

fn parse(o: &Opts) -> B {
    let mut b = B {
        engine: "a2".into(),
        samples: 200,
        jobs: 1,
        prefilter: false,
        duration: Duration::from_secs(600),
        pairs: None,
        burst_files: 10_000,
    };
    let mut it = o.rest.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--engine" => b.engine = v(),
            "--samples" => b.samples = v().parse().unwrap_or(200),
            "--jobs" => b.jobs = v().parse().unwrap_or(1),
            "--prefilter" => b.prefilter = true,
            "--duration" => b.duration = Duration::from_secs(v().parse().unwrap_or(600)),
            "--pairs" => b.pairs = Some(v().into()),
            "--burst-files" => b.burst_files = v().parse().unwrap_or(10_000),
            _ => {}
        }
    }
    b
}

pub fn summary(name: &str, ms: &[f64]) -> String {
    if ms.is_empty() {
        return format!("{name}\tn=0");
    }
    let mut v = ms.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |f: f64| v[((f * v.len() as f64) as usize).min(v.len() - 1)];
    format!(
        "{name}\tn={}\tp50={:.3}\tp95={:.3}\tp99={:.3}\tmax={:.3}\tms",
        v.len(),
        q(0.5),
        q(0.95),
        q(0.99),
        v[v.len() - 1]
    )
}

fn load() -> String {
    let mut l = [0f64; 3];
    // SAFETY: `l` holds 3 doubles, as requested.
    let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    if n == 3 {
        format!("{:.2} {:.2} {:.2}", l[0], l[1], l[2])
    } else {
        "?".into()
    }
}

fn rusage() -> (f64, u64) {
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `r` is a valid, writable `rusage`.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) };
    let cpu = r.ru_utime.tv_sec as f64
        + r.ru_utime.tv_usec as f64 / 1e6
        + r.ru_stime.tv_sec as f64
        + r.ru_stime.tv_usec as f64 / 1e6;
    (cpu, r.ru_maxrss as u64) // bytes on macOS
}

fn utility_qos() {
    // SAFETY: plain call on the current thread.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("/usr/bin/git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.com")
        .env("GIT_COMMITTER_NAME", "bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.com")
        .args(args)
        .output()
        .expect("git");
    assert!(
        st.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&st.stderr)
    );
}

/// A simulated agent commit in worktree `i` (outside the measured window).
fn agent_commit(fleet: &Path, i: usize, n: usize) {
    let wt = fleet.join(format!("wt-{i}"));
    std::fs::write(wt.join("AGENT-BENCH.txt"), format!("{n}\n")).expect("write");
    git(&wt, &["add", "AGENT-BENCH.txt"]);
    git(&wt, &["commit", "-qm", &format!("bench {n}")]);
}

fn read_pairs(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?.to_owned(), f.next()?.to_owned()))
        })
        .collect()
}

/// Computes a set of pairs with `jobs` workers; returns (pairs computed, conflicts found).
struct Engine {
    a2_base: Option<gix::Repository>,
    clients: Vec<Mutex<worker::Client>>,
    limits: a2::Limits,
    prefilter: bool,
    jobs: usize,
}

impl Engine {
    fn new(repo: &Path, b: &B, o: &Opts) -> Result<Self, String> {
        let mut e = Engine {
            a2_base: None,
            clients: Vec::new(),
            limits: o.limits.clone(),
            prefilter: b.prefilter,
            jobs: b.jobs.max(1),
        };
        if b.engine == "a2w" {
            for _ in 0..e.jobs {
                let args = vec![
                    "--max-blob".into(),
                    o.limits.max_blob.to_string(),
                    "--rename-limit".into(),
                    o.limits.rename_limit.to_string(),
                    "--max-written".into(),
                    o.limits.max_written.to_string(),
                ];
                e.clients.push(Mutex::new(worker::Client::new(
                    repo, &args, 30, o.max_rss, o.timeout,
                )?));
            }
        }
        e.a2_base = Some(crate::open::open(repo)?);
        Ok(e)
    }

    fn compute(&self, pairs: &[(String, String)]) -> (usize, usize) {
        let base = self.a2_base.as_ref().expect("repo");
        // The prefilter runs in the caller, before dispatch (it is part of the measured time).
        let todo: Vec<&(String, String)> = if self.prefilter {
            let repo = base.clone();
            pairs
                .iter()
                .filter(|(a, b)| {
                    let (Ok(x), Ok(y)) = (
                        repo.rev_parse_single(a.as_str()),
                        repo.rev_parse_single(b.as_str()),
                    ) else {
                        return true;
                    };
                    crate::prefilter::may_conflict(&repo, x.detach(), y.detach()).unwrap_or(true)
                })
                .collect()
        } else {
            pairs.iter().collect()
        };
        let next = Mutex::new(0usize);
        let found = Mutex::new(0usize);
        let (clients, limits) = (&self.clients, &self.limits);
        std::thread::scope(|s| {
            for j in 0..self.jobs {
                let repo = base.clone();
                let (next, found, todo) = (&next, &found, &todo);
                s.spawn(move || {
                    utility_qos();
                    loop {
                        let k = {
                            let mut n = next.lock().expect("lock");
                            *n += 1;
                            *n - 1
                        };
                        let Some((a, b)) = todo.get(k) else { break };
                        let v = if clients.is_empty() {
                            crate::predict_json(&repo, a, b, limits, Instant::now())
                        } else {
                            clients[j].lock().expect("lock").run(a, b)
                        };
                        if v["conflicts"].as_array().is_some_and(|c| !c.is_empty()) {
                            *found.lock().expect("lock") += 1;
                        }
                    }
                });
            }
        });
        (todo.len(), found.into_inner().expect("lock"))
    }

    fn child_rss(&self) -> u64 {
        self.clients
            .iter()
            .map(|c| {
                let c = c.lock().expect("lock");
                worker::rss_of(c.pid()).max(c.peak_rss)
            })
            .sum()
    }
}

fn affected(pairs: &[(String, String)], i: usize) -> Vec<(String, String)> {
    let me = format!("agent-{i}");
    pairs
        .iter()
        .filter(|(a, b)| *a == me || *b == me)
        .cloned()
        .collect()
}

pub fn run(scenario: &str, fleet: &Path, o: &Opts) -> Result<(), String> {
    let b = parse(o);
    println!(
        "# scenario={scenario} engine={} jobs={} prefilter={} load_start={}",
        b.engine,
        b.jobs,
        b.prefilter,
        load()
    );
    match scenario {
        "pairs" => {
            let pairs = read_pairs(b.pairs.as_deref().ok_or("--pairs")?);
            let e = Engine::new(&fleet.join("repo"), &b, o)?;
            for (a, c) in &pairs {
                let mut ms = Vec::new();
                for s in 0..b.samples + WARMUP {
                    let t = Instant::now();
                    e.compute(&[(a.clone(), c.clone())]);
                    if s >= WARMUP {
                        ms.push(t.elapsed().as_secs_f64() * 1e3);
                    }
                }
                println!("{}", summary(&format!("pair {a} {c}"), &ms));
            }
        }
        "recompute" => {
            let pairs = read_pairs(&fleet.join("pairs.tsv"));
            let e = Engine::new(&fleet.join("repo"), &b, o)?;
            let (cpu0, _) = rusage();
            let mut ms = Vec::new();
            let mut computed = 0;
            let t_all = Instant::now();
            let mut busy = 0.0;
            for s in 0..b.samples + WARMUP {
                let i = s % 10;
                agent_commit(fleet, i, s);
                let t0 = Instant::now(); // end of the commit
                let (n, _) = e.compute(&affected(&pairs, i));
                let d = t0.elapsed().as_secs_f64();
                if s >= WARMUP {
                    ms.push(d * 1e3);
                    computed += n;
                    busy += d;
                }
            }
            let (cpu1, maxrss) = rusage();
            println!("{}", summary("recompute-after-commit", &ms));
            println!(
                "pairs_computed={computed} cpu_s={:.2} cpu_pct_of_busy={:.0} maxrss_mb={:.0} child_rss_mb={:.0} wall_s={:.1} load_end={}",
                cpu1 - cpu0,
                100.0 * (cpu1 - cpu0) / busy.max(1e-9),
                maxrss as f64 / 1048576.0,
                e.child_rss() as f64 / 1048576.0,
                t_all.elapsed().as_secs_f64(),
                load()
            );
        }
        "initial" => {
            let pairs = read_pairs(&fleet.join("pairs.tsv"));
            let mut ms = Vec::new();
            for s in 0..b.samples + WARMUP {
                // A fresh handle each time: the daemon starting on a repo, or the base moving.
                let t0 = Instant::now();
                let e = Engine::new(&fleet.join("repo"), &b, o)?;
                e.compute(&pairs);
                if s >= WARMUP {
                    ms.push(t0.elapsed().as_secs_f64() * 1e3);
                }
            }
            println!("{}", summary("initial-55-pairs", &ms));
        }
        "burst" => burst(fleet, &b, o)?,
        "hostile" => {
            let pairs = read_pairs(b.pairs.as_deref().ok_or("--pairs")?);
            let repo = crate::open::open(fleet)?;
            for (a, c) in &pairs {
                let mut ms = Vec::new();
                let mut errs: BTreeMap<String, usize> = BTreeMap::new();
                let mut rss = 0u64;
                let mut w = if b.engine == "a2w" {
                    Some(worker::Client::new(
                        fleet,
                        &[
                            "--max-blob".into(),
                            o.limits.max_blob.to_string(),
                            "--rename-limit".into(),
                            o.limits.rename_limit.to_string(),
                            "--max-written".into(),
                            o.limits.max_written.to_string(),
                        ],
                        30,
                        o.max_rss,
                        o.timeout,
                    )?)
                } else {
                    None
                };
                for _ in 0..b.samples {
                    let t = Instant::now();
                    let v = match w.as_mut() {
                        Some(w) => w.run(a, c),
                        None => crate::predict_json(&repo, a, c, &o.limits, Instant::now()),
                    };
                    ms.push(t.elapsed().as_secs_f64() * 1e3);
                    let key = if v["ok"].as_bool() == Some(true) {
                        format!("ok:{}", v["conflicts"].as_array().map_or(0, |x| x.len()))
                    } else {
                        let e = v["err"].as_str().unwrap_or("?");
                        if let Some(i) = e.find("ckp-limit") {
                            e[i..].split(':').take(2).collect::<Vec<_>>().join(":")
                        } else {
                            e.chars().take(60).collect()
                        }
                    };
                    *errs.entry(key).or_default() += 1;
                    rss = rss.max(w.as_ref().map_or(rusage().1, |w| w.peak_rss));
                }
                println!(
                    "{}\toutcomes={errs:?}\tpeak_rss_mb={:.0}\trestarts={}",
                    summary(&format!("hostile {a} {c}"), &ms),
                    rss as f64 / 1048576.0,
                    w.as_ref().map_or(0, |w| w.restarts)
                );
            }
        }
        "overlap" => overlap(&b),
        _ => return Err(format!("unknown scenario {scenario}")),
    }
    println!("# load_end={}", load());
    Ok(())
}

/// E8 burst: 10 agents commit every 2–10 s; the predictor coalesces per worktree (only the
/// latest commit of a worktree is computed) and publishes. Age = publication − commit end.
fn burst(fleet: &Path, b: &B, o: &Opts) -> Result<(), String> {
    let pairs = read_pairs(&fleet.join("pairs.tsv"));
    let e = Engine::new(&fleet.join("repo"), b, o)?;
    let (tx, rx) = channel::<(usize, Instant)>();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut agents = Vec::new();
    for i in 0..10usize {
        let (tx, stop, fleet) = (tx.clone(), stop.clone(), fleet.to_owned());
        agents.push(std::thread::spawn(move || {
            let mut seed = 0x9E37_79B9u64.wrapping_mul(i as u64 + 1);
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let wait = Duration::from_millis(2000 + seed % 8000);
                let until = Instant::now() + wait;
                while Instant::now() < until && !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                agent_commit(&fleet, i, 100_000 + n);
                n += 1;
                if tx.send((i, Instant::now())).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);
    let (cpu0, _) = rusage();
    let start = Instant::now();
    let mut ages = Vec::new();
    let mut max_depth = 0;
    let (mut events, mut coalesced, mut rounds) = (0usize, 0usize, 0usize);
    let mut busy = 0.0;
    while start.elapsed() < b.duration {
        let Ok(first) = rx.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        // Everything queued now: keep the oldest pending commit time per worktree.
        let mut pending: BTreeMap<usize, Instant> = BTreeMap::new();
        pending.insert(first.0, first.1);
        let mut depth = 1;
        while let Ok((i, t)) = rx.try_recv() {
            depth += 1;
            match pending.entry(i) {
                std::collections::btree_map::Entry::Occupied(_) => coalesced += 1,
                std::collections::btree_map::Entry::Vacant(v) => {
                    v.insert(t);
                }
            }
        }
        events += depth;
        max_depth = max_depth.max(depth);
        let mut set: Vec<(String, String)> = Vec::new();
        for i in pending.keys() {
            for p in affected(&pairs, *i) {
                if !set.contains(&p) {
                    set.push(p);
                }
            }
        }
        let t = Instant::now();
        e.compute(&set);
        busy += t.elapsed().as_secs_f64();
        rounds += 1;
        let now = Instant::now();
        for t in pending.values() {
            ages.push(now.duration_since(*t).as_secs_f64() * 1e3);
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for a in agents {
        let _ = a.join();
    }
    let (cpu1, maxrss) = rusage();
    let wall = start.elapsed().as_secs_f64();
    println!("{}", summary("burst-published-age", &ages));
    println!(
        "events={events} coalesced={coalesced} rounds={rounds} max_queue_depth={max_depth} busy_pct={:.0} cpu_pct_avg_of_one_core={:.1} maxrss_mb={:.0} child_rss_mb={:.0} wall_s={wall:.0}",
        100.0 * busy / wall,
        100.0 * (cpu1 - cpu0) / wall,
        maxrss as f64 / 1048576.0,
        e.child_rss() as f64 / 1048576.0
    );
    Ok(())
}

/// E11: sets of uncommitted paths per worktree (10), one of them with a burst of N files;
/// cost of one full intersection round (each worktree against the other 9).
fn overlap(b: &B) {
    let mut sets: Vec<HashSet<String>> = (0..10)
        .map(|i| {
            (0..50)
                .map(|k| format!("src/m{i}/p{k}/f{}.rs", k * 7 + i))
                .collect()
        })
        .collect();
    sets[0] = (0..b.burst_files)
        .map(|k| format!("src/m{}/p{}/f{k}.rs", k % 8, k % 13))
        .collect();
    sets[1].insert("src/m1/p1/f1.rs".into());
    let mut ms = Vec::new();
    let mut hits = 0;
    for s in 0..b.samples + WARMUP {
        let t = Instant::now();
        let mut n = 0;
        for i in 0..10 {
            for j in i + 1..10 {
                let (small, big) = if sets[i].len() < sets[j].len() {
                    (&sets[i], &sets[j])
                } else {
                    (&sets[j], &sets[i])
                };
                n += small.iter().filter(|p| big.contains(*p)).count();
            }
        }
        hits = n;
        if s >= WARMUP {
            ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
    }
    let bytes: usize = sets
        .iter()
        .flat_map(|s| s.iter())
        .map(|p| p.len() + 24)
        .sum::<usize>()
        + sets.iter().map(|s| s.capacity() * 8).sum::<usize>();
    println!(
        "{}",
        summary(&format!("overlap-round burst_files={}", b.burst_files), &ms)
    );
    println!(
        "intersections_found={hits} approx_memory_kb={}",
        bytes / 1024
    );
}
