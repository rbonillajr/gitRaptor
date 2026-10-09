//! The command line of another process of the user (DS-US-GRD-018 § 5.3): read only to classify
//! a Git subcommand, never stored. Of a `git` the detector also reads whether it redirects its
//! target, from its global options and the *names* of its environment: one boolean leaves, never
//! an argument nor a variable's value.

use std::ffi::OsString;

/// Most arguments returned; a longer command line is unreadable.
pub const MAX_ARGS: usize = 4096;

/// The arguments (`argv`, program name first) of `pid`, or `None` when they cannot be read.
/// macOS only; elsewhere always `None`.
pub fn process_args(pid: u32) -> Option<Vec<OsString>> {
    #[cfg(target_os = "macos")]
    {
        parse_procargs2(&crate::ffi_procargs::procargs2(pid)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// Parses a `KERN_PROCARGS2` area: a native-endian `int` argc, the executable path, NUL padding,
/// then `argc` NUL-terminated strings. The environment that follows is never read. Anything
/// malformed is `None`.
pub fn parse_procargs2(area: &[u8]) -> Option<Vec<OsString>> {
    let (args, _env) = split_procargs2(area)?;
    Some(args.into_iter().map(os).collect())
}

/// Whether the `git` `pid` redirects its target (`-C`, `--git-dir`, `--work-tree` before the
/// subcommand, or `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR` in its environment). `None` when
/// its area cannot be read. Only this boolean leaves: never an argument nor a variable's value.
/// macOS only; elsewhere always `None`.
pub fn process_git_redirect(pid: u32) -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        parse_procargs2_git_redirect(&crate::ffi_procargs::procargs2(pid)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// [`process_git_redirect`] over a `KERN_PROCARGS2` area: the global options of the argv (after
/// `argv[0]`), then the names of the environment that follows it. Anything malformed is `None`.
pub fn parse_procargs2_git_redirect(area: &[u8]) -> Option<bool> {
    let (args, env) = split_procargs2(area)?;
    if global_options_redirect(args.get(1..).unwrap_or_default()) {
        return Some(true);
    }
    // The environment ends at the first empty string (its NUL padding, then the loader's own
    // strings); a last string cut by the end of the area still names a variable.
    let mut names = env
        .split(|b| *b == 0)
        .take_while(|s| !s.is_empty())
        .map(|kv| kv.split(|b| *b == b'=').next().unwrap_or_default());
    Some(names.any(|name| REDIRECT_ENV.contains(&name)))
}

/// The environment variables that move a `git`'s repository or worktree.
const REDIRECT_ENV: [&[u8]; 3] = [b"GIT_DIR", b"GIT_WORK_TREE", b"GIT_COMMON_DIR"];

/// The global options of `git` that take their value as the next argument (in the `=` form they
/// are one argument). `--exec-path` only takes one in the `=` form.
const GLOBAL_WITH_VALUE: [&[u8]; 7] = [
    b"-C",
    b"-c",
    b"--git-dir",
    b"--work-tree",
    b"--namespace",
    b"--config-env",
    b"--super-prefix",
];

/// Whether the global options (the arguments before the subcommand, program name excluded)
/// redirect the target: `-C`, `--git-dir` or `--work-tree`, separate or with `=`. The first
/// argument that is not an option is the subcommand; what follows it is never read.
fn global_options_redirect(args: &[&[u8]]) -> bool {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if !arg.starts_with(b"-") || *arg == b"--" {
            return false;
        }
        let name = arg.split(|b| *b == b'=').next().unwrap_or_default();
        if matches!(name, b"-C" | b"--git-dir" | b"--work-tree") {
            return true;
        }
        let has_value = name.len() < arg.len();
        if !has_value && GLOBAL_WITH_VALUE.contains(arg) {
            // Its value, which may itself start with `-`.
            args.next();
        }
    }
    false
}

/// Splits a `KERN_PROCARGS2` area into its `argc` arguments and what follows them (the
/// environment). Anything malformed is `None`.
fn split_procargs2(area: &[u8]) -> Option<(Vec<&[u8]>, &[u8])> {
    let (count, rest) = area.split_first_chunk::<4>()?;
    let argc = usize::try_from(i32::from_ne_bytes(*count)).ok()?;
    if argc == 0 || argc > MAX_ARGS {
        return None;
    }
    // The executable path, then its NUL padding.
    let end = rest.iter().position(|b| *b == 0)?;
    let mut rest = &rest[end..];
    while let [0, tail @ ..] = rest {
        rest = tail;
    }
    let mut out = Vec::with_capacity(argc);
    for _ in 0..argc {
        let end = rest.iter().position(|b| *b == 0)?;
        out.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    Some((out, rest))
}

/// A process as the detector's table needs it: identity `(pid, start_us)` and parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcBrief {
    pub pid: u32,
    pub ppid: u32,
    /// Start time, microseconds since the epoch (the `pbi_start_tv*` of `proc_pidinfo`).
    pub start_us: u64,
}

/// `sizeof(struct kinfo_proc)`, the same on arm64 and x86_64 (both LP64).
pub(crate) const KINFO_PROC_SIZE: usize = 648;
/// Offsets in `struct kinfo_proc` (`<sys/sysctl.h>`), checked with `offsetof` on both
/// architectures: `kp_proc.p_un.__p_starttime` (`tv_sec` i64, `tv_usec` i32), `kp_proc.p_stat`,
/// `kp_proc.p_pid` and `kp_eproc.e_ppid`.
const START_SEC: usize = 0;
const START_USEC: usize = 8;
const STAT: usize = 36;
const PID: usize = 40;
const PPID: usize = 560;
/// `p_stat` of a zombie: it ended and waits for its parent, so it is not listed.
const SZOMB: u8 = 5;

/// Every live process whose effective uid is `uid` (as `proc_listpids(PROC_UID_ONLY)` filters),
/// with one `sysctl(KERN_PROC_UID)` instead of one `proc_pidinfo` per process (RES-01: the
/// detector reads this table every second). `None` when it cannot be read, and outside macOS on
/// arm64 or x86_64, the two layouts the offsets were checked on: callers fall back to
/// `proc_pidinfo`.
pub fn user_processes(uid: u32) -> Option<Vec<ProcBrief>> {
    #[cfg(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ))]
    {
        parse_kinfo(&crate::ffi_kinfo::kinfo_by_uid(uid)?)
    }
    #[cfg(not(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    )))]
    {
        let _ = uid;
        None
    }
}

/// Parses an array of `struct kinfo_proc` records, skipping zombies. `None` when its length is
/// not a whole number of records.
pub fn parse_kinfo(table: &[u8]) -> Option<Vec<ProcBrief>> {
    if !table.len().is_multiple_of(KINFO_PROC_SIZE) {
        return None;
    }
    let i32_at = |r: &[u8], at: usize| {
        r.get(at..at + 4)
            .map(|b| i32::from_ne_bytes(b.try_into().unwrap()))
    };
    let i64_at = |r: &[u8], at: usize| {
        r.get(at..at + 8)
            .map(|b| i64::from_ne_bytes(b.try_into().unwrap()))
    };
    table
        .as_chunks::<KINFO_PROC_SIZE>()
        .0
        .iter()
        .filter(|r| r[STAT] != SZOMB)
        .map(|r| {
            let sec = u64::try_from(i64_at(r, START_SEC)?).ok()?;
            let usec = u64::try_from(i32_at(r, START_USEC)?).ok()?;
            Some(ProcBrief {
                pid: u32::try_from(i32_at(r, PID)?).ok()?,
                ppid: u32::try_from(i32_at(r, PPID)?).ok()?,
                start_us: sec.saturating_mul(1_000_000).saturating_add(usec),
            })
        })
        .collect()
}

#[cfg(unix)]
fn os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).to_owned()
}

#[cfg(not(unix))]
fn os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(test)]
#[path = "process_redirect_tests.rs"]
mod redirect_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn area(argc: i32, body: &[u8]) -> Vec<u8> {
        let mut v = argc.to_ne_bytes().to_vec();
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn reads_argv_and_never_the_environment() {
        let a = area(
            3,
            b"/usr/bin/git\0\0\0\0git\0commit\0--no-verify\0SECRET=1\0",
        );
        let args = parse_procargs2(&a).unwrap();
        assert_eq!(args, ["git", "commit", "--no-verify"]);
    }

    #[test]
    fn malformed_areas_are_unreadable() {
        assert_eq!(parse_procargs2(&[]), None);
        assert_eq!(parse_procargs2(&area(0, b"/x\0x\0")), None);
        assert_eq!(parse_procargs2(&area(-1, b"/x\0x\0")), None);
        // Fewer strings than argc.
        assert_eq!(parse_procargs2(&area(3, b"/x\0\0git\0commit")), None);
        // No terminated executable path.
        assert_eq!(parse_procargs2(&area(1, b"/x")), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_this_process() {
        let args = process_args(std::process::id()).unwrap();
        let me: Vec<OsString> = std::env::args_os().collect();
        assert_eq!(args, me);
    }

    fn record(pid: i32, ppid: i32, sec: i64, usec: i32, stat: u8) -> Vec<u8> {
        let mut r = vec![0u8; KINFO_PROC_SIZE];
        r[START_SEC..START_SEC + 8].copy_from_slice(&sec.to_ne_bytes());
        r[START_USEC..START_USEC + 4].copy_from_slice(&usec.to_ne_bytes());
        r[STAT] = stat;
        r[PID..PID + 4].copy_from_slice(&pid.to_ne_bytes());
        r[PPID..PPID + 4].copy_from_slice(&ppid.to_ne_bytes());
        r
    }

    #[test]
    fn parses_records_and_skips_zombies() {
        let mut t = record(42, 1, 1_700_000_000, 250_000, 2);
        t.extend(record(43, 42, 1_700_000_001, 0, SZOMB));
        t.extend(record(44, 42, 1_700_000_002, 7, 3));
        assert_eq!(
            parse_kinfo(&t).unwrap(),
            [
                ProcBrief {
                    pid: 42,
                    ppid: 1,
                    start_us: 1_700_000_000_250_000
                },
                ProcBrief {
                    pid: 44,
                    ppid: 42,
                    start_us: 1_700_000_002_000_007
                },
            ]
        );
        assert_eq!(parse_kinfo(&[]).unwrap(), []);
        // A partial record, or a negative pid, is unreadable.
        assert_eq!(parse_kinfo(&t[..KINFO_PROC_SIZE + 1]), None);
        assert_eq!(parse_kinfo(&record(-1, 1, 0, 0, 2)), None);
    }

    /// The table agrees with `ps`, which reads the same records, for this process and a child.
    #[cfg(target_os = "macos")]
    #[test]
    fn lists_this_process_and_a_child() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let uid = String::from_utf8(
            std::process::Command::new("/usr/bin/id")
                .arg("-u")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .parse()
        .unwrap();
        let table = user_processes(uid).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let me = table.iter().find(|p| p.pid == std::process::id()).unwrap();
        let kid = table.iter().find(|p| p.pid == child.id()).unwrap();
        assert_eq!(kid.ppid, std::process::id());
        assert!(me.start_us <= kid.start_us);
        // Within a minute of now: the start is read from the right offset.
        let now_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        assert!(now_us - kid.start_us < 60_000_000, "{kid:?}");
        // Another user's process (launchd, uid 0) is not listed.
        assert!(table.iter().all(|p| p.pid != 1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_gone_pid_is_unreadable() {
        assert_eq!(process_args(u32::MAX), None);
        assert_eq!(process_args(0x7fff_fff0), None);
    }
}
