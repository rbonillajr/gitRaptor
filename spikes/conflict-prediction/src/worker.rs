//! A2-W: A2 in a long-lived worker process (the prototype binary itself, never `git`).
//! The client side applies `RLIMIT_CPU`, a wall-clock timer per pair, a resident-memory watch
//! (macOS does not enforce `RLIMIT_AS`/`RLIMIT_RSS`) and kills and relaunches the worker.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use crate::a2;

/// Worker side: one job per stdin line `<ours> <theirs>`, one JSON line per answer.
pub fn serve(repo_path: &Path, limits: &a2::Limits) -> Result<(), String> {
    let base = crate::open::open(repo_path)?;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let mut f = line.split_whitespace();
        let (Some(o), Some(t)) = (f.next(), f.next()) else {
            continue;
        };
        let started = Instant::now();
        let v = crate::predict_json(&base, o, t, limits, started);
        writeln!(out, "{v}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub struct Client {
    exe: PathBuf,
    args: Vec<String>,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    pub cpu_secs: u64,
    pub max_rss: u64,
    pub timeout: Duration,
    pub restarts: u32,
    pub peak_rss: u64,
}

fn spawn(
    exe: &Path,
    args: &[String],
    cpu_secs: u64,
) -> Result<(Child, ChildStdin, Receiver<String>), String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls (setrlimit, setpriority) between fork and exec.
    unsafe {
        cmd.pre_exec(move || {
            let lim = libc::rlimit {
                rlim_cur: cpu_secs as libc::rlim_t,
                rlim_max: cpu_secs as libc::rlim_t + 1,
            };
            if libc::setrlimit(libc::RLIMIT_CPU, &lim) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Background priority (QoS utility is per thread on macOS; nice is the portable part).
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("spawn worker: {e}"))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Ok((child, stdin, rx))
}

/// Resident memory of a process in bytes (macOS `proc_pidinfo`).
pub fn rss_of(pid: u32) -> u64 {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable `proc_taskinfo`.
    let n = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    if n == size { info.pti_resident_size } else { 0 }
}

impl Client {
    pub fn new(
        repo: &Path,
        worker_args: &[String],
        cpu_secs: u64,
        max_rss: u64,
        timeout: Duration,
    ) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut args = vec!["worker".to_owned(), repo.display().to_string()];
        args.extend(worker_args.iter().cloned());
        let (child, stdin, lines) = spawn(&exe, &args, cpu_secs)?;
        Ok(Self {
            exe,
            args,
            child,
            stdin,
            lines,
            cpu_secs,
            max_rss,
            timeout,
            restarts: 0,
            peak_rss: 0,
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    fn restart(&mut self) -> Result<(), String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (child, stdin, lines) = spawn(&self.exe, &self.args, self.cpu_secs)?;
        self.child = child;
        self.stdin = stdin;
        self.lines = lines;
        self.restarts += 1;
        Ok(())
    }

    /// One pair. On timeout, memory over the cap or a dead worker: kill, relaunch, and report
    /// the pair as not computable.
    pub fn run(&mut self, ours: &str, theirs: &str) -> serde_json::Value {
        if writeln!(self.stdin, "{ours} {theirs}")
            .and_then(|_| self.stdin.flush())
            .is_err()
        {
            let _ = self.restart();
            return serde_json::json!({"ok": false, "err": "worker-dead"});
        }
        let start = Instant::now();
        loop {
            match self.lines.recv_timeout(Duration::from_millis(20)) {
                Ok(line) => {
                    return serde_json::from_str(&line)
                        .unwrap_or_else(|_| serde_json::json!({"ok": false, "err": "bad-answer"}));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self.child.try_wait().ok().flatten();
                    let _ = self.restart();
                    return serde_json::json!({"ok": false, "err": format!("worker-died: {status:?}")});
                }
                Err(RecvTimeoutError::Timeout) => {
                    let rss = rss_of(self.child.id());
                    self.peak_rss = self.peak_rss.max(rss);
                    if self.max_rss > 0 && rss > self.max_rss {
                        let _ = self.restart();
                        return serde_json::json!({"ok": false, "err": "worker-killed: rss", "rss": rss});
                    }
                    if start.elapsed() > self.timeout {
                        let _ = self.restart();
                        return serde_json::json!({"ok": false, "err": "worker-killed: timeout"});
                    }
                }
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
