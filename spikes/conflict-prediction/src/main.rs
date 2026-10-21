//! SPIKE-CKP-001 prototype (`ckp-spike`): dry-run merge prediction without writing to the repo.
//! Not product code. See README.md.

mod a1;
mod a2;
mod bench;
mod gitcli;
mod hunks;
mod open;
mod prefilter;
mod worker;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;

fn usage() -> ! {
    eprintln!(
        "usage:
  ckp-spike batch <a1|a2|a2w|b|ref> <repo> <pairs-file|-> [opts]   one TSV line per pair
  ckp-spike worker <repo> [opts]                                A2-W worker (stdin jobs)
  ckp-spike prefilter <repo> <pairs-file|->                     H6: may the pair conflict?
  ckp-spike intact <root> [--keep-mtime-of DIR] -- <cmd...>     fingerprint diff around cmd (E1)
  ckp-spike bench <scenario> <repo> [opts]                      E8-E11, see bench.rs
opts: --git BIN  --store DIR  --scratch DIR  --hunks  --marker N  --deadline-ms N
      --max-blob BYTES  --max-written BYTES  --rename-limit N  --fail-fast  --timeout-ms N  --max-rss BYTES"
    );
    std::process::exit(2)
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub git: PathBuf,
    pub store: Option<PathBuf>,
    pub scratch: PathBuf,
    pub hunks: bool,
    pub limits: a2::Limits,
    pub timeout: Duration,
    pub max_rss: u64,
    pub rest: Vec<String>,
}

pub fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts {
        git: "/usr/bin/git".into(),
        store: None,
        scratch: std::env::temp_dir(),
        hunks: false,
        limits: a2::Limits::default(),
        timeout: Duration::from_secs(2),
        max_rss: 0,
        rest: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_else(|| usage());
        match a.as_str() {
            "--git" => o.git = val().into(),
            "--store" => o.store = Some(val().into()),
            "--scratch" => o.scratch = val().into(),
            "--hunks" => o.hunks = true,
            "--marker" => o.limits.marker_size = val().parse().unwrap_or_else(|_| usage()),
            "--deadline-ms" => {
                o.limits.deadline = Some(Duration::from_millis(
                    val().parse().unwrap_or_else(|_| usage()),
                ))
            }
            "--max-blob" => o.limits.max_blob = val().parse().unwrap_or_else(|_| usage()),
            "--rename-limit" => o.limits.rename_limit = val().parse().unwrap_or_else(|_| usage()),
            "--fail-fast" => o.limits.fail_fast = true,
            "--max-written" => o.limits.max_written = val().parse().unwrap_or_else(|_| usage()),
            "--timeout-ms" => {
                o.timeout = Duration::from_millis(val().parse().unwrap_or_else(|_| usage()))
            }
            "--max-rss" => o.max_rss = val().parse().unwrap_or_else(|_| usage()),
            _ => o.rest.push(a.clone()),
        }
    }
    o
}

/// Worker-side args that reproduce the A2 limits.
fn limit_args(l: &a2::Limits) -> Vec<String> {
    let mut v = vec![
        "--marker".into(),
        l.marker_size.to_string(),
        "--max-blob".into(),
        l.max_blob.to_string(),
        "--rename-limit".into(),
        l.rename_limit.to_string(),
        "--max-written".into(),
        l.max_written.to_string(),
    ];
    if let Some(d) = l.deadline {
        v.extend(["--deadline-ms".into(), d.as_millis().to_string()]);
    }
    if l.fail_fast {
        v.push("--fail-fast".into());
    }
    v
}

fn resolve(repo: &gix::Repository, rev: &str) -> Result<gix::ObjectId, String> {
    repo.rev_parse_single(rev)
        .map_err(|e| format!("rev {rev}: {e}"))?
        .object()
        .map_err(|e| e.to_string())?
        .peel_to_commit()
        .map(|c| c.id)
        .map_err(|e| e.to_string())
}

/// One A2 prediction on a fresh in-memory object store, as a JSON answer.
pub fn predict_json(
    base: &gix::Repository,
    ours: &str,
    theirs: &str,
    limits: &a2::Limits,
    started: Instant,
) -> serde_json::Value {
    let repo = base.clone().with_object_memory();
    let r = resolve(&repo, ours)
        .and_then(|o| resolve(&repo, theirs).map(|t| (o, t)))
        .and_then(|(o, t)| a2::predict(&repo, o, t, limits));
    let us = started.elapsed().as_micros() as u64;
    match r {
        Ok(out) => json!({
            "ok": true, "us": us,
            "conflicts": out.conflicts.iter().map(|(p, k)| json!([p, k])).collect::<Vec<_>>(),
            "hunks": out.hunks.iter().map(|(p, h)| json!([p, h])).collect::<Vec<_>>(),
            "bases": out.merge_bases, "reads": out.object_reads, "early": out.stopped_early,
        }),
        Err(e) => json!({"ok": false, "us": us, "err": e}),
    }
}

fn read_pairs(path: &str) -> Vec<(String, String)> {
    let reader: Box<dyn BufRead> = if path == "-" {
        Box::new(std::io::BufReader::new(std::io::stdin()))
    } else {
        Box::new(std::io::BufReader::new(
            std::fs::File::open(path).unwrap_or_else(|e| panic!("{path}: {e}")),
        ))
    };
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((f.next()?.to_owned(), f.next()?.to_owned()))
        })
        .collect()
}

/// TSV: ours, theirs, ok, ms, n, paths (`|`), kinds (`|`), hunks (JSON), error.
fn emit(out: &mut impl Write, o: &str, t: &str, v: &serde_json::Value) {
    let ok = v["ok"].as_bool().unwrap_or(false);
    let ms = v["us"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let conf = v["conflicts"].as_array().cloned().unwrap_or_default();
    let paths: Vec<String> = conf
        .iter()
        .map(|c| c[0].as_str().unwrap_or("").to_owned())
        .collect();
    let kinds: Vec<String> = conf
        .iter()
        .map(|c| c[1].as_str().unwrap_or("").to_owned())
        .collect();
    let _ = writeln!(
        out,
        "{o}\t{t}\t{}\t{ms:.3}\t{}\t{}\t{}\t{}\t{}",
        if ok { "ok" } else { "err" },
        paths.len(),
        paths.join("|"),
        kinds.join("|"),
        v.get("hunks")
            .map(|h| h.to_string())
            .unwrap_or_else(|| "[]".into()),
        v["err"].as_str().unwrap_or("").replace(['\t', '\n'], " "),
    );
}

fn batch(engine: &str, repo_path: &Path, pairs: &str, o: &Opts) -> Result<(), String> {
    let pairs = read_pairs(pairs);
    let mut out = std::io::stdout().lock();
    match engine {
        "a2" => {
            let base = open::open(repo_path)?;
            for (a, b) in &pairs {
                let v = predict_json(&base, a, b, &o.limits, Instant::now());
                emit(&mut out, a, b, &v);
            }
        }
        "a1" => {
            for (a, b) in &pairs {
                let t = Instant::now();
                let v = match a1::predict(repo_path, a, b) {
                    Ok(c) => json!({"ok": true, "us": t.elapsed().as_micros() as u64,
                        "conflicts": c.iter().map(|p| json!([p, ""])).collect::<Vec<_>>()}),
                    Err(e) => json!({"ok": false, "us": t.elapsed().as_micros() as u64, "err": e}),
                };
                emit(&mut out, a, b, &v);
            }
        }
        "a2w" => {
            let mut c =
                worker::Client::new(repo_path, &limit_args(&o.limits), 30, o.max_rss, o.timeout)?;
            for (a, b) in &pairs {
                let t = Instant::now();
                let mut v = c.run(a, b);
                // Round trip as seen by the daemon, not the worker's own time.
                v["us"] = json!(t.elapsed().as_micros() as u64);
                emit(&mut out, a, b, &v);
            }
        }
        "b" | "ref" => {
            let git = gitcli::Git::new(&o.git, &o.scratch);
            let git_dir = if engine == "b" {
                let store = o.store.clone().ok_or("--store is required for b")?;
                if !store.exists() {
                    let objects = gix::open_opts(repo_path, gix::open::Options::isolated())
                        .map_err(|e| e.to_string())?
                        .common_dir()
                        .join("objects");
                    gitcli::make_store(&git, &store, &objects)?;
                }
                store
            } else {
                let dst = o.scratch.join(format!("ref-clone-{}", std::process::id()));
                if !dst.exists() {
                    gitcli::disposable_clone(&git, repo_path, &dst, o.limits.marker_size)?;
                }
                dst
            };
            let (maj, min) = git.version();
            let mut extra: Vec<&str> = Vec::new();
            if engine == "b" && (maj, min) >= (2, 45) {
                extra.push("--no-lazy-fetch");
            }
            // Revisions are resolved in the observed repo: the store has no refs.
            let resolver = gix::open_opts(repo_path, gix::open::Options::isolated())
                .map_err(|e| e.to_string())?;
            for (a, b) in &pairs {
                let t = Instant::now();
                let r = resolve(&resolver, a).and_then(|oa| {
                    resolve(&resolver, b).and_then(|ob| {
                        gitcli::merge_tree(
                            &git,
                            &git_dir,
                            &oa.to_string(),
                            &ob.to_string(),
                            &extra,
                            o.hunks,
                            o.limits.marker_size,
                        )
                    })
                });
                let us = t.elapsed().as_micros() as u64;
                let v = match r {
                    Ok(m) => json!({"ok": true, "us": us,
                        "conflicts": m.conflicts.iter().map(|p| json!([p, ""])).collect::<Vec<_>>(),
                        "hunks": m.hunks.iter().map(|(p, h)| json!([p, h])).collect::<Vec<_>>()}),
                    Err(e) => json!({"ok": false, "us": us, "err": e}),
                };
                emit(&mut out, a, b, &v);
            }
        }
        _ => usage(),
    }
    Ok(())
}

fn intact(root: &Path, keep_scope: &[String], cmd: &[String]) -> i32 {
    use gitraptor_testkit::fingerprint::{Scope, Snapshot, diff, wait_for_timestamp_tick};
    let mut scopes = vec![Scope::new("repo", root)];
    for (i, s) in keep_scope.iter().enumerate() {
        scopes.push(Scope::new(format!("extra{i}"), s));
    }
    let before = Snapshot::take(&scopes, &Default::default());
    wait_for_timestamp_tick();
    let status = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(std::process::Stdio::null())
        .status();
    wait_for_timestamp_tick();
    let after = Snapshot::take(&scopes, &Default::default());
    let changes = diff(&before, &after);
    println!(
        "cmd_exit={} entries={} changes={}",
        status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1),
        before.entries.len(),
        changes.len()
    );
    for c in &changes {
        println!("  {c}");
    }
    if changes.is_empty() { 0 } else { 3 }
}

/// R-H: profile `H` of `repogen` with the seed and the extra commit of the engine bench
/// (apps/cli/benches/engine.rs `reference`), so both measure the same repository.
fn gen_h(dir: &Path) -> Result<(), String> {
    use gitraptor_testkit::repogen;
    let git = gitraptor_testkit::fixture::git_from_path();
    let p = repogen::profile("H").ok_or("profile H")?;
    let g = repogen::generate(&git, &p, dir, 0x1f_9002)?;
    std::fs::write(dir.join("bench-touched.txt"), "0\n").map_err(|e| e.to_string())?;
    for args in [
        &["add", "bench-touched.txt"][..],
        &["commit", "-qm", "bench file"],
        &["commit-graph", "write", "--reachable"],
    ] {
        let st = repogen::git_command(&git, dir)
            .args(args)
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() {
            return Err(format!("git {args:?} failed"));
        }
    }
    println!("{g:?}");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage() };
    let code = match cmd.as_str() {
        "batch" if args.len() >= 4 => {
            let o = parse_opts(&args[4..]);
            batch(&args[1], Path::new(&args[2]), &args[3], &o).map(|_| 0)
        }
        "worker" if args.len() >= 2 => {
            let o = parse_opts(&args[2..]);
            worker::serve(Path::new(&args[1]), &o.limits).map(|_| 0)
        }
        "prefilter" if args.len() >= 3 => (|| {
            let repo = open::open(Path::new(&args[1]))?;
            let mut out = std::io::stdout().lock();
            for (a, b) in read_pairs(&args[2]) {
                let t = Instant::now();
                let r = resolve(&repo, &a).and_then(|oa| {
                    resolve(&repo, &b).and_then(|ob| prefilter::may_conflict(&repo, oa, ob))
                });
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let v = match r {
                    Ok(true) => "may".to_owned(),
                    Ok(false) => "no".to_owned(),
                    Err(e) => format!("err:{e}"),
                };
                let _ = writeln!(out, "{a}\t{b}\t{v}\t{ms:.3}");
            }
            Ok::<i32, String>(0)
        })(),
        "intact" if args.len() >= 4 => {
            let sep = args
                .iter()
                .position(|a| a == "--")
                .unwrap_or_else(|| usage());
            let extra: Vec<String> = args[2..sep]
                .chunks(2)
                .filter(|c| c[0] == "--keep-mtime-of" && c.len() == 2)
                .map(|c| c[1].clone())
                .collect();
            Ok(intact(Path::new(&args[1]), &extra, &args[sep + 1..]))
        }
        "gen-h" if args.len() >= 2 => gen_h(Path::new(&args[1])).map(|_| 0),
        "bench" if args.len() >= 3 => {
            let o = parse_opts(&args[3..]);
            bench::run(&args[1], Path::new(&args[2]), &o).map(|_| 0)
        }
        _ => usage(),
    };
    match code {
        Ok(c) => std::process::exit(c),
        Err(e) => {
            eprintln!("ckp-spike: {e}");
            std::process::exit(1)
        }
    }
}
