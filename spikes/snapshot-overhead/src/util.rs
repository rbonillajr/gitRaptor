//! Process, filesystem and statistics helpers shared by the prototype.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn err<T>(msg: impl Into<String>) -> Res<T> {
    Err(msg.into().into())
}

/// A `git` command with a scrubbed environment: no inherited `GIT_*` variables,
/// no system or global configuration, a fixed locale. Makes runs reproducible and
/// keeps the developer's own Git setup out of the measurements.
/// Real Git binary. On macOS `/usr/bin/git` is an `xcrun` shim that adds ~10 ms per spawn,
/// so the prototype resolves the real binary once (`SPIKE_GIT` overrides it).
pub fn git_bin() -> &'static str {
    static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        if let Ok(b) = std::env::var("SPIKE_GIT") {
            return b;
        }
        if cfg!(target_os = "macos")
            && let Ok(o) = Command::new("xcrun").args(["-f", "git"]).output()
            && o.status.success()
        {
            return trim(&o.stdout);
        }
        "git".to_string()
    })
}

pub fn git() -> Command {
    git_with(git_bin())
}

pub fn git_with(bin: &str) -> Command {
    let mut c = Command::new(bin);
    c.env_clear();
    for k in ["PATH", "TMPDIR", "HOME"] {
        if let Ok(v) = std::env::var(k) {
            c.env(k, v);
        }
    }
    c.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        // Read-only commands on the user's repo must never refresh/write its index.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    c
}

pub fn run(mut c: Command) -> Res<Vec<u8>> {
    let out = c.stderr(Stdio::piped()).stdout(Stdio::piped()).output()?;
    if !out.status.success() {
        return err(format!(
            "{:?} failed: {}",
            c,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out.stdout)
}

pub fn run_input(mut c: Command, input: &[u8]) -> Res<Vec<u8>> {
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("stdin");
    let data = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&data));
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_end(&mut stdout)?;
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_end(&mut stderr)?;
    writer.join().expect("writer thread")?;
    let status = child.wait()?;
    if !status.success() {
        return err(format!(
            "{:?} failed: {}",
            c,
            String::from_utf8_lossy(&stderr)
        ));
    }
    Ok(stdout)
}

pub fn trim(v: &[u8]) -> String {
    String::from_utf8_lossy(v).trim().to_string()
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub struct Timer(Instant);
impl Timer {
    pub fn start() -> Self {
        Timer(Instant::now())
    }
    /// Elapsed since the last lap, then restart.
    pub fn lap(&mut self) -> Duration {
        let now = Instant::now();
        let d = now - self.0;
        self.0 = now;
        d
    }
}

/// Stat key used by the capture's stat cache (racy-git style: any change forces a re-hash).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatKey {
    pub mtime_ns: i128,
    pub ctime_ns: i128,
    pub size: u64,
    pub ino: u64,
    pub mode: u32,
}

pub fn stat_key(p: &Path) -> std::io::Result<StatKey> {
    let m = std::fs::symlink_metadata(p)?;
    Ok(StatKey {
        mtime_ns: m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128,
        ctime_ns: m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128,
        size: m.size(),
        ino: m.ino(),
        mode: m.mode(),
    })
}

pub fn git_mode(mode: u32) -> &'static str {
    if mode & 0o170000 == 0o120000 {
        "120000"
    } else if mode & 0o111 != 0 {
        "100755"
    } else {
        "100644"
    }
}

/// Git blob id computed in-process (SHA-1 of `blob <len>\0<bytes>`).
pub fn blob_sha(bytes: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    h.digest().to_string()
}

/// Plain `fsync(2)`: on macOS it only pushes data to the drive, it does not flush the drive cache.
pub fn fsync_plain(f: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: valid open file descriptor owned by `f` for the duration of the call.
    let r = unsafe { libc::fsync(f.as_raw_fd()) };
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Full durability barrier. Rust's `sync_all` uses `F_FULLFSYNC` on Apple platforms.
pub fn fsync_full(f: &std::fs::File) -> std::io::Result<()> {
    f.sync_all()
}

pub fn dir_size_kb(p: &Path) -> Res<u64> {
    let mut c = Command::new("du");
    c.args(["-sk"]).arg(p);
    let out = trim(&run(c)?);
    Ok(out.split_whitespace().next().unwrap_or("0").parse()?)
}

/// Free bytes on the volume holding `p` (via `df -k`).
pub fn free_kb(p: &Path) -> Res<u64> {
    let mut c = Command::new("df");
    c.args(["-k"]).arg(p);
    let out = String::from_utf8(run(c)?)?;
    let line = out.lines().nth(1).ok_or("df output")?;
    Ok(line.split_whitespace().nth(3).ok_or("df col")?.parse()?)
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,

    pub max: f64,
}

/// Nearest-rank percentiles.
pub fn stats(samples: &[f64]) -> Stats {
    if samples.is_empty() {
        return Stats::default();
    }
    let mut v = samples.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pick = |p: f64| {
        let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
        v[rank.clamp(1, v.len()) - 1]
    };
    Stats {
        n: v.len(),
        p50: pick(50.0),
        p95: pick(95.0),

        max: *v.last().unwrap(),
    }
}

/// Fingerprint of a `.git` directory: every path with size, mtime and inode, plus the
/// content hash of every file that is not an immutable pack. Used to prove that the
/// prototype never writes to the user's repository.
pub fn fingerprint_git_dir(dir: &Path) -> Res<Vec<String>> {
    let mut entries = Vec::new();
    walk(dir, dir, &mut entries)?;
    entries.sort();
    Ok(entries)
}

/// Entries that differ between two fingerprints (empty = identical).
pub fn fingerprint_diff(a: &[String], b: &[String]) -> Vec<String> {
    let (sa, sb): (std::collections::BTreeSet<_>, std::collections::BTreeSet<_>) =
        (a.iter().collect(), b.iter().collect());
    sa.symmetric_difference(&sb)
        .map(|s| s.to_string())
        .collect()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Res<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        let m = std::fs::symlink_metadata(&p)?;
        let rel = p.strip_prefix(root)?.display().to_string();
        if m.is_dir() {
            out.push(format!(
                "d {rel} {}",
                m.mtime_nsec() + m.mtime() * 1_000_000_000
            ));
            walk(root, &p, out)?;
        } else {
            let content = if rel.ends_with(".pack") {
                "pack".to_string()
            } else {
                blob_sha(&std::fs::read(&p)?)
            };
            out.push(format!(
                "f {rel} {} {} {} {content}",
                m.size(),
                m.mtime() * 1_000_000_000 + m.mtime_nsec(),
                m.ino()
            ));
        }
    }
    Ok(())
}

pub fn ensure_dir(p: &Path) -> Res<PathBuf> {
    std::fs::create_dir_all(p)?;
    Ok(p.to_path_buf())
}
