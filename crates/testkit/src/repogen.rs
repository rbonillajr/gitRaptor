//! Deterministic generator of the reference repositories of SPIKE-TMC-001 (D-TMC-21), ported from
//! `spikes/snapshot-overhead/src/repogen.rs` for the snapshot bench (ADR-TMC-006 § 3).
//!
//! Same profile and seed give the same commits (fixed authors, dates and content), so the
//! reference repo is rebuilt on any machine without downloading anything. The history is
//! streamed into `git fast-import`. Profile `M` is the approved "medium repo".

use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// splitmix64: small, fast and stable across platforms.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }

    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn normal(&mut self) -> f64 {
        let u1 = self.unit().max(1e-12);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
}

/// Size of a generated repository.
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: &'static str,
    pub files: usize,
    pub wt_bytes: u64,
    pub commits: usize,
    pub ignored_files: usize,
}

/// `S` (smoke), `P50` (median of the public sample), `M` (reference, D-TMC-21) and `L`
/// (larger than the reference, US-TMC-020 escenario 3).
pub fn profile(name: &str) -> Option<Profile> {
    Some(match name {
        "S" => Profile {
            name: "S",
            files: 2_000,
            wt_bytes: 50 << 20,
            commits: 5_000,
            ignored_files: 500,
        },
        "P50" => Profile {
            name: "P50",
            files: 6_000,
            wt_bytes: 50 << 20,
            commits: 20_000,
            ignored_files: 2_000,
        },
        "M" => Profile {
            name: "M",
            files: 10_000,
            wt_bytes: 300 << 20,
            commits: 50_000,
            ignored_files: 3_000,
        },
        "L" => Profile {
            name: "L",
            files: 40_000,
            wt_bytes: 1 << 30,
            commits: 150_000,
            ignored_files: 10_000,
        },
        _ => return None,
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

/// Rewrites a random region of a text file (a typical small edit).
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

struct FileSpec {
    path: String,
    binary: bool,
}

fn layout(p: &Profile, rng: &mut Rng) -> Vec<(FileSpec, Vec<u8>)> {
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

/// What [`generate`] built.
#[derive(Debug, Clone)]
pub struct Generated {
    pub head: String,
    pub tracked_files: usize,
    pub wt_bytes: u64,
    pub commits: usize,
    pub secs: f64,
}

/// A Git command with no system or global configuration.
pub fn git_command(git: &Path, dir: &Path) -> Command {
    let mut c = Command::new(git);
    c.current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_AUTHOR_NAME", "gen")
        .env("GIT_AUTHOR_EMAIL", "gen@example.com")
        .env("GIT_COMMITTER_NAME", "gen")
        .env("GIT_COMMITTER_EMAIL", "gen@example.com");
    c
}

fn run(mut c: Command) -> Result<Vec<u8>, String> {
    let out = c.output().map_err(|e| format!("{c:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{c:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

fn trim(v: &[u8]) -> String {
    String::from_utf8_lossy(v).trim().to_owned()
}

/// Generates profile `p` into the new folder `repo` with `seed`.
pub fn generate(git: &Path, p: &Profile, repo: &Path, seed: u64) -> Result<Generated, String> {
    let t0 = std::time::Instant::now();
    if repo.exists() {
        return Err(format!("{} already exists", repo.display()));
    }
    std::fs::create_dir_all(repo).map_err(|e| e.to_string())?;
    let mut c = git_command(git, repo);
    c.args(["init", "-q", "-b", "main"]);
    run(c)?;

    let mut rng = Rng::new(seed);
    let mut files = layout(p, &mut rng);
    let n = files.len();
    let text_idx: Vec<usize> = (0..n).filter(|&i| !files[i].0.binary).collect();
    let bin_idx: Vec<usize> = (0..n).filter(|&i| files[i].0.binary).collect();

    let mut child = git_command(git, repo)
        .args(["fast-import", "--quiet", "--depth=50"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut w = BufWriter::with_capacity(1 << 20, stdin);
        let mut emit = || -> std::io::Result<()> {
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
            w.flush()
        };
        emit().map_err(|e| e.to_string())?;
    }
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err("fast-import failed".into());
    }
    let mut c = git_command(git, repo);
    c.args(["reset", "--hard", "-q", "main"]);
    run(c)?;

    // Ignored content (never captured): a node_modules-like tree and a .env.
    for i in 0..p.ignored_files {
        let d = repo.join(format!("node_modules/pkg{}/lib", i % 200));
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let mut content = vec![0u8; 1024];
        rng.fill(&mut content);
        std::fs::write(d.join(format!("m{i}.js")), &content).map_err(|e| e.to_string())?;
    }
    std::fs::write(repo.join(".env"), b"SECRET=do-not-capture\n").map_err(|e| e.to_string())?;

    let head = {
        let mut c = git_command(git, repo);
        c.args(["rev-parse", "HEAD"]);
        trim(&run(c)?)
    };
    let tracked_files = {
        let mut c = git_command(git, repo);
        c.args(["ls-files", "-z"]);
        run(c)?.split(|&b| b == 0).filter(|s| !s.is_empty()).count()
    };
    let commits = {
        let mut c = git_command(git, repo);
        c.args(["rev-list", "--count", "HEAD"]);
        trim(&run(c)?).parse().map_err(|e| format!("{e}"))?
    };
    Ok(Generated {
        head,
        tracked_files,
        wt_bytes: files.iter().map(|f| f.1.len() as u64).sum(),
        commits,
        secs: t0.elapsed().as_secs_f64(),
    })
}
