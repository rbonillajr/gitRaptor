//! Deterministic generator of synthetic reference repositories.
//!
//! Same profile + same seed => same commit ids (fixed authors, dates and content), so the
//! reference repo is reproducible on any machine without downloading anything. History is
//! streamed into `git fast-import`.

use crate::util::{Res, err, git, run, run_input, trim};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::Stdio;

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        let u1 = self.unit().max(1e-12);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let v = self.next().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: &'static str,
    pub files: usize,
    pub wt_bytes: u64,
    pub commits: usize,
    pub ignored_files: usize,
}

pub fn profile(name: &str) -> Res<Profile> {
    Ok(match name {
        // Smoke profile, only to test the harness quickly.
        "S" => Profile {
            name: "S",
            files: 2_000,
            wt_bytes: 50 << 20,
            commits: 5_000,
            ignored_files: 500,
        },
        // ~p50 of the public sample (alternative reference, for comparison).
        "P50" => Profile {
            name: "P50",
            files: 6_000,
            wt_bytes: 50 << 20,
            commits: 20_000,
            ignored_files: 2_000,
        },
        // Proposed reference "medium repo" (ADR-TMC-006 § 3 hypothesis).
        "M" => Profile {
            name: "M",
            files: 10_000,
            wt_bytes: 300 << 20,
            commits: 50_000,
            ignored_files: 3_000,
        },
        // Larger repo, outside the reference (US-TMC-020 scenario 3).
        "L" => Profile {
            name: "L",
            files: 40_000,
            wt_bytes: 1 << 30,
            commits: 150_000,
            ignored_files: 10_000,
        },
        _ => return err(format!("unknown profile {name} (S|P50|M|L)")),
    })
}

const WORDS: &[&str] = &[
    "fn", "let", "mut", "self", "impl", "struct", "pub", "use", "return", "match", "if", "else",
    "for", "while", "const", "value", "result", "error", "config", "state", "index", "tree",
    "commit", "branch", "worktree", "agent", "snapshot", "store", "path", "file", "buffer",
    "count", "async", "await", "into", "from", "map", "filter", "collect", "string", "vec",
    "option", "some", "none", "ok", "err", "=>", "{", "}", "(", ")", ";", "=", "+", "-", "*", "&",
    "::", ".", ",", "<", ">", "0", "1", "42", "\"text\"",
];

fn text(rng: &mut Rng, target: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(target + 80);
    while out.len() < target {
        let indent = rng.below(4) * 4;
        out.extend(std::iter::repeat_n(b' ', indent));
        let words = 3 + rng.below(10);
        for i in 0..words {
            if i > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(WORDS[rng.below(WORDS.len())].as_bytes());
        }
        out.push(b'\n');
    }
    out
}

/// Rewrite a random region of a text file (a typical small edit).
pub fn edit_text(rng: &mut Rng, content: &mut Vec<u8>) {
    if content.is_empty() {
        *content = text(rng, 200);
        return;
    }
    let start = rng.below(content.len());
    let start = content[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let span = 40 + rng.below(400);
    let end = (start + span).min(content.len());
    let end = content[end..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(content.len(), |p| end + p + 1);
    let replacement = text(rng, (end - start).max(40));
    content.splice(start..end, replacement);
}

pub struct FileSpec {
    pub path: String,
    pub binary: bool,
}

pub fn layout(p: &Profile, rng: &mut Rng) -> Vec<(FileSpec, Vec<u8>)> {
    let dirs = (p.files / 25).max(1);
    let n_bin = p.files * 3 / 100;
    let mut out = Vec::with_capacity(p.files);
    let mut text_total = 0u64;
    for i in 0..p.files - n_bin {
        let d = i % dirs;
        let ext = ["rs", "ts", "md", "json", "toml", "py"][rng.below(6)];
        let path = format!("src/m{}/p{}/d{}/f{}.{}", d % 8, (d / 8) % 8, d / 64, i, ext);
        // Log-normal sizes, median ~4 KiB.
        let size = ((4096f64.ln() + rng.normal()).exp() as usize).clamp(200, 256 * 1024);
        let content = text(rng, size);
        text_total += content.len() as u64;
        out.push((
            FileSpec {
                path,
                binary: false,
            },
            content,
        ));
    }
    let bin_budget = p.wt_bytes.saturating_sub(text_total);
    let per = (bin_budget / n_bin.max(1) as u64) as usize;
    for i in 0..n_bin {
        let size = (per as f64 * (0.5 + rng.unit())) as usize;
        let mut content = vec![0u8; size.max(1)];
        rng.fill(&mut content);
        let ext = ["png", "bin", "woff2"][rng.below(3)];
        out.push((
            FileSpec {
                path: format!("assets/a{}/b{}.{}", i % 20, i, ext),
                binary: true,
            },
            content,
        ));
    }
    out
}

pub struct Generated {
    pub head: String,
    pub tracked_files: usize,
    pub wt_bytes: u64,
    pub commits: usize,
    pub pack_kb: u64,
    pub secs: f64,
}

pub fn generate(p: &Profile, repo: &Path, seed: u64) -> Res<Generated> {
    let t0 = std::time::Instant::now();
    if repo.exists() {
        return err(format!("{} already exists", repo.display()));
    }
    std::fs::create_dir_all(repo)?;
    let mut c = git();
    c.args(["init", "-q", "-b", "main"]).arg(repo);
    run(c)?;

    let mut rng = Rng::new(seed);
    let mut files = layout(p, &mut rng);
    let n = files.len();
    let text_idx: Vec<usize> = (0..n).filter(|&i| !files[i].0.binary).collect();
    let bin_idx: Vec<usize> = (0..n).filter(|&i| files[i].0.binary).collect();

    let mut child = git()
        .current_dir(repo)
        .args(["fast-import", "--quiet", "--depth=50"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;
    {
        let mut w = BufWriter::with_capacity(1 << 20, child.stdin.take().unwrap());
        let gitignore = b"node_modules/\ntarget/\n.env\n*.log\n";
        for c in 1..=p.commits {
            let author = rng.below(12);
            let ts = 1_600_000_000u64 + c as u64 * 600;
            let msg = format!("change {c}\n");
            write!(
                w,
                "commit refs/heads/main\nauthor Dev{author} <dev{author}@example.com> {ts} +0000\ncommitter Dev{author} <dev{author}@example.com> {ts} +0000\ndata {}\n{msg}",
                msg.len()
            )?;
            let mut touched: Vec<usize> = Vec::new();
            if c == 1 {
                write!(w, "M 100644 inline .gitignore\ndata {}\n", gitignore.len())?;
                w.write_all(gitignore)?;
                touched.extend(0..n);
            } else {
                for _ in 0..1 + rng.below(4) {
                    // 80/20 hot set
                    let pool = if rng.unit() < 0.8 {
                        text_idx.len() / 5
                    } else {
                        text_idx.len()
                    };
                    let i = text_idx[rng.below(pool.max(1))];
                    edit_text(&mut rng, &mut files[i].1);
                    touched.push(i);
                }
                if c % 1000 == 0 && !bin_idx.is_empty() {
                    let i = bin_idx[rng.below(bin_idx.len())];
                    let len = files[i].1.len();
                    let s = rng.below(len);
                    let e = (s + len / 10).min(len);
                    rng.fill(&mut files[i].1[s..e]);
                    touched.push(i);
                }
            }
            touched.sort_unstable();
            touched.dedup();
            for i in touched {
                let (spec, content) = &files[i];
                write!(w, "M 100644 inline {}\ndata {}\n", spec.path, content.len())?;
                w.write_all(content)?;
                w.write_all(b"\n")?;
            }
            w.write_all(b"\n")?;
        }
        w.flush()?;
    }
    if !child.wait()?.success() {
        return err("fast-import failed");
    }
    let mut c = git();
    c.current_dir(repo).args(["reset", "--hard", "-q", "main"]);
    run(c)?;

    // Ignored content (never captured): node_modules-like tree and a .env.
    for i in 0..p.ignored_files {
        let d = repo.join(format!("node_modules/pkg{}/lib", i % 200));
        std::fs::create_dir_all(&d)?;
        let mut content = vec![0u8; 1024];
        rng.fill(&mut content);
        std::fs::write(d.join(format!("m{i}.js")), &content)?;
    }
    std::fs::write(repo.join(".env"), b"SECRET=do-not-capture\n")?;

    let head = trim(&run({
        let mut c = git();
        c.current_dir(repo).args(["rev-parse", "HEAD"]);
        c
    })?);
    let tracked = run({
        let mut c = git();
        c.current_dir(repo).args(["ls-files", "-z"]);
        c
    })?
    .split(|&b| b == 0)
    .filter(|s| !s.is_empty())
    .count();
    let commits: usize = trim(&run({
        let mut c = git();
        c.current_dir(repo).args(["rev-list", "--count", "HEAD"]);
        c
    })?)
    .parse()?;
    let wt_bytes = files.iter().map(|f| f.1.len() as u64).sum::<u64>();
    let cov = String::from_utf8(run_input(
        {
            let mut c = git();
            c.current_dir(repo).args(["count-objects", "-v"]);
            c
        },
        b"",
    )?)?;
    let pack_kb = cov
        .lines()
        .find_map(|l| l.strip_prefix("size-pack: "))
        .unwrap_or("0")
        .parse()?;
    std::fs::write(
        repo.join(".git/spike-generated"),
        format!("{} {head}\n", p.name),
    )?;
    Ok(Generated {
        head,
        tracked_files: tracked,
        wt_bytes,
        commits,
        pack_kb,
        secs: t0.elapsed().as_secs_f64(),
    })
}
