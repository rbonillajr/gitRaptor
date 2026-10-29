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

/// The arguments of `pid` (program name first) and whether it redirects its target
/// ([`process_git_redirect`]), from one read of its area: what S3 reads of a `git`. `None` when
/// the area cannot be read. macOS only; elsewhere always `None`.
pub fn process_git_args_and_redirect(pid: u32) -> Option<(Vec<OsString>, bool)> {
    #[cfg(target_os = "macos")]
    {
        parse_procargs2_git_args_and_redirect(&crate::ffi_procargs::procargs2(pid)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// [`process_git_args_and_redirect`] over a `KERN_PROCARGS2` area: its arguments and
/// [`parse_procargs2_git_redirect`] of the same area. Anything malformed is `None`.
pub fn parse_procargs2_git_args_and_redirect(area: &[u8]) -> Option<(Vec<OsString>, bool)> {
    let (args, env) = split_procargs2(area)?;
    let redirect = git_redirects(
        args.get(1..).unwrap_or_default().iter().copied(),
        env.split(|b| *b == 0),
    );
    Some((args.into_iter().map(os).collect(), redirect))
}

/// The arguments of a process (program name first) and some of its environment variables, by
/// name and value.
pub type ArgsAndEnv = (Vec<OsString>, Vec<(OsString, OsString)>);

/// The arguments (program name first) of `pid` and the values of the environment variables
/// named in `names`, to resolve where a `git` points. Every other variable is skipped unread
/// into the result. `None` when the area cannot be read. macOS only; elsewhere always `None`.
pub fn process_git_target(pid: u32, names: &[&str]) -> Option<ArgsAndEnv> {
    #[cfg(target_os = "macos")]
    {
        parse_procargs2_git_target(&crate::ffi_procargs::procargs2(pid)?, names)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, names);
        None
    }
}

/// [`process_git_target`] over a `KERN_PROCARGS2` area: its arguments, and of the strings that
/// follow them only the `name=value` ones whose name is in `names`. Anything malformed is `None`.
pub fn parse_procargs2_git_target(area: &[u8], names: &[&str]) -> Option<ArgsAndEnv> {
    let (args, env) = split_procargs2(area)?;
    let wanted = env
        .split(|b| *b == 0)
        .filter_map(|kv| {
            let eq = kv.iter().position(|b| *b == b'=')?;
            let (name, value) = (&kv[..eq], &kv[eq + 1..]);
            names
                .iter()
                .any(|n| n.as_bytes() == name)
                .then(|| (os(name), os(value)))
        })
        .collect();
    Some((args.into_iter().map(os).collect(), wanted))
}

/// [`process_git_redirect`] over a `KERN_PROCARGS2` area: [`git_redirects`] over the argv (after
/// `argv[0]`) and every non-empty string that follows it, the environment and the loader's own
/// `key=value` strings alike. Anything malformed is `None`.
pub fn parse_procargs2_git_redirect(area: &[u8]) -> Option<bool> {
    let (args, env) = split_procargs2(area)?;
    let args = args.get(1..).unwrap_or_default().iter().copied();
    // The whole tail, not up to the first empty string: an empty entry must
    // not hide a `GIT_DIR` after it. A last string cut by the end of the area
    // still names a variable.
    Some(git_redirects(args, env.split(|b| *b == 0)))
}

/// Whether a `git` with these arguments (program name excluded) and this environment redirects
/// its target: `-C`, `--git-dir` or `--work-tree` among its global options, or `GIT_DIR`,
/// `GIT_WORK_TREE` or `GIT_COMMON_DIR` in its environment.
///
/// `env` holds names or `NAME=value` entries: only what goes before the first `=` is compared,
/// and empty entries are skipped. Fails closed: a global option this parser does not know, or a
/// `--` before the subcommand, counts as a redirect. The first argument that is not an option is
/// the subcommand and nothing after it is read, so `git log -C` (copy detection) does not
/// redirect. Only the boolean leaves: never an argument nor a variable's value (SEC-04).
pub fn git_redirects<'a, 'b>(
    args: impl IntoIterator<Item = &'a [u8]>,
    env: impl IntoIterator<Item = &'b [u8]>,
) -> bool {
    global_options_redirect(args.into_iter())
        || env
            .into_iter()
            .filter(|entry| !entry.is_empty())
            .any(|entry| REDIRECT_ENV.contains(&before_equals(entry)))
}

/// The environment variables that move a `git`'s repository or worktree.
const REDIRECT_ENV: [&[u8]; 3] = [b"GIT_DIR", b"GIT_WORK_TREE", b"GIT_COMMON_DIR"];

/// The global options that move a `git`'s repository or worktree. `-C` and `--work-tree` change
/// directory, `--git-dir` does not, but all three are treated alike.
const REDIRECT_OPTIONS: [&[u8]; 3] = [b"-C", b"--git-dir", b"--work-tree"];

/// The global options of `git` (`handle_options()` in `git.c`) whose value is the next argument;
/// the long ones also take it after `=`, as one argument. `--super-prefix` is gone from current
/// Git and stays for older ones.
const GLOBAL_WITH_VALUE: [&[u8]; 9] = [
    b"-C",
    b"-c",
    b"--git-dir",
    b"--work-tree",
    b"--namespace",
    b"--config-env",
    b"--attr-source",
    b"--shallow-file",
    b"--super-prefix",
];

/// The global options of `git` that take no value.
const GLOBAL_FLAGS: [&[u8]; 21] = [
    b"-p",
    b"-P",
    b"--paginate",
    b"--no-pager",
    b"--bare",
    b"--no-replace-objects",
    b"--no-lazy-fetch",
    b"--no-optional-locks",
    b"--no-advice",
    b"--literal-pathspecs",
    b"--glob-pathspecs",
    b"--noglob-pathspecs",
    b"--icase-pathspecs",
    b"--exec-path",
    b"--html-path",
    b"--man-path",
    b"--info-path",
    b"--version",
    b"-v",
    b"--help",
    b"-h",
];

/// The global options of `git` that take a value only after `=`.
const GLOBAL_EQUALS_ONLY: [&[u8]; 2] = [b"--list-cmds", b"--exec-path"];

/// What goes before the first `=` of `entry`, or all of it.
fn before_equals(entry: &[u8]) -> &[u8] {
    entry.split(|b| *b == b'=').next().unwrap_or_default()
}

/// What the global options of a `git` (the arguments before the subcommand, program name
/// excluded) say.
struct GlobalOptions<'a> {
    /// One of them is a [`REDIRECT_OPTIONS`].
    redirect: bool,
    /// One of them is an option this parser does not know (`--` included): what follows it
    /// cannot be told apart.
    unknown: bool,
    /// The first argument that is not an option, unless an unknown option came first.
    subcommand: Option<&'a [u8]>,
}

fn global_options<'a>(mut args: impl Iterator<Item = &'a [u8]>) -> GlobalOptions<'a> {
    let mut redirect = false;
    while let Some(arg) = args.next() {
        if !arg.starts_with(b"-") {
            // The subcommand: what follows is its own.
            return GlobalOptions {
                redirect,
                unknown: false,
                subcommand: Some(arg),
            };
        }
        let name = before_equals(arg);
        redirect |= REDIRECT_OPTIONS.contains(&name);
        let known = if name.len() < arg.len() {
            // Only the long options take their value after `=`.
            name.starts_with(b"--")
                && (GLOBAL_WITH_VALUE.contains(&name) || GLOBAL_EQUALS_ONLY.contains(&name))
        } else if GLOBAL_WITH_VALUE.contains(&name) {
            // Its value, which may itself start with `-`.
            args.next();
            true
        } else {
            GLOBAL_FLAGS.contains(&name)
        };
        if !known {
            return GlobalOptions {
                redirect,
                unknown: true,
                subcommand: None,
            };
        }
    }
    GlobalOptions {
        redirect,
        unknown: false,
        subcommand: None,
    }
}

/// Whether the global options (the arguments before the subcommand, program name excluded)
/// redirect the target, or cannot be told apart from options that might: an unknown option,
/// `--` included, counts.
fn global_options_redirect<'a>(args: impl Iterator<Item = &'a [u8]>) -> bool {
    let options = global_options(args);
    options.redirect || options.unknown
}

/// The subcommands that can move a ref or `HEAD`, the only ones whose `git` can make the Git
/// event of a session (DS-US-GRP-008 D4). `fetch` is not one: it only moves `refs/remotes/*`,
/// which is no event. `bisect` is: it moves `HEAD`.
const REF_WRITERS: [&[u8]; 18] = [
    b"commit",
    b"merge",
    b"rebase",
    b"cherry-pick",
    b"revert",
    b"am",
    b"pull",
    b"reset",
    b"checkout",
    b"switch",
    b"bisect",
    b"branch",
    b"update-ref",
    b"symbolic-ref",
    b"stash",
    b"tag",
    b"push",
    b"worktree",
];

/// Whether a `git` with these arguments (program name excluded) runs a subcommand that can move
/// a ref or `HEAD` ([`REF_WRITERS`]). Fails closed: an unknown subcommand (an alias included), an
/// unknown global option or `--` before it, or no subcommand, is `false`. Only the boolean leaves
/// (SEC-04).
pub fn git_subcommand_writes<'a>(args: impl IntoIterator<Item = &'a [u8]>) -> bool {
    global_options(args.into_iter())
        .subcommand
        .is_some_and(|s| REF_WRITERS.contains(&s))
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
    /// The kernel's short name of its executable (`p_comm`: the file name, at most 16 bytes,
    /// NUL-padded), which `exec` updates. A hint to pick candidates, never an identity.
    pub comm: [u8; COMM_LEN],
}

impl ProcBrief {
    /// Whether its short name is `name` (compared up to the first NUL).
    pub fn named(&self, name: &str) -> bool {
        let end = self.comm.iter().position(|b| *b == 0).unwrap_or(COMM_LEN);
        &self.comm[..end] == name.as_bytes()
    }
}

/// `MAXCOMLEN + 1`: the size of `p_comm`.
pub const COMM_LEN: usize = 17;

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
/// `kp_proc.p_comm`.
const COMM: usize = 243;
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
                comm: r.get(COMM..COMM + COMM_LEN)?.try_into().ok()?,
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
        if pid == 44 {
            r[COMM..COMM + 3].copy_from_slice(b"git");
        }
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
                    start_us: 1_700_000_000_250_000,
                    comm: [0; COMM_LEN],
                },
                ProcBrief {
                    pid: 44,
                    ppid: 42,
                    start_us: 1_700_000_002_000_007,
                    comm: *b"git\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
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
        // The short name is read from the right offset.
        assert!(kid.named("sleep"), "{kid:?}");
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

    fn redirects(line: &str) -> bool {
        git_redirects(line.split_whitespace().map(str::as_bytes), [])
    }

    #[test]
    fn options_with_a_value_are_skipped_before_a_redirect() {
        assert!(redirects("--attr-source HEAD -C x commit"));
        assert!(redirects("--shallow-file x -C y commit"));
        assert!(redirects(
            "--attr-source=HEAD --config-env=k=E --work-tree=/w status"
        ));
    }

    #[test]
    fn an_unknown_global_option_counts_as_a_redirect() {
        assert!(redirects("--unknown-global status"));
        assert!(redirects("-x status"));
        // Glued or `=` forms git does not take are unknown too.
        assert!(redirects("-Cx status"));
        assert!(redirects("-c=k=v status"));
        assert!(redirects("--list-cmds status"));
        // `--` before the subcommand: fail closed.
        assert!(redirects("-- status"));
    }

    fn writes(line: &str) -> bool {
        git_subcommand_writes(line.split_whitespace().map(str::as_bytes))
    }

    #[test]
    fn subcommand_writes_table() {
        for line in [
            "commit -q -m x",
            "-c x=y commit",
            "-C /r --no-pager merge x",
            "--git-dir=/r/.git rebase main",
            "bisect start",
            "worktree add ../x",
            "update-ref refs/heads/x HEAD",
            "push origin x",
        ] {
            assert!(writes(line), "{line}");
        }
        for line in [
            "log",
            "status",
            "cat-file --batch",
            "-c core.pager=x log",
            // It only moves `refs/remotes/*`: no event.
            "fetch origin",
            // An alias, even of `commit`: unknown, so it fails closed.
            "ci -m x",
            "-- commit",
            "--unknown-global commit",
            "-c",
            "",
        ] {
            assert!(!writes(line), "{line}");
        }
    }

    #[test]
    fn the_args_and_the_redirect_come_from_one_read() {
        let a = area(3, b"/usr/bin/git\0\0\0git\0-C\0commit\0HOME=/u\0\0");
        let (args, redirect) = parse_procargs2_git_args_and_redirect(&a).unwrap();
        assert_eq!(args, ["git", "-C", "commit"]);
        assert!(redirect);
        let a = area(2, b"/usr/bin/git\0git\0log\0GIT_DIR=/r\0");
        assert!(parse_procargs2_git_args_and_redirect(&a).unwrap().1);
        let a = area(2, b"/usr/bin/git\0git\0log\0HOME=/u\0");
        assert!(!parse_procargs2_git_args_and_redirect(&a).unwrap().1);
        assert_eq!(parse_procargs2_git_args_and_redirect(&[1, 2]), None);
    }

    #[test]
    fn known_flags_do_not_redirect() {
        assert!(!redirects("--no-pager log -C"));
        assert!(!redirects("-p status"));
        assert!(!redirects(
            "-P --paginate --bare --no-replace-objects --no-lazy-fetch --no-optional-locks \
             --no-advice --literal-pathspecs --glob-pathspecs --noglob-pathspecs \
             --icase-pathspecs --html-path --man-path --info-path -v -h --version --help status"
        ));
        assert!(!redirects(
            "--exec-path --exec-path=/x --list-cmds=main status"
        ));
        assert!(!redirects(
            "--namespace ns --shallow-file f --super-prefix p/ status"
        ));
    }

    #[test]
    fn the_whole_environment_is_read_past_an_empty_entry() {
        let a = area(
            2,
            b"/usr/bin/git\0\0\0\0git\0status\0HOME=/u\0\0\0GIT_DIR=/r/.git\0\0",
        );
        assert_eq!(parse_procargs2_git_redirect(&a), Some(true));
        let names: [&[u8]; 3] = [b"", b"GIT_DIR", b"PATH"];
        assert!(git_redirects([], names));
        assert!(!git_redirects([], [&b""[..], b"=x", b"PATH=GIT_DIR"]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_gone_pid_is_unreadable() {
        assert_eq!(process_args(u32::MAX), None);
        assert_eq!(process_args(0x7fff_fff0), None);
    }
}
