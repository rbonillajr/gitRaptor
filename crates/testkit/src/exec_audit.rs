//! Dynamic `exec` audit (ADR-GRP-009, Validación 7; SEC-09; M1).
//!
//! Two layers, both driven by a *probe*: the code under test runs in a child process (the test
//! binary itself, re-executed) with a clean environment.
//!
//! - **Trap audit** (portable gate, no privileges). [`TrapAudit::install`] copies the running
//!   test binary into `shim/git` and into `trap/<name>` for every program a reader could launch
//!   by name (`git`, `sh`, `gpg`, `ssh`, pagers…). The probe gets `PATH=trap/` and resolves Git
//!   to `shim/git` (`engine.gitPath`). The test target calls [`dispatch`] first thing in `main`:
//!   a copy started as the shim logs its argv from the child side and runs the real Git; a copy
//!   started as a trap leaves a marker and fails. So every Git launch is recorded independently
//!   of the code under test, and a launch by name (gix looking up `git`, Git looking up a
//!   pager or `gpg`) fires a trap. Launches by absolute path are covered by the SEC-09 canary.
//!   The copies are binaries, not scripts, so the same gate works on Windows.
//! - **Kernel tracer** (deep audit, optional). [`Tracer::Strace`] (Linux, no root: traces its
//!   own descendants) and [`Tracer::Eslogger`] (macOS, root through `sudo -n` and Full Disk
//!   Access) record every `exec` of the probe's process tree. ETW on Windows is pending. Chosen
//!   with `GITRAPTOR_EXEC_AUDIT` (`auto`, `off`, `strace`, `eslogger`); an explicit choice that
//!   is unavailable is an error, `auto` skips.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use crate::guard;

/// Names trapped in the probe's `PATH`.
pub const TRAPS: &[&str] = &[
    "git",
    "sh",
    "bash",
    "zsh",
    "dash",
    "gpg",
    "gpg2",
    "gpgsm",
    "ssh",
    "less",
    "more",
    "git-lfs",
    "git-upload-pack",
    "git-receive-pack",
    "git-credential-osxkeychain",
    "perl",
    "python3",
    "cmd",
    "powershell",
];

const LAYOUT_MARKER: &str = "gitraptor-exec-audit";
const REAL_GIT: &str = "real-git";
const SHIM_LOG: &str = "shim.log";

/// A trap audit installed in a directory of a testkit root.
pub struct TrapAudit {
    pub dir: PathBuf,
}

impl TrapAudit {
    /// Install the shim and the traps in `dir` (created; must be inside a testkit root).
    /// `real_git` is the Git the shim runs.
    pub fn install(dir: &Path, real_git: &Path) -> Self {
        std::fs::create_dir_all(dir).unwrap();
        guard::check(dir).unwrap_or_else(|e| panic!("harness guard: {e}"));
        for sub in ["shim", "trap", "fired"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join(LAYOUT_MARKER), "").unwrap();
        std::fs::write(dir.join(REAL_GIT), real_git.as_os_str().as_encoded_bytes()).unwrap();
        let exe = std::env::current_exe().expect("current test binary");
        link_or_copy(&exe, &dir.join("shim").join(exe_name("git")));
        for name in TRAPS {
            link_or_copy(&exe, &dir.join("trap").join(exe_name(name)));
        }
        Self {
            dir: dir.to_owned(),
        }
    }

    /// Path to give the code under test as its Git (`engine.gitPath`).
    pub fn shim_git(&self) -> PathBuf {
        self.dir.join("shim").join(exe_name("git"))
    }

    /// `PATH` value for the probe and for the Git it launches.
    pub fn trap_path(&self) -> OsString {
        self.dir.join("trap").into_os_string()
    }

    /// Every argv the shim received, oldest first (without the program itself).
    pub fn launches(&self) -> Vec<Vec<String>> {
        let Ok(bytes) = std::fs::read(self.dir.join(SHIM_LOG)) else {
            return Vec::new();
        };
        decode_log(&bytes)
    }

    /// Traps that fired.
    pub fn fired(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(self.dir.join("fired"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }
}

/// Call first thing in `main` of a `harness = false` test target that uses [`TrapAudit`].
/// Returns the exit code when this process is a shim or a trap copy; `None` otherwise.
pub fn dispatch() -> Option<ExitCode> {
    let exe = std::env::current_exe().ok()?;
    let role_dir = exe.parent()?;
    let audit_dir = role_dir.parent()?;
    if !audit_dir.join(LAYOUT_MARKER).is_file() {
        return None;
    }
    let name = exe.file_stem()?.to_string_lossy().into_owned();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match role_dir.file_name()?.to_str()? {
        "shim" => {
            let mut record = Vec::new();
            for a in &args {
                let bytes = a.as_encoded_bytes();
                record.extend_from_slice(format!("{}:", bytes.len()).as_bytes());
                record.extend_from_slice(bytes);
            }
            record.push(b'\n');
            if let Ok(mut log) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(audit_dir.join(SHIM_LOG))
            {
                let _ = log.write_all(&record);
            }
            let real = std::fs::read(audit_dir.join(REAL_GIT)).ok()?;
            let real = PathBuf::from(String::from_utf8_lossy(&real).into_owned());
            let status = Command::new(real).args(&args).status();
            Some(match status.ok().and_then(|s| s.code()) {
                Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
                None => ExitCode::FAILURE,
            })
        }
        "trap" => {
            let _ = std::fs::write(audit_dir.join("fired").join(&name), "");
            Some(ExitCode::from(127))
        }
        _ => None,
    }
}

fn decode_log(bytes: &[u8]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let mut argv = Vec::new();
        let mut rest = line;
        while let Some(colon) = rest.iter().position(|b| *b == b':') {
            let len: usize = String::from_utf8_lossy(&rest[..colon]).parse().unwrap();
            let start = colon + 1;
            argv.push(String::from_utf8_lossy(&rest[start..start + len]).into_owned());
            rest = &rest[start + len..];
        }
        out.push(argv);
    }
    out
}

fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn link_or_copy(from: &Path, to: &Path) {
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).expect("copy test binary");
    }
}

/// The child process that runs the code under test.
#[derive(Debug, Clone)]
pub struct ProbeSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The whole environment of the probe (nothing is inherited).
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
}

impl ProbeSpec {
    /// The running test binary, re-executed with `args` and `env`.
    pub fn current_exe(args: &[&str], env: &[(&str, &OsStr)], cwd: &Path) -> Self {
        Self {
            program: std::env::current_exe().expect("current test binary"),
            args: args.iter().map(OsString::from).collect(),
            env: env
                .iter()
                .map(|(k, v)| (OsString::from(k), v.to_os_string()))
                .collect(),
            cwd: cwd.to_owned(),
        }
    }

    fn command(&self, program: &Path, prefix: &[OsString]) -> Command {
        let mut c = Command::new(program);
        c.args(prefix)
            .arg(&self.program)
            .args(&self.args)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        c
    }

    /// Run the probe without a kernel tracer.
    pub fn run(&self) -> ProbeOutcome {
        let out = Command::new(&self.program)
            .args(&self.args)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .expect("run probe");
        ProbeOutcome::from(out)
    }
}

#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl From<std::process::Output> for ProbeOutcome {
    fn from(out: std::process::Output) -> Self {
        Self {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

/// One `exec` observed by a kernel tracer in the probe's process tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
    pub pid: u32,
    pub program: String,
    pub argv: Vec<String>,
    /// `false` when the kernel rejected it (e.g. `ENOENT` while searching `PATH`): an attempt
    /// still counts.
    pub ok: bool,
}

/// Kernel-level tracer of the probe's process tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tracer {
    Strace(PathBuf),
    Eslogger,
}

pub const TRACER_ENV: &str = "GITRAPTOR_EXEC_AUDIT";

impl Tracer {
    /// The tracer chosen by `GITRAPTOR_EXEC_AUDIT`. `Ok(None)`: none available in `auto`, or
    /// `off`. `Err`: an explicit choice is not available here.
    pub fn from_env() -> Result<Option<Self>, String> {
        let choice = std::env::var(TRACER_ENV).unwrap_or_else(|_| "auto".into());
        match choice.as_str() {
            "off" => Ok(None),
            "strace" => find_strace()
                .map(|p| Some(Self::Strace(p)))
                .ok_or_else(|| "strace not found".into()),
            "eslogger" => {
                if eslogger_available() {
                    Ok(Some(Self::Eslogger))
                } else {
                    Err("eslogger needs macOS and passwordless sudo (`sudo -n`)".into())
                }
            }
            "auto" if cfg!(target_os = "linux") => Ok(find_strace().map(Self::Strace)),
            "auto" => Ok(None),
            other => Err(format!("unknown {TRACER_ENV}={other}")),
        }
    }

    /// Run `probe` under the tracer; `work` (inside a testkit root) holds the trace.
    pub fn trace(
        &self,
        probe: &ProbeSpec,
        work: &Path,
    ) -> Result<(ProbeOutcome, Vec<Exec>), String> {
        guard::check(work).map_err(|e| e.to_string())?;
        match self {
            Self::Strace(strace) => {
                let log = work.join("strace.log");
                let prefix: Vec<OsString> = [
                    "-f",
                    "-qq",
                    "-v",
                    "-s",
                    "65535",
                    "-e",
                    "trace=execve,execveat",
                    "-o",
                ]
                .iter()
                .map(OsString::from)
                .chain([log.clone().into_os_string(), "--".into()])
                .collect();
                let out = probe
                    .command(strace, &prefix)
                    .output()
                    .map_err(|e| format!("strace: {e}"))?;
                let text = std::fs::read_to_string(&log).map_err(|e| format!("strace log: {e}"))?;
                let mut execs = parse_strace(&text);
                // The first exec is the probe itself.
                if !execs.is_empty() {
                    execs.remove(0);
                }
                Ok((ProbeOutcome::from(out), execs))
            }
            Self::Eslogger => trace_eslogger(probe, work),
        }
    }
}

fn find_strace() -> Option<PathBuf> {
    ["/usr/bin/strace", "/bin/strace", "/usr/local/bin/strace"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

fn eslogger_available() -> bool {
    cfg!(target_os = "macos")
        && Path::new("/usr/bin/eslogger").is_file()
        && Command::new("/usr/bin/sudo")
            .args(["-n", "true"])
            .stdin(Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success())
}

/// Parse `strace -f -o` output: `PID execve("path", ["a", "b"], ...) = 0`.
pub fn parse_strace(text: &str) -> Vec<Exec> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((pid, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        let rest = rest.trim_start();
        let call = if let Some(r) = rest.strip_prefix("execve(") {
            r
        } else if let Some(r) = rest.strip_prefix("execveat(") {
            // execveat(dirfd, "path", [...], ...): skip the dirfd.
            r.split_once(", ").map_or(r, |(_, r)| r)
        } else {
            continue;
        };
        let mut strings = quoted_strings(call);
        if strings.is_empty() {
            continue;
        }
        let program = strings.remove(0);
        // Only the argv array follows the path; envp is printed after it with `-v`, so cut at
        // the closing bracket of the first array.
        let argv_part = call
            .split_once('[')
            .and_then(|(_, r)| split_array(r))
            .unwrap_or_default();
        let argv = quoted_strings(&argv_part);
        out.push(Exec {
            pid,
            program,
            argv,
            ok: !line.contains("= -1"),
        });
    }
    out
}

/// The text of a `[...]` array up to its closing bracket, outside quotes.
fn split_array(s: &str) -> Option<String> {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            ']' if !in_str => return Some(s[..i].to_owned()),
            _ => {}
        }
    }
    None
}

/// Every C-style quoted string in `s`, unescaped.
fn quoted_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut cur = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => cur.push('\n'),
                    Some('t') => cur.push('\t'),
                    Some(other) => cur.push(other),
                    None => {}
                },
                c => cur.push(c),
            }
        }
        out.push(cur);
    }
    out
}

/// eslogger needs root: it is started through `sudo -n`, the probe runs as the user, and the
/// process tree is rebuilt from `ppid`. Not verified on a developer Mac without root.
fn trace_eslogger(probe: &ProbeSpec, work: &Path) -> Result<(ProbeOutcome, Vec<Exec>), String> {
    let log = work.join("eslogger.jsonl");
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut es = Command::new("/usr/bin/sudo")
        .args(["-n", "/usr/bin/eslogger", "exec", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("eslogger: {e}"))?;
    std::thread::sleep(Duration::from_secs(2));
    let child = probe
        .command(&probe.program, &[])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let result = child.map_err(|e| e.to_string()).and_then(|child| {
        let pid = child.id();
        child
            .wait_with_output()
            .map(|o| (pid, ProbeOutcome::from(o)))
            .map_err(|e| e.to_string())
    });
    std::thread::sleep(Duration::from_secs(1));
    // `sudo` relays SIGTERM to eslogger; the user can signal it (same real uid).
    let _ = Command::new("/bin/kill")
        .args(["-TERM", &es.id().to_string()])
        .status();
    let _ = es.wait();
    let (pid, outcome) = result?;
    let text = std::fs::read_to_string(&log).map_err(|e| e.to_string())?;
    Ok((outcome, parse_eslogger(&text, pid, &probe.program)))
}

/// Execs of the process tree rooted at `root_pid` in eslogger JSON lines, without the probe's
/// own exec.
pub fn parse_eslogger(text: &str, root_pid: u32, probe: &Path) -> Vec<Exec> {
    let mut tree = std::collections::BTreeSet::from([root_pid]);
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let pid = v["process"]["audit_token"]["pid"].as_u64().unwrap_or(0) as u32;
        let ppid = v["process"]["ppid"].as_u64().unwrap_or(0) as u32;
        if !tree.contains(&pid) && !tree.contains(&ppid) {
            continue;
        }
        tree.insert(pid);
        let exec = &v["event"]["exec"];
        let program = exec["target"]["executable"]["path"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if pid == root_pid && Path::new(&program) == probe {
            continue;
        }
        let argv = exec["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| s.as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        out.push(Exec {
            pid,
            program,
            argv,
            ok: true,
        });
    }
    out
}
