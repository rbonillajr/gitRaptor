//! Synthetic repository generator. Everything lives under a temporary directory;
//! the prototype never touches a real repository (NFR-01).

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, serde::Serialize)]
pub struct GenCfg {
    pub commits: usize,
    pub dirs: usize,
    pub files_per_dir: usize,
    pub worktrees: usize,
    pub commit_graph: bool,
}

pub struct Synthetic {
    pub base: PathBuf,
    pub main: PathBuf,
    /// Linked worktree names (`wt-01` ..), siblings of `main`.
    pub linked: Vec<String>,
}

impl Synthetic {
    pub fn wt_root(&self, name: &str) -> PathBuf {
        if name == "main" {
            self.main.clone()
        } else {
            self.base.join(name)
        }
    }
    pub fn all_names(&self) -> Vec<String> {
        std::iter::once("main".to_string())
            .chain(self.linked.iter().cloned())
            .collect()
    }
}

/// Git invocation isolated from the user's global and system config (no hooks, no signing).
pub const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

pub fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.invalid")
        .env("GIT_COMMITTER_NAME", "bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.invalid")
        .args([
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "core.fsmonitor=false",
        ]);
    c
}

pub fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd.stderr(Stdio::piped()).output().context("spawn git")?;
    if !out.status.success() {
        bail!("{:?} failed: {}", cmd, String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn file_path(d: usize, f: usize) -> String {
    format!("src/d{d:03}/f{f:03}.txt")
}

pub fn generate(base: &Path, cfg: &GenCfg) -> Result<Synthetic> {
    let main = base.join("main");
    std::fs::create_dir_all(&main)?;
    run(git(&main).args(["init", "-q", "-b", "main"]))?;

    let mut child = git(&main)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .context("spawn fast-import")?;
    {
        let mut w = std::io::BufWriter::new(child.stdin.take().unwrap());
        let total = cfg.dirs * cfg.files_per_dir;
        let ts = 1_700_000_000u64;
        // Initial commit with the whole tree.
        writeln!(
            w,
            "commit refs/heads/main\nmark :1\ncommitter bench <bench@example.invalid> {ts} +0000\ndata 4\ninit"
        )?;
        let gi = b"target/\nnode_modules/\n";
        writeln!(w, "M 100644 inline .gitignore\ndata {}", gi.len())?;
        w.write_all(gi)?;
        writeln!(w)?;
        for d in 0..cfg.dirs {
            for f in 0..cfg.files_per_dir {
                let body = format!("file {d}/{f}\nline 2\nline 3\n");
                writeln!(
                    w,
                    "M 100644 inline {}\ndata {}",
                    file_path(d, f),
                    body.len()
                )?;
                w.write_all(body.as_bytes())?;
                writeln!(w)?;
            }
        }
        // History: each commit touches one file round-robin.
        for i in 1..cfg.commits {
            let idx = i % total;
            let (d, f) = (idx / cfg.files_per_dir, idx % cfg.files_per_dir);
            let body = format!("file {d}/{f}\nrev {i}\n");
            let msg = format!("c{i}");
            writeln!(
                w,
                "commit refs/heads/main\ncommitter bench <bench@example.invalid> {} +0000\ndata {}\n{msg}",
                ts + i as u64,
                msg.len()
            )?;
            writeln!(
                w,
                "M 100644 inline {}\ndata {}",
                file_path(d, f),
                body.len()
            )?;
            w.write_all(body.as_bytes())?;
            writeln!(w)?;
        }
        w.flush()?;
    }
    if !child.wait()?.success() {
        bail!("fast-import failed");
    }
    run(git(&main).args(["reset", "-q", "--hard", "main"]))?;
    if cfg.commit_graph {
        run(git(&main).args(["commit-graph", "write", "--reachable"]))?;
    }
    let mut linked = Vec::new();
    for i in 1..cfg.worktrees {
        let name = format!("wt-{i:02}");
        run(git(&main).args([
            "worktree",
            "add",
            "-q",
            "-b",
            &name,
            base.join(&name).to_str().unwrap(),
            "main",
        ]))?;
        // Branch used by the checkout scenario: 50 commits behind main.
        run(git(&main).args(["branch", &format!("alt-{i:02}"), "main~50"]))?;
        linked.push(name);
    }
    run(git(&main).args(["branch", "alt-main", "main~50"]))?;
    Ok(Synthetic {
        base: base.to_path_buf(),
        main,
        linked,
    })
}

/// Read-only snapshot of every file and directory under `base`: path, size, mtime, inode, mode.
/// Content of the Git metadata files (not objects) is hashed too.
pub fn fingerprint(base: &Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for e in walk_all(base) {
        let Ok(m) = std::fs::symlink_metadata(&e) else {
            continue;
        };
        let s = crate::util::sig(&m);
        let rel = e.strip_prefix(base).unwrap().to_string_lossy().to_string();
        let mut v = format!(
            "{}:{}:{}.{}:{}",
            m.is_dir(),
            s.len,
            s.mtime_s,
            s.mtime_ns,
            s.ino
        );
        if m.is_file() && rel.contains(".git") && !rel.contains("objects") {
            v.push_str(&format!(
                ":{}",
                crate::util::hash_file(&e)
                    .map(|o| o.to_string())
                    .unwrap_or_default()
            ));
        }
        out.insert(rel, v);
    }
    out
}

fn walk_all(dir: &Path) -> Vec<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    let mut out = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out
}
