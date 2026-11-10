//! `presence-probe`: SPIKE-GRD-002 (macOS) isolated prototype.
//!
//! All `unsafe` lives in the private `ffi_*` modules (the shape `crates/macsys`
//! would use, ADR-GRP-002 exception); this file is safe Rust only.
#![deny(unsafe_code)]

#[cfg(target_os = "macos")]
mod ffi_events;
#[cfg(target_os = "macos")]
mod ffi_local_auth;
#[cfg(target_os = "macos")]
mod ffi_session;

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("presence-probe only runs on macOS");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    imp::main()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::HashSet;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use serde_json::{Value, json};

    use crate::ffi_events as ev;
    use crate::ffi_local_auth::{self as la, Policy};
    use crate::ffi_session as sess;

    struct Args(Vec<String>);
    impl Args {
        fn flag(&self, k: &str) -> bool {
            self.0.iter().any(|a| a == k)
        }
        fn opt(&self, k: &str) -> Option<String> {
            self.0
                .iter()
                .position(|a| a == k)
                .and_then(|i| self.0.get(i + 1).cloned())
        }
        fn f64(&self, k: &str, d: f64) -> f64 {
            self.opt(k).and_then(|v| v.parse().ok()).unwrap_or(d)
        }
    }

    fn now_unix_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    fn cmd_out(prog: &str, args: &[&str]) -> Option<String> {
        Command::new(prog)
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    pub fn context_info() -> Value {
        json!({
            "pid": std::process::id(),
            "ppid": std::os::unix::process::parent_id(),
            "sid": nix_getsid(),
            "uid": nix_getuid(),
            "exe": std::env::current_exe().ok().map(|p| p.display().to_string()),
            "sw_vers": cmd_out("/usr/bin/sw_vers", &["-productVersion"]),
            "build": cmd_out("/usr/bin/sw_vers", &["-buildVersion"]),
            "launchctl_managername": cmd_out("/bin/launchctl", &["managername"]),
            "security_session": sess::security_session(),
            "cg_session": sess::cg_session(),
            "event_access": ev::access(),
            "env_keys": std::env::vars().map(|(k, _)| k).collect::<Vec<_>>(),
        })
    }

    fn nix_getsid() -> i64 {
        crate::ffi_setsid::getsid()
    }
    fn nix_getuid() -> String {
        cmd_out("/usr/bin/id", &["-u"]).unwrap_or_default()
    }

    fn policy_of(a: &Args) -> Policy {
        match a.opt("--policy").as_deref() {
            Some("bio-watch") => Policy::BiometricsOrWatch,
            Some("bio") => Policy::Biometrics,
            _ => Policy::Owner,
        }
    }

    /// Polls the window list to time the dialog (appearance after the request,
    /// disappearance after `invalidate`). Only geometry and owner, never content.
    struct Watcher {
        stop: Arc<AtomicBool>,
        seen: Arc<Mutex<Vec<(u128, sess::Win)>>>,
        gone: Arc<Mutex<Option<u128>>>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Watcher {
        fn start(t0: Instant) -> Self {
            let baseline: HashSet<i64> = sess::windows().iter().map(|w| w.number).collect();
            let me = std::process::id() as i64;
            let stop = Arc::new(AtomicBool::new(false));
            let seen: Arc<Mutex<Vec<(u128, sess::Win)>>> = Arc::default();
            let gone: Arc<Mutex<Option<u128>>> = Arc::default();
            let (s2, seen2, gone2) = (stop.clone(), seen.clone(), gone.clone());
            let handle = std::thread::spawn(move || {
                let mut tracked: HashSet<i64> = HashSet::new();
                while !s2.load(Ordering::Relaxed) {
                    let ws = sess::windows();
                    let now: HashSet<i64> = ws.iter().map(|w| w.number).collect();
                    for w in ws {
                        if !baseline.contains(&w.number)
                            && w.pid != me
                            && !tracked.contains(&w.number)
                        {
                            tracked.insert(w.number);
                            seen2.lock().unwrap().push((t0.elapsed().as_millis(), w));
                        }
                    }
                    if !tracked.is_empty() && tracked.iter().all(|n| !now.contains(n)) {
                        let mut g = gone2.lock().unwrap();
                        if g.is_none() {
                            *g = Some(t0.elapsed().as_millis());
                        }
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            Watcher {
                stop,
                seen,
                gone,
                handle: Some(handle),
            }
        }
        fn finish(mut self) -> (Value, Option<u128>) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
            let seen = self.seen.lock().unwrap();
            let first = seen.first().map(|(t, _)| *t);
            let list: Vec<Value> = seen
                .iter()
                .take(6)
                .map(|(t, w)| {
                    let mut v = sess::win_json(w);
                    v["t_ms"] = json!(t);
                    v
                })
                .collect();
            (
                json!({"first_window_ms": first, "windows": list, "all_gone_ms": *self.gone.lock().unwrap()}),
                first,
            )
        }
    }

    fn outcome_of(r: &la::Reply) -> &'static str {
        if r.success {
            return "verified";
        }
        match r.code {
            Some(-2) => "cancelled", // userCancel
            Some(-3) => "fallback",  // userFallback
            Some(-1) => "denied",    // authenticationFailed
            Some(-4) => "system-cancel",
            Some(-9) => "app-cancel",     // invalidate()
            Some(-1004) => "unavailable", // notInteractive
            Some(-6) | Some(-7) | Some(-5) | Some(-8) | Some(-11) => "unavailable",
            _ => "error",
        }
    }

    /// One evaluation with a fresh context: the unit the daemon would run per request.
    pub fn evaluate_once(
        reason: &str,
        timeout: f64,
        policy: Policy,
        late_wait: f64,
        drop_early: bool,
    ) -> Value {
        let t0 = Instant::now();
        let ctx = la::Context::new();
        let pre = ctx.preflight(policy);
        let watcher = Watcher::start(t0);
        let rx = ctx.start(policy, reason);
        let requested_ms = t0.elapsed().as_millis();
        let reply = la::wait(&rx, Duration::from_secs_f64(timeout));
        let mut timed_out = false;
        let mut late: Value = Value::Null;
        if reply.is_none() {
            timed_out = true;
            let t_inv = Instant::now();
            ctx.invalidate();
            if drop_early {
                // H1: drop context and receiver right away; the framework's reply
                // must then hit a closed channel without UB.
                drop(ctx);
                drop(rx);
                std::thread::sleep(Duration::from_secs_f64(late_wait));
                let (w, _) = watcher.finish();
                return json!({
                    "kind": "evaluate", "policy": policy.name(), "outcome": "timeout",
                    "drop_early": true, "preflight": pre_json(&pre), "dialog": w,
                    "note": "context and receiver dropped right after invalidate; process alive => no UB observed",
                    "timeout_s": timeout,
                });
            }
            let lr = la::wait(&rx, Duration::from_secs_f64(late_wait));
            late = match lr {
                Some(r) => json!({
                    "arrived": true, "after_invalidate_ms": r.at.duration_since(t_inv).as_millis(),
                    "code": r.code, "success": r.success, "on_main_thread": r.on_main_thread,
                }),
                None => json!({"arrived": false}),
            };
            // Give the watcher a moment to see the dialog go away.
            std::thread::sleep(Duration::from_millis(1500));
        } else {
            std::thread::sleep(Duration::from_millis(600));
        }
        let (dialog, _) = watcher.finish();
        let domain_state_len = ctx.domain_state_len();
        let r = reply.as_ref();
        json!({
            "kind": "evaluate",
            "at_unix_ms": now_unix_ms(),
            "policy": policy.name(),
            "reason_chars": reason.chars().count(),
            "preflight": pre_json(&pre),
            "requested_ms": requested_ms,
            "outcome": if timed_out { "timeout" } else { r.map(outcome_of).unwrap_or("error") },
            "la_code": r.and_then(|r| r.code),
            "la_domain": r.and_then(|r| r.domain.clone()),
            "la_message": r.and_then(|r| r.message.clone()),
            "result_ms": r.map(|r| r.at.duration_since(t0).as_millis()),
            "reply_on_main_thread": r.map(|r| r.on_main_thread),
            "reply_thread_id": r.map(|r| r.thread_id),
            "caller_thread_id": current_tid(),
            "timeout_s": timeout,
            "late_reply_after_invalidate": late,
            "dialog": dialog,
            "evaluated_domain_state_bytes": domain_state_len,
            "method_reported": "not-reported",
        })
    }

    fn current_tid() -> String {
        format!("{:?}", std::thread::current().id())
    }

    fn pre_json(p: &la::Preflight) -> Value {
        json!({"ok": p.ok, "code": p.code, "message": p.message, "biometry_type": p.biometry_type, "reuse_duration_s": p.reuse_duration})
    }

    fn print(v: &Value) {
        println!("{}", serde_json::to_string(v).unwrap());
    }

    const DEFAULT_REASON: &str =
        "confirmar «añadir repo /tmp/spike-repo» en GitRaptor (spike, código K7Q2)";

    fn cmd_evaluate(a: &Args) {
        let reason = a
            .opt("--reason")
            .unwrap_or_else(|| DEFAULT_REASON.to_string());
        let timeout = a.f64("--timeout", 60.0);
        let late = a.f64("--late-wait", 5.0);
        let count = a.f64("--count", 1.0) as usize;
        let policy = policy_of(a);
        let mut out = vec![];
        for i in 0..count {
            let mut v = evaluate_once(&reason, timeout, policy, late, a.flag("--drop-early"));
            v["index"] = json!(i);
            print(&v);
            out.push(v);
        }
        if a.flag("--with-context") {
            print(&json!({"kind": "context", "context": context_info()}));
        }
    }

    /// F3 demonstration: re-evaluating the SAME context after a success.
    fn cmd_same_context(a: &Args) {
        let reason = a
            .opt("--reason")
            .unwrap_or_else(|| DEFAULT_REASON.to_string());
        let timeout = a.f64("--timeout", 30.0);
        let ctx = la::Context::new();
        for i in 0..2 {
            let t0 = Instant::now();
            let rx = ctx.start(Policy::Owner, &reason);
            let r = la::wait(&rx, Duration::from_secs_f64(timeout));
            print(&json!({"kind": "same-context", "index": i,
                "outcome": r.as_ref().map(outcome_of).unwrap_or("timeout"),
                "la_code": r.as_ref().and_then(|r| r.code),
                "result_ms": t0.elapsed().as_millis()}));
            if r.is_none() {
                ctx.invalidate();
                break;
            }
        }
    }

    /// E8: two contexts evaluating at once in one process.
    fn cmd_pair(a: &Args) {
        let timeout = a.f64("--timeout", 8.0);
        let t0 = Instant::now();
        let w = Watcher::start(t0);
        let c1 = la::Context::new();
        let c2 = la::Context::new();
        let r1 = c1.start(Policy::Owner, "spike par 1 (código P1P1)");
        std::thread::sleep(Duration::from_millis(300));
        let r2 = c2.start(Policy::Owner, "spike par 2 (código P2P2)");
        let a1 = la::wait(&r1, Duration::from_secs_f64(timeout));
        let a2 = la::wait(&r2, Duration::from_millis(500));
        c1.invalidate();
        c2.invalidate();
        let l1 = a1.clone().or_else(|| la::wait(&r1, Duration::from_secs(3)));
        let l2 = a2.clone().or_else(|| la::wait(&r2, Duration::from_secs(3)));
        std::thread::sleep(Duration::from_millis(1200));
        let (d, _) = w.finish();
        let j = |r: &Option<la::Reply>| {
            r.as_ref().map(|r| json!({"outcome": outcome_of(r), "code": r.code, "ms": r.at.duration_since(t0).as_millis(), "msg": r.message}))
        };
        print(&json!({"kind": "pair", "first": j(&l1), "second": j(&l2), "dialog": d}));
    }

    // ---- daemon simulation -------------------------------------------------

    fn cmd_serve(a: &Args) {
        if a.flag("--setsid") {
            // Same order as the product: the daemon itself calls setsid() on start.
            setsid_self();
        }
        let path = a.opt("--socket").expect("--socket");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind");
        let busy = Arc::new(Mutex::new(()));
        eprintln!(
            "{}",
            json!({"kind": "serve-start", "context": context_info()})
        );
        let max = a.f64("--max-requests", 50.0) as usize;
        for (n, conn) in listener.incoming().enumerate() {
            let Ok(conn) = conn else { continue };
            let busy = busy.clone();
            let h = std::thread::spawn(move || handle(conn, busy));
            if n + 1 >= max {
                let _ = h.join();
                break;
            }
        }
    }

    fn setsid_self() {
        crate::ffi_session_setsid()
    }

    fn handle(mut conn: UnixStream, busy: Arc<Mutex<()>>) {
        let mut line = String::new();
        if BufReader::new(&conn).read_line(&mut line).is_err() {
            return;
        }
        let req: Value = serde_json::from_str(&line).unwrap_or(json!({}));
        let resp = match req["op"].as_str() {
            Some("info") => json!({"kind": "info", "context": context_info()}),
            Some("quit") => {
                let _ = writeln!(conn, "{}", json!({"kind": "bye"}));
                std::process::exit(0)
            }
            _ => match busy.try_lock() {
                Err(_) => json!({"kind": "evaluate", "outcome": "busy"}),
                Ok(_guard) => {
                    let reason = req["reason"].as_str().unwrap_or(DEFAULT_REASON).to_string();
                    let timeout = req["timeout"].as_f64().unwrap_or(60.0);
                    let mut v = evaluate_once(&reason, timeout, Policy::Owner, 5.0, false);
                    v["server_context"] = context_info();
                    v
                }
            },
        };
        let _ = writeln!(conn, "{}", resp);
    }

    fn cmd_request(a: &Args) {
        let path = a.opt("--socket").expect("--socket");
        let mut s = UnixStream::connect(&path).expect("connect");
        let req = match a.opt("--op").as_deref() {
            Some(op) => json!({"op": op}),
            None => json!({
                "op": "evaluate",
                "reason": a.opt("--reason").unwrap_or_else(|| DEFAULT_REASON.to_string()),
                "timeout": a.f64("--timeout", 60.0),
            }),
        };
        writeln!(s, "{req}").unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let mut v: Value = serde_json::from_str(&line).unwrap_or(json!({"raw": line}));
        v["client_context"] = context_info();
        print(&v);
    }

    /// On-demand start in the product's order: the client clears the environment
    /// (env_clear + minimal vars, like crates/core/src/client.rs) and spawns
    /// `serve --setsid`; the daemon calls setsid() itself.
    fn cmd_spawn(a: &Args) {
        let path = a.opt("--socket").expect("--socket");
        let log = a.opt("--log").expect("--log");
        let exe = std::env::current_exe().unwrap();
        let logf = std::fs::File::create(&log).unwrap();
        let mut c = Command::new(exe);
        c.args(["serve", "--setsid", "--socket", &path])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(logf);
        for k in ["HOME", "TMPDIR", "USER", "LOGNAME"] {
            if let Ok(v) = std::env::var(k) {
                c.env(k, v);
            }
        }
        c.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
        // The daemon outlives the spawner on purpose (on-demand start); it is never waited on.
        #[allow(clippy::zombie_processes)]
        let child = c.spawn().expect("spawn");
        let t0 = Instant::now();
        while !std::path::Path::new(&path).exists() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        print(
            &json!({"kind": "spawned", "pid": child.id(), "socket": path, "ready_ms": t0.elapsed().as_millis(), "spawner_context": context_info()}),
        );
    }

    fn cmd_windows() {
        let ws: Vec<Value> = sess::windows().iter().map(sess::win_json).collect();
        print(&json!({"kind": "windows", "windows": ws}));
    }

    /// Waits until a window of one of `owners` appears and prints it (for attack scripts).
    fn cmd_wait_dialog(a: &Args) {
        let owners: Vec<String> = a
            .opt("--owners")
            .unwrap_or_default()
            .split(',')
            .map(str::to_string)
            .collect();
        let secs = a.f64("--secs", 10.0);
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs_f64(secs) {
            if let Some(w) = sess::windows()
                .into_iter()
                .find(|w| owners.iter().any(|o| !o.is_empty() && w.owner == *o))
            {
                print(
                    &json!({"kind": "dialog", "found": true, "window": sess::win_json(&w), "ms": t0.elapsed().as_millis()}),
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        print(&json!({"kind": "dialog", "found": false}));
    }

    /// E15: run as the main executable of a test `.app`. `--mode child` spawns the
    /// sibling headless binary (`Contents/MacOS/raptor`, like Tauri's externalBin)
    /// with fixed argv and a cleared environment; `--mode ondemand` makes it the
    /// on-demand daemon; `--mode direct` evaluates in the bundle's main process.
    fn cmd_bundle_main(a: &Args) {
        let out = a.opt("--out").expect("--out");
        let timeout = a.f64("--timeout", 5.0).to_string();
        let mode = a.opt("--mode").unwrap_or_else(|| "child".into());
        let exe = std::env::current_exe().unwrap();
        let sibling = exe.with_file_name("raptor");
        let mut lines =
            vec![json!({"kind": "bundle-main", "mode": mode, "context": context_info()})];
        let run = |args: &[&str]| -> Vec<Value> {
            let o = Command::new(&sibling)
                .args(args)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::null())
                .output()
                .expect("spawn sibling");
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        };
        match mode.as_str() {
            "direct" => {
                let mut v = evaluate_once(
                    DEFAULT_REASON,
                    timeout.parse().unwrap_or(5.0),
                    Policy::Owner,
                    3.0,
                    false,
                );
                v["cell"] = json!("bundle:direct");
                lines.push(v);
            }
            "ondemand" => {
                let sock = format!("{out}.sock");
                for mut v in run(&[
                    "spawn",
                    "--socket",
                    &sock,
                    "--log",
                    &format!("{out}.serve.log"),
                ]) {
                    v["cell"] = json!("bundle:ondemand:spawn");
                    lines.push(v);
                }
                for mut v in run(&["request", "--socket", &sock, "--timeout", &timeout]) {
                    v["cell"] = json!("bundle:ondemand:evaluate");
                    lines.push(v);
                }
                let _ = run(&["request", "--socket", &sock, "--op", "quit"]);
            }
            _ => {
                for mut v in run(&[
                    "evaluate",
                    "--timeout",
                    &timeout,
                    "--late-wait",
                    "3",
                    "--with-context",
                ]) {
                    v["cell"] = json!("bundle:child");
                    lines.push(v);
                }
            }
        }
        let body: String = lines.iter().map(|v| format!("{v}\n")).collect();
        std::fs::write(&out, body).expect("write --out");
    }

    pub fn main() {
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let a = Args(argv.clone());
        let state = a.opt("--source").unwrap_or_else(|| "combined".into());
        let loc = a.opt("--tap").unwrap_or_else(|| "hid".into());
        match argv.first().map(String::as_str) {
            Some("info") => print(&context_info()),
            Some("evaluate") => cmd_evaluate(&a),
            Some("same-context") => cmd_same_context(&a),
            Some("pair") => cmd_pair(&a),
            Some("serve") => cmd_serve(&a),
            Some("request") => cmd_request(&a),
            Some("spawn") => cmd_spawn(&a),
            Some("windows") => cmd_windows(),
            Some("wait-dialog") => cmd_wait_dialog(&a),
            Some("bundle-main") => cmd_bundle_main(&a),
            Some("key") => {
                let code: u16 = a
                    .opt("--code")
                    .and_then(|c| c.parse().ok())
                    .expect("--code");
                ev::key(code, &state, &loc);
                print(
                    &json!({"kind": "key", "code": code, "source": state, "tap": loc, "access": ev::access()}),
                );
            }
            Some("type") => {
                let text = a.opt("--text").expect("--text");
                ev::type_text(&text, &state, &loc);
                print(
                    &json!({"kind": "type", "chars": text.chars().count(), "source": state, "tap": loc}),
                );
            }
            Some("click") => {
                let x = a.f64("--x", 0.0);
                let y = a.f64("--y", 0.0);
                ev::click(x, y, &state, &loc);
                print(&json!({"kind": "click", "x": x, "y": y, "source": state, "tap": loc}));
            }
            Some("tap") => print(&json!({"kind": "tap", "result": ev::tap(a.f64("--secs", 5.0))})),
            _ => {
                eprintln!(
                    "usage: presence-probe <info|evaluate|same-context|pair|serve|request|spawn|windows|wait-dialog|key|type|click|tap> [opts]"
                );
                std::process::exit(2);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn ffi_session_setsid() {
    ffi_setsid::setsid()
}

#[cfg(target_os = "macos")]
mod ffi_setsid {
    #![allow(unsafe_code)]
    pub fn setsid() {
        // SAFETY: setsid() has no memory-safety preconditions; failure (already a
        // group leader) is reported via the return value and ignored here.
        let r = unsafe { libc::setsid() };
        eprintln!("{{\"kind\":\"setsid\",\"ret\":{r}}}");
    }
    pub fn getsid() -> i64 {
        // SAFETY: getsid(0) queries the calling process; no pointers involved.
        unsafe { libc::getsid(0) as i64 }
    }
}
