//! Shared helpers: monotonic clock, stats, blob hashing, process metrics.

use std::fs::Metadata;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Monotonic nanoseconds since process start. Every stage stamp uses this clock.
pub fn now_ns() -> u64 {
    START.elapsed().as_nanos() as u64
}

pub fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

/// Nearest-rank percentile over an unsorted slice.
pub fn pct(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = ((p / 100.0) * v.len() as f64).ceil().max(1.0) as usize;
    v[rank.min(v.len()) - 1]
}

#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

pub fn summarize(values: &[f64]) -> Summary {
    if values.is_empty() {
        return Summary::default();
    }
    Summary {
        n: values.len(),
        p50: pct(values, 50.0),
        p95: pct(values, 95.0),
        p99: pct(values, 99.0),
        max: values.iter().cloned().fold(f64::MIN, f64::max),
        mean: values.iter().sum::<f64>() / values.len() as f64,
    }
}

/// Git blob id of `content` (SHA-1 of `blob <len>\0<content>`).
pub fn blob_oid(content: &[u8]) -> gix::ObjectId {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("blob {}\0", content.len()).as_bytes());
    h.update(content);
    gix::ObjectId::Sha1(h.digest().bytes())
}

pub fn hash_file(path: &Path) -> Option<gix::ObjectId> {
    std::fs::read(path).ok().map(|c| blob_oid(&c))
}

/// Cheap stat signature used by the in-memory stat cache.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FileSig {
    pub len: u64,
    pub mtime_s: i64,
    pub mtime_ns: i64,
    pub ino: u64,
}

#[cfg(unix)]
pub fn sig(m: &Metadata) -> FileSig {
    use std::os::unix::fs::MetadataExt;
    FileSig {
        len: m.len(),
        mtime_s: m.mtime(),
        mtime_ns: m.mtime_nsec(),
        ino: m.ino(),
    }
}

#[cfg(not(unix))]
pub fn sig(m: &Metadata) -> FileSig {
    let d = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default();
    FileSig {
        len: m.len(),
        mtime_s: d.as_secs() as i64,
        mtime_ns: d.subsec_nanos() as i64,
        ino: 0,
    }
}

/// User+system CPU time of this process, in milliseconds.
#[cfg(unix)]
pub fn cpu_ms() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let tv = |t: libc::timeval| t.tv_sec as f64 * 1e3 + t.tv_usec as f64 / 1e3;
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

#[cfg(not(unix))]
pub fn cpu_ms() -> f64 {
    f64::NAN
}

/// CPU time of the calling thread, in milliseconds.
#[cfg(unix)]
pub fn thread_cpu_ms() -> f64 {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as f64 * 1e3 + ts.tv_nsec as f64 / 1e6
}

#[cfg(not(unix))]
pub fn thread_cpu_ms() -> f64 {
    f64::NAN
}

/// Current resident set size in MiB (via `ps`, portable across macOS and Linux).
pub fn rss_mib() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output();
    match out {
        Ok(o) => {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<f64>()
                .unwrap_or(f64::NAN)
                / 1024.0
        }
        Err(_) => f64::NAN,
    }
}

/// Open file descriptors of this process.
pub fn open_fds() -> usize {
    for dir in ["/dev/fd", "/proc/self/fd"] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            return rd.count();
        }
    }
    0
}
