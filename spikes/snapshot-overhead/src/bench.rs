//! Benchmark scenarios of SPIKE-TMC-001. Everything runs under a temporary root; the
//! generated repo plays the "user repo" and the bench plays the agent that edits files.

use crate::repogen::{self, Rng};
use crate::snap::{SeedMode, Shared, Timing, Variant, Wt, init_store, seed};
use crate::util::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Seeding used by the measured scenarios. Hard links are NOT used: see `seeding()`.
const SEED: SeedMode = SeedMode::Clone;

pub struct Opts {
    pub profile: String,
    pub root: PathBuf,
    pub variants: Vec<Variant>,
    pub iters: usize,
    pub multi: bool,
    pub week: usize,
    pub big: bool,
    pub seed_exp: bool,
}

struct Report(String);
impl Report {
    fn line(&mut self, s: impl AsRef<str>) {
        println!("{}", s.as_ref());
        self.0.push_str(s.as_ref());
        self.0.push('\n');
    }
}

/// Files the bench edits, with their original bytes so they can be restored.
struct Editor {
    rng: Rng,
    text: Vec<String>,
    assets: Vec<(String, u64)>,
    originals: HashMap<(PathBuf, String), Vec<u8>>,
}

impl Editor {
    fn new(repo: &Path) -> Res<Self> {
        let mut c = git();
        c.current_dir(repo).args(["ls-files", "-z"]);
        let out = run(c)?;
        let mut text = Vec::new();
        let mut assets = Vec::new();
        for p in out.split(|&b| b == 0).filter(|p| !p.is_empty()) {
            let p = String::from_utf8_lossy(p).to_string();
            if p.starts_with("assets/") {
                let len = std::fs::metadata(repo.join(&p))?.len();
                assets.push((p, len));
            } else if p.starts_with("src/") {
                text.push(p);
            }
        }
        Ok(Editor {
            rng: Rng::new(7),
            text,
            assets,
            originals: HashMap::new(),
        })
    }

    fn pick_text(&mut self, n: usize, offset: usize) -> Vec<String> {
        (0..n)
            .map(|i| self.text[(offset + i * 7919) % self.text.len()].clone())
            .collect()
    }

    /// `n` files with about `bytes` of content: big assets first, text to complete the count.
    fn pick_heavy(&mut self, n: usize, bytes: u64) -> Vec<String> {
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

    fn mutate(&mut self, wt: &Path, paths: &[String]) -> Res<()> {
        for p in paths {
            let full = wt.join(p);
            let mut content = std::fs::read(&full)?;
            self.originals
                .entry((wt.to_path_buf(), p.clone()))
                .or_insert_with(|| content.clone());
            if p.starts_with("assets/") {
                self.rng.fill(&mut content);
            } else {
                repogen::edit_text(&mut self.rng, &mut content);
            }
            std::fs::write(&full, &content)?;
        }
        Ok(())
    }

    fn restore(&mut self, wt: &Path) -> Res<Vec<String>> {
        let keys: Vec<_> = self
            .originals
            .keys()
            .filter(|(w, _)| w == wt)
            .cloned()
            .collect();
        let mut paths = Vec::new();
        for k in keys {
            let content = self.originals.remove(&k).unwrap();
            std::fs::write(wt.join(&k.1), content)?;
            paths.push(k.1);
        }
        Ok(paths)
    }
}

fn stage_stats(ts: &[Timing]) -> [Stats; 8] {
    let f = |g: fn(&Timing) -> Duration| stats(&ts.iter().map(|t| ms(g(t))).collect::<Vec<_>>());
    [
        f(|t| t.total),
        f(|t| t.detect),
        f(|t| t.queue),
        f(|t| t.anchor),
        f(|t| t.blobs),
        f(|t| t.trees),
        f(|t| t.ref_oplog),
        stats(
            &ts.iter()
                .map(|t| t.hashed_bytes as f64 / 1_048_576.0)
                .collect::<Vec<_>>(),
        ),
    ]
}

fn row(r: &mut Report, variant: &str, scenario: &str, ts: &[Timing]) {
    let [total, detect, queue, anchor, blobs, trees, refo, mb] = stage_stats(ts);
    r.line(format!(
        "| {variant} | {scenario} | {} | **{:.1}** | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
        total.n, total.p95, total.p50, total.max, detect.p95, queue.p95, anchor.p95, blobs.p95, trees.p95, refo.p95, mb.p50
    ));
}

fn table_header(r: &mut Report) {
    r.line("| variante | escenario | n | p95 total (ms) | p50 | máx | detección p95 | cola p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB nuevos (p50) |");
    r.line("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
}

fn ensure_repo(opts: &Opts) -> Res<(PathBuf, String)> {
    let p = repogen::profile(&opts.profile)?;
    let repo = opts.root.join(format!("repo-{}", p.name));
    let marker = repo.join(".git/spike-generated");
    if marker.exists() {
        return Ok((repo, std::fs::read_to_string(marker)?));
    }
    println!("generating profile {} into {} ...", p.name, repo.display());
    let g = repogen::generate(&p, &repo, 42)?;
    let desc = format!(
        "{} HEAD={} tracked_files={} wt_bytes={} commits={} pack_kb={} gen_secs={:.1}",
        p.name, g.head, g.tracked_files, g.wt_bytes, g.commits, g.pack_kb, g.secs
    );
    std::fs::write(&marker, &desc)?;
    Ok((repo, desc))
}

fn ensure_worktrees(repo: &Path, n: usize) -> Res<Vec<PathBuf>> {
    let mut out = vec![repo.to_path_buf()];
    for i in 1..n {
        let p = repo.with_file_name(format!(
            "{}-wt{i}",
            repo.file_name().unwrap().to_string_lossy()
        ));
        if !p.exists() {
            let mut c = git();
            c.current_dir(repo)
                .args(["worktree", "add", "-q", "-b", &format!("wt{i}")])
                .arg(&p);
            run(c)?;
        }
        out.push(p);
    }
    Ok(out)
}

/// The "agent" refreshes the stat info of its index, as any `git status` of a user or IDE
/// would. Not part of a snapshot and never inside a measured window.
fn agent_refresh(wts: &[PathBuf]) -> Res<()> {
    std::thread::sleep(Duration::from_millis(1100)); // avoid racily-clean entries
    for w in wts {
        let mut c = git();
        c.current_dir(w).args(["update-index", "-q", "--refresh"]);
        let _ = run(c);
    }
    Ok(())
}

#[derive(Default)]
struct Guard {
    windows: usize,
    diffs: Vec<String>,
}

impl Guard {
    fn check(&mut self, label: &str, a: &(Vec<String>, String), b: &(Vec<String>, String)) {
        self.windows += 1;
        let mut d = fingerprint_diff(&a.0, &b.0);
        if a.1 != b.1 {
            d.push(format!("refs/log/stash: {} -> {}", a.1, b.1));
        }
        for x in d {
            self.diffs.push(format!("{label}: {x}"));
        }
    }
}

fn integrity(repo: &Path) -> Res<(Vec<String>, String)> {
    let gitdir = repo.join(".git");
    let fp = fingerprint_git_dir(&gitdir)?;
    let mut c = git();
    c.current_dir(repo).args(["for-each-ref"]);
    let refs = blob_sha(&run(c)?);
    let mut c = git();
    c.current_dir(repo).args(["log", "--all", "--format=%H"]);
    let log = blob_sha(&run(c)?);
    let mut c = git();
    c.current_dir(repo).args(["stash", "list"]);
    let stash = blob_sha(&run(c)?);
    Ok((fp, format!("refs={refs} log_all={log} stash={stash}")))
}

fn rusage_cpu() -> f64 {
    let get = |who| {
        // SAFETY: getrusage writes into a zeroed, properly sized struct.
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(who, &mut ru) };
        ru.ru_utime.tv_sec as f64
            + ru.ru_utime.tv_usec as f64 / 1e6
            + ru.ru_stime.tv_sec as f64
            + ru.ru_stime.tv_usec as f64 / 1e6
    };
    get(libc::RUSAGE_SELF) + get(libc::RUSAGE_CHILDREN)
}

fn hw() -> String {
    let s = |args: &[&str]| {
        let mut c = std::process::Command::new(args[0]);
        c.args(&args[1..]);
        run(c).map(|o| trim(&o)).unwrap_or_default()
    };
    format!(
        "{} · {} núcleos · {} GiB RAM · macOS {} ({}) · {} · git {}",
        s(&["sysctl", "-n", "machdep.cpu.brand_string"]),
        s(&["sysctl", "-n", "hw.ncpu"]),
        s(&["sysctl", "-n", "hw.memsize"])
            .parse::<u64>()
            .unwrap_or(0)
            >> 30,
        s(&["sw_vers", "-productVersion"]),
        s(&["sw_vers", "-buildVersion"]),
        s(&[
            "sh",
            "-c",
            "diskutil info / | grep 'File System Personality' | sed 's/.*: *//'"
        ]),
        s(&["git", "--version"]).replace("git version ", ""),
    )
}

pub fn run_all(opts: &Opts) -> Res<()> {
    let mut r = Report(String::new());
    let (repo, desc) = ensure_repo(opts)?;
    let user_git = repo.join(".git");
    let wts = if opts.multi {
        ensure_worktrees(&repo, 10)?
    } else {
        vec![repo.clone()]
    };
    r.line(format!("# SPIKE-TMC-001 — resultados ({})\n", opts.profile));
    r.line(format!("- Hardware: {}", hw()));
    r.line(format!("- Repo: {}", desc.trim()));
    r.line(format!(
        "- Iteraciones por escenario: {} (1.000 archivos: {})",
        opts.iters,
        (opts.iters / 4).max(10)
    ));
    r.line(format!("- Carga de la máquina al empezar: {}", load()));
    agent_refresh(&wts)?;
    let mut guard = Guard::default();

    micro(&mut r, &repo)?;
    if opts.seed_exp {
        seeding(&mut r, opts, &user_git)?;
    }

    r.line("\n## Snapshot previo, 1 worktree, almacén sembrado (clon APFS)\n");
    table_header(&mut r);
    let mut ed = Editor::new(&repo)?;
    for &v in &opts.variants {
        let prof = opts.root.join(format!("prof-{}", v.name()));
        let _ = std::fs::remove_dir_all(&prof);
        let store = init_store(&prof)?;
        seed(&user_git, &store, SEED)?;
        let sh = Shared::open(v, &prof, &store, &user_git)?;
        let t0 = Instant::now();
        let mut wt0 = sh.new_wt("w0", &repo)?;
        let (_, first) = sh.snapshot(&mut [&mut wt0], Some(&[vec![]]), "observacion")?;
        r.line(format!(
            "| {} | primera captura (índice reflejado + árbol) | 1 | {:.1} | | | | | | | | | |",
            v.name(),
            ms(t0.elapsed())
        ));
        let _ = first;
        let scenarios: Vec<(&str, usize, Option<u64>)> = vec![
            ("sin cambios (camino rápido)", 0, None),
            ("delta 1 archivo", 1, None),
            ("delta 10 archivos", 10, None),
            ("delta 100 archivos", 100, None),
            ("delta 100 archivos / ~20 MB", 100, Some(20 << 20)),
            ("delta 1.000 archivos", 1000, None),
        ];
        for (name, n, heavy) in scenarios {
            let paths = match heavy {
                Some(b) => ed.pick_heavy(n, b),
                None => ed.pick_text(n, 0),
            };
            let iters = if n >= 1000 {
                (opts.iters / 4).max(10)
            } else {
                opts.iters
            };
            let mut ts = Vec::new();
            let g0 = integrity(&repo)?;
            for i in 0..iters + 3 {
                if n > 0 {
                    ed.mutate(&repo, &paths)?;
                }
                let hint = vec![paths.clone()];
                let (_, t) = sh.snapshot(&mut [&mut wt0], Some(&hint), "previo_garantizado")?;
                if i >= 3 {
                    ts.push(t);
                }
            }
            guard.check(&format!("{} {name}", v.name()), &g0, &integrity(&repo)?);
            row(&mut r, v.name(), name, &ts);
            let restored = ed.restore(&repo)?;
            agent_refresh(std::slice::from_ref(&repo))?;
            sh.snapshot(&mut [&mut wt0], Some(&[restored]), "observacion")?;
        }
        if opts.big {
            for (label, size) in [
                ("50 MB", 50u64 << 20),
                ("200 MB", 200 << 20),
                ("1 GB", 1 << 30),
            ] {
                let p = "big-untracked.bin".to_string();
                let mut ts = Vec::new();
                for _ in 0..3 {
                    let mut buf = vec![0u8; size as usize];
                    ed.rng.fill(&mut buf);
                    std::fs::write(repo.join(&p), &buf)?;
                    drop(buf);
                    let (_, t) = sh.snapshot(
                        &mut [&mut wt0],
                        Some(&[vec![p.clone()]]),
                        "previo_garantizado",
                    )?;
                    ts.push(t);
                }
                row(
                    &mut r,
                    v.name(),
                    &format!("archivo sin seguimiento de {label}"),
                    &ts,
                );
                std::fs::remove_file(repo.join(&p))?;
                sh.snapshot(&mut [&mut wt0], Some(&[vec![p]]), "observacion")?;
            }
        }
        sh.close()?;
        r.line(format!(
            "| {} | (almacén tras el escenario: {} MiB, sin contar la siembra compartida) | | | | | | | | | | | |",
            v.name(),
            dir_size_kb(&store)? / 1024 - dir_size_kb(&user_git.join("objects/pack"))? / 1024
        ));
    }

    if opts.multi {
        multi(&mut r, opts, &repo, &user_git, &wts, &mut ed, &mut guard)?;
    }
    if opts.week > 0 {
        week(&mut r, opts, &repo, &user_git, &mut ed, &mut guard)?;
    }

    r.line("\n## Repo intacto (NFR-01, garantía 4 de ADR-TMC-001)\n");
    r.line(format!(
        "- {} ventanas medidas; en cada una se compara la huella de todo `.git` de todos los worktrees (ruta, tamaño, mtime, inodo y contenido salvo packs), `for-each-ref`, `log --all` y `stash list` antes y después.",
        guard.windows
    ));
    r.line(format!(
        "- Resultado: **{}**",
        if guard.diffs.is_empty() {
            "idéntico en todas las ventanas".to_string()
        } else {
            format!("{} diferencias", guard.diffs.len())
        }
    ));
    for d in guard.diffs.iter().take(20) {
        r.line(format!("  - `{d}`"));
    }
    r.line(format!("- Carga de la máquina al terminar: {}", load()));
    let out = opts.root.join(format!("results-{}.md", opts.profile));
    std::fs::write(&out, &r.0)?;
    println!("\nresults written to {}", out.display());
    if !guard.diffs.is_empty() {
        return err("user repository changed");
    }
    Ok(())
}

fn load() -> String {
    let mut c = std::process::Command::new("sysctl");
    c.args(["-n", "vm.loadavg"]);
    run(c).map(|o| trim(&o)).unwrap_or_default()
}

fn micro(r: &mut Report, repo: &Path) -> Res<()> {
    r.line("\n## Micro-mediciones\n");
    for bin in ["git", git_bin()] {
        let mut spawn = Vec::new();
        for _ in 0..200 {
            let t = Instant::now();
            let mut c = git_with(bin);
            c.arg("--version");
            run(c)?;
            spawn.push(ms(t.elapsed()));
        }
        let s = stats(&spawn);
        r.line(format!(
            "- Lanzar `{bin} --version`: p50 {:.2} ms, p95 {:.2} ms",
            s.p50, s.p95
        ));
    }
    let mut st = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let mut c = git();
        c.current_dir(repo).args([
            "-c",
            "core.untrackedCache=false",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ]);
        run(c)?;
        st.push(ms(t.elapsed()));
    }
    let s = stats(&st);
    r.line(format!(
        "- `git status --porcelain=v2 -uall` en el repo limpio: p50 {:.1} ms, p95 {:.1} ms",
        s.p50, s.p95
    ));
    let mut buf = vec![0u8; 20 << 20];
    Rng::new(1).fill(&mut buf);
    let t = Instant::now();
    let _ = blob_sha(&buf);
    let smol = ms(t.elapsed());
    let t = Instant::now();
    let mut h = gix::hash::hasher(gix::hash::Kind::Sha1);
    h.update(&buf);
    let _ = h.try_finalize();
    r.line(format!(
        "- SHA-1 de 20 MiB en el proceso: `sha1_smol` {smol:.1} ms · gitoxide (SHA-1 con detección de colisiones) {:.1} ms",
        ms(t.elapsed())
    ));
    let tmp = repo.with_file_name("fsync-probe");
    let mut plain = Vec::new();
    let mut full = Vec::new();
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&tmp)?;
        for i in 0..400 {
            f.write_all(&[b'x'; 200])?;
            let t = Instant::now();
            if i % 2 == 0 {
                fsync_plain(&f)?;
                plain.push(ms(t.elapsed()));
            } else {
                fsync_full(&f)?;
                full.push(ms(t.elapsed()));
            }
        }
    }
    std::fs::remove_file(&tmp)?;
    let (a, b) = (stats(&plain), stats(&full));
    r.line(format!(
        "- Fila de 200 B + `fsync` simple: p50 {:.2} / p95 {:.2} ms · con `F_FULLFSYNC`: p50 {:.2} / p95 {:.2} ms",
        a.p50, a.p95, b.p50, b.p95
    ));
    Ok(())
}

fn seeding(r: &mut Report, opts: &Opts, user_git: &Path) -> Res<()> {
    r.line("\n## Siembra del almacén\n");
    r.line("| modo | tiempo (s) | espacio libre consumido (MiB) | `du` del almacén (MiB) |");
    r.line("|---|---|---|---|");
    for (name, mode) in [
        ("enlace duro", SeedMode::Hardlink),
        ("clon APFS (`cp -c`)", SeedMode::Clone),
        ("copia de bytes", SeedMode::Copy),
    ] {
        let prof = opts.root.join("prof-seed");
        let _ = std::fs::remove_dir_all(&prof);
        let store = init_store(&prof)?;
        let f0 = free_kb(&opts.root)?;
        let t = Instant::now();
        seed(user_git, &store, mode)?;
        let secs = t.elapsed().as_secs_f64();
        std::thread::sleep(Duration::from_secs(2));
        let f1 = free_kb(&opts.root)?;
        r.line(format!(
            "| {name} | {secs:.2} | {} | {} |",
            (f0 as i64 - f1 as i64) / 1024,
            dir_size_kb(&store)? / 1024
        ));
        std::fs::remove_dir_all(&prof)?;
    }
    // Side effect of hard links: Git "freshens" a packed object it is asked to write again by
    // touching the pack's mtime. With a hard-linked pack that is the user's pack file.
    let mut c = git();
    c.arg("--git-dir")
        .arg(user_git)
        .args(["ls-tree", "-r", "HEAD", "--name-only"]);
    let some_file = String::from_utf8(run(c)?)?
        .lines()
        .find(|l| l.starts_with("src/"))
        .ok_or("no file")?
        .to_string();
    let wt = user_git.parent().ok_or("repo")?;
    for (name, mode) in [
        ("enlace duro", SeedMode::Hardlink),
        ("clon APFS", SeedMode::Clone),
    ] {
        let prof = opts.root.join("prof-freshen");
        let _ = std::fs::remove_dir_all(&prof);
        let store = init_store(&prof)?;
        seed(user_git, &store, mode)?;
        let before = fingerprint_git_dir(user_git)?;
        std::thread::sleep(Duration::from_millis(1100));
        let mut c = git();
        c.env("GIT_DIR", &store).current_dir(wt).args([
            "hash-object",
            "-w",
            "--no-filters",
            &some_file,
        ]);
        run(c)?;
        let diff = fingerprint_diff(&before, &fingerprint_git_dir(user_git)?);
        r.line(format!(
            "- Siembra por {name} y luego `hash-object -w` en el almacén de un archivo que ya está en el pack: el `.git` del usuario {}",
            if diff.is_empty() { "no cambia".to_string() } else { format!("**cambia** ({} entradas: el `mtime` del pack del usuario)", diff.len() / 2) }
        ));
        std::fs::remove_dir_all(&prof)?;
    }
    Ok(())
}

fn multi(
    r: &mut Report,
    opts: &Opts,
    repo: &Path,
    user_git: &Path,
    wts: &[PathBuf],
    ed: &mut Editor,
    guard: &mut Guard,
) -> Res<()> {
    r.line("\n## 10 worktrees activos\n");
    r.line("Primer plano: snapshot previo de `w0` con un delta de 100 archivos. Fondo: 9 worktrees con capturas por observación (10 archivos cada `Q` = 1 s; `wt1` empieza con una ráfaga de 1.000 archivos). Un escritor por almacén.\n");
    table_header(r);
    let mut cpu_lines = Vec::new();
    for &v in &opts.variants {
        let prof = opts.root.join(format!("prof-multi-{}", v.name()));
        let _ = std::fs::remove_dir_all(&prof);
        let store = init_store(&prof)?;
        seed(user_git, &store, SEED)?;
        let sh = Shared::open(v, &prof, &store, user_git)?;
        let mut states: Vec<Wt> = wts
            .iter()
            .enumerate()
            .map(|(i, p)| sh.new_wt(&format!("w{i}"), p))
            .collect::<Res<_>>()?;
        for st in states.iter_mut() {
            sh.snapshot(&mut [st], Some(&[vec![]]), "observacion")?;
        }
        let (fg, bg) = states.split_first_mut().unwrap();
        let stop = AtomicBool::new(false);
        let bg_caps = std::sync::atomic::AtomicUsize::new(0);
        let cpu0 = rusage_cpu();
        let store_kb0 = dir_size_kb(&store)?;
        let wall = Instant::now();
        let fg_paths = ed.pick_text(100, 0);
        let mut fg_ts = Vec::new();
        let bg_times = std::sync::Mutex::new(Vec::new());
        let g0 = integrity(repo)?;
        std::thread::scope(|s| -> Res<()> {
            for (i, st) in bg.iter_mut().enumerate() {
                let (sh, stop, caps, bgt) = (&sh, &stop, &bg_caps, &bg_times);
                let mut med = Editor::new(&st.path).expect("editor");
                med.rng = Rng::new(100 + i as u64);
                s.spawn(move || {
                    let mut first = i == 0;
                    while !stop.load(Ordering::Relaxed) {
                        let paths = if first {
                            med.pick_text(1000, 3)
                        } else {
                            med.pick_text(10, i * 97)
                        };
                        first = false;
                        med.mutate(&st.path.clone(), &paths).expect("mutate");
                        std::thread::sleep(Duration::from_millis(1000));
                        let (_, t) = sh
                            .snapshot(&mut [&mut *st], Some(&[paths]), "observacion")
                            .expect("bg snapshot");
                        bgt.lock().unwrap().push(t);
                        caps.fetch_add(1, Ordering::Relaxed);
                    }
                    let restored = med.restore(&st.path.clone()).expect("restore");
                    sh.snapshot(&mut [&mut *st], Some(&[restored]), "observacion")
                        .expect("bg final");
                });
            }
            std::thread::sleep(Duration::from_millis(500));
            for i in 0..opts.iters + 3 {
                ed.mutate(repo, &fg_paths)?;
                let (_, t) = sh.snapshot(
                    &mut [&mut *fg],
                    Some(std::slice::from_ref(&fg_paths)),
                    "previo_garantizado",
                )?;
                if i >= 3 {
                    fg_ts.push(t);
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            stop.store(true, Ordering::Relaxed);
            Ok(())
        })?;
        let secs = wall.elapsed().as_secs_f64();
        let cpu = rusage_cpu() - cpu0;
        guard.check(&format!("{} multi", v.name()), &g0, &integrity(repo)?);
        row(
            r,
            v.name(),
            "previo w0 (100 arch.) con 9 worktrees capturando",
            &fg_ts,
        );
        let bgt = bg_times.into_inner().unwrap();
        row(r, v.name(), "capturas por observación de fondo", &bgt);
        cpu_lines.push(format!(
            "- `{}`: {} capturas de fondo en {:.0} s; CPU total del proceso y sus hijos {:.1} s = **{:.0} % de un núcleo** en promedio; almacén +{} MiB",
            v.name(),
            bg_caps.load(Ordering::Relaxed),
            secs,
            cpu,
            cpu / secs * 100.0,
            (dir_size_kb(&store)? as i64 - store_kb0 as i64) / 1024
        ));
        let restored = ed.restore(repo)?;
        agent_refresh(wts)?;
        sh.snapshot(&mut [&mut *fg], Some(&[restored]), "observacion")?;

        // Guaranteed snapshot whose scope is the 10 worktrees (10 files changed in each).
        let mut ts = Vec::new();
        let mut eds: Vec<Editor> = wts.iter().map(|p| Editor::new(p)).collect::<Res<_>>()?;
        let per: Vec<Vec<String>> = eds
            .iter_mut()
            .enumerate()
            .map(|(i, e)| e.pick_text(10, 500 + i))
            .collect();
        let g0 = integrity(repo)?;
        for i in 0..opts.iters / 2 + 3 {
            for (k, e) in eds.iter_mut().enumerate() {
                e.mutate(&wts[k], &per[k])?;
            }
            let mut scope: Vec<&mut Wt> = states.iter_mut().collect();
            let (_, t) = sh.snapshot(&mut scope, Some(&per), "previo_garantizado")?;
            if i >= 3 {
                ts.push(t);
            }
        }
        guard.check(&format!("{} scope10", v.name()), &g0, &integrity(repo)?);
        row(
            r,
            v.name(),
            "previo con ámbito de 10 worktrees (10 arch. c/u)",
            &ts,
        );
        for (k, e) in eds.iter_mut().enumerate() {
            e.restore(&wts[k])?;
        }
        agent_refresh(wts)?;
        sh.close()?;
    }
    r.line("");
    for l in cpu_lines {
        r.line(l);
    }
    Ok(())
}

fn week(
    r: &mut Report,
    opts: &Opts,
    repo: &Path,
    user_git: &Path,
    ed: &mut Editor,
    guard: &mut Guard,
) -> Res<()> {
    r.line(format!(
        "\n## Semana simulada ({} capturas de 5 archivos editados)\n",
        opts.week
    ));
    for &v in &opts.variants {
        // Store growth depends on the store format, not on detection: loose objects (`gix-hint`,
        // same layout as `cli`) and one pack per snapshot (`fi`, same as `fi-hint`).
        if v == Variant::FiHint && opts.variants.contains(&Variant::Fi)
            || v == Variant::Cli && opts.variants.contains(&Variant::GixHint)
        {
            continue;
        }
        let prof = opts.root.join(format!("prof-week-{}", v.name()));
        let _ = std::fs::remove_dir_all(&prof);
        let store = init_store(&prof)?;
        seed(user_git, &store, SEED)?;
        let base_kb = dir_size_kb(&store)?;
        let sh = Shared::open(v, &prof, &store, user_git)?;
        let mut wt = sh.new_wt("w0", repo)?;
        let g0 = integrity(repo)?;
        let t = Instant::now();
        let mut new_mb = 0.0;
        for i in 0..opts.week {
            let paths = ed.pick_text(5, i * 13 % 2000);
            ed.mutate(repo, &paths)?;
            let (_, tm) = sh.snapshot(&mut [&mut wt], Some(&[paths]), "observacion")?;
            new_mb += tm.hashed_bytes as f64 / 1_048_576.0;
        }
        let secs = t.elapsed().as_secs_f64();
        guard.check(&format!("{} week", v.name()), &g0, &integrity(repo)?);
        sh.close()?;
        ed.restore(repo)?;
        agent_refresh(std::slice::from_ref(&repo.to_path_buf()))?;
        let grown = dir_size_kb(&store)? - base_kb;
        let packs = std::fs::read_dir(store.join("objects/pack"))?.count();
        let t = Instant::now();
        let mut c = git();
        c.arg("--git-dir")
            .arg(&store)
            .args(["repack", "-d", "-q", "--geometric=2"]);
        run(c)?;
        let repack = t.elapsed().as_secs_f64();
        let grown_after = dir_size_kb(&store)? as i64 - base_kb as i64;
        r.line(format!(
            "- `{}`: {:.0} s; contenido nuevo {:.0} MiB; almacén +{} MiB ({} archivos en `objects/pack`); `repack -d --geometric=2` {:.1} s → +{} MiB",
            v.name(),
            secs,
            new_mb,
            grown / 1024,
            packs,
            repack,
            grown_after / 1024
        ));
    }
    Ok(())
}
