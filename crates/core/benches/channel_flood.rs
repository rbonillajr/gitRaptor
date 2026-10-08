//! Bench of the channel under a slow client and a connection flood (TS-GRP-004 SEC-08,
//! ADR-GRP-011 § 4, INF-GRP-002).
//!
//! An in-process daemon over a temporary profile (NFR-01) with a good subscriber, one that
//! subscribes and never reads again, and 100 more connections past the limit. The bench
//! publishes 3000 change events of ~2 KB on the bus and measures, for each one the good client
//! receives, `t_published` → receipt on the common monotonic clock. **Fails** (exit code 1) when
//! the p95 reaches the IPC budget of ADR-GRP-011 (25 ms), confirmed by measuring again: two of
//! three attempts over the budget. The functional side (no event lost, the slow client gets the
//! resync) is the debug test `slow_client_and_connection_flood_do_not_starve_the_others`.
//!
//! ```sh
//! cargo bench -p gitraptor-core --bench channel_flood
//! cargo bench -p gitraptor-core --bench channel_flood -- --events 10000
//! ```

#[cfg(not(unix))]
fn main() {
    // The channel client only exists on Unix (TS-GRP-004).
    // Pendiente: etapa de validación multiplataforma.
    println!("channel flood bench: skipped, the channel client is Unix-only for now");
}

#[cfg(unix)]
fn main() {
    std::process::exit(unix::run());
}

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use gitraptor_api::messages::{ClientKind, SubscribeResult};
    use gitraptor_api::rpc::Notification;
    use gitraptor_api::{PROTOCOL_VERSION, Timings, clock, methods};
    use gitraptor_core::channel::ChannelConfig;
    use gitraptor_core::client::{self, Client};
    use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
    use gitraptor_core::profile::ProfileDirs;
    use gitraptor_git::resolve::ResolveConfig;
    use serde_json::json;

    /// IPC budget of ADR-GRP-011 § 4: `t_published` → receipt by the client, p95.
    const BUDGET_MS: f64 = 25.0;
    /// Connections opened on top of the two subscribers; past the limit they are refused.
    const FLOOD: usize = 100;
    const PAYLOAD: usize = 2048;
    const ATTEMPTS: usize = 3;
    /// Attempts over the budget that fail the bench.
    const CONFIRMED: usize = 2;

    struct Opts {
        events: usize,
    }

    fn opts() -> Opts {
        let mut o = Opts { events: 3000 };
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            // `cargo bench` passes `--bench`; nothing else is expected.
            if args[i] == "--events" {
                o.events = args
                    .get(i + 1)
                    .and_then(|n| n.parse().ok())
                    .expect("--events N");
                i += 1;
            }
            i += 1;
        }
        o
    }

    fn connect(dirs: &ProfileDirs) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return c,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    /// A raw connection that says hello, subscribes and never reads again.
    fn slow_subscriber(socket: &Path) -> UnixStream {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writeln!(
            stream,
            r#"{{"jsonrpc":"2.0","id":0,"method":"hello","params":{{"protocol":{PROTOCOL_VERSION},"client":"cli","client_version":"bench"}}}}"#
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.contains("\"result\""), "hello: {line}");
        writeln!(
            stream,
            r#"{{"jsonrpc":"2.0","id":1,"method":"events.subscribe"}}"#
        )
        .unwrap();
        stream
    }

    fn timings() -> Timings {
        let now = clock::monotonic_ns();
        Timings {
            batch_id: 1,
            t_recv: now,
            t_flush: now,
            t_computed: now,
            t_persisted: now,
            t_published: 0,
        }
    }

    /// Records the latency of a change event; `false` when nothing arrived.
    fn record(latencies: &mut Vec<u64>, note: Option<Notification>) -> bool {
        let published = note
            .as_ref()
            .and_then(|n| n.params["event"]["timings"]["t_published"].as_u64());
        if let Some(published) = published {
            latencies.push(clock::monotonic_ns() - published);
        }
        note.is_some()
    }

    struct Measured {
        received: usize,
        p50: f64,
        p95: f64,
        max: f64,
    }

    /// One attempt on a fresh daemon and profile.
    fn attempt(events: usize) -> Measured {
        let root = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(root.path().join("profile"));
        let daemon = Daemon::start(DaemonConfig {
            dirs: dirs.clone(),
            env: DaemonEnv::from_vars(Vec::new()),
            git: ResolveConfig {
                configured_path: None,
                path_env: None,
                known_locations: Vec::new(),
                shim_paths: Vec::new(),
                toolchain_gits: Vec::new(),
            },
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: ChannelConfig::default(),
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery: Default::default(),
            tm_capture: Default::default(),
        })
        .unwrap();
        let handle = daemon.shutdown_handle();
        let bus = daemon.events();
        let join = std::thread::spawn(move || daemon.run());

        let mut good = connect(&dirs);
        let _: SubscribeResult = good.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
        let socket = client::socket_path(&dirs).unwrap();
        let _slow = slow_subscriber(&socket);
        let deadline = Instant::now() + Duration::from_secs(5);
        while bus.subscriber_count() < 2 {
            assert!(
                Instant::now() < deadline,
                "the slow client never subscribed"
            );
            std::thread::yield_now();
        }
        let _flood: Vec<UnixStream> = (0..FLOOD)
            .map(|_| UnixStream::connect(&socket).unwrap())
            .collect();

        let mut latencies = Vec::with_capacity(events);
        let payload = "x".repeat(PAYLOAD);
        for i in 0..events {
            bus.publish(
                "git.event",
                json!({"n": i, "pad": payload}),
                Some(timings()),
                |_| {},
            );
            if i % 10 == 0 {
                // Drain the good client as a real one would.
                while record(
                    &mut latencies,
                    good.next_notification(Duration::from_millis(1)).unwrap(),
                ) {}
            }
        }
        let start = Instant::now();
        while latencies.len() < events && start.elapsed() < Duration::from_secs(10) {
            record(
                &mut latencies,
                good.next_notification(Duration::from_millis(50)).unwrap(),
            );
        }
        handle.request(StopCause::Signal("TERM"));
        let _ = join.join();

        assert_eq!(latencies.len(), events, "the good client lost events");
        latencies.sort_unstable();
        let ms =
            |q: usize| latencies[(latencies.len() * q / 100).min(latencies.len() - 1)] as f64 / 1e6;
        Measured {
            received: latencies.len(),
            p50: ms(50),
            p95: ms(95),
            max: *latencies.last().unwrap() as f64 / 1e6,
        }
    }

    pub fn run() -> i32 {
        let o = opts();
        println!(
            "channel flood: {} events of {PAYLOAD} B, 1 slow subscriber, {FLOOD} extra connections",
            o.events
        );
        let mut over = 0;
        for n in 1..=ATTEMPTS {
            let m = attempt(o.events);
            let verdict = if m.p95 < BUDGET_MS { "ok" } else { "OVER" };
            println!(
                "attempt {n}: received {}, p50 {:.2} ms, p95 {:.2} ms (budget {BUDGET_MS:.0}), max {:.2} ms: {verdict}",
                m.received, m.p50, m.p95, m.max
            );
            if m.p95 >= BUDGET_MS {
                over += 1;
            }
            // The first attempt under the budget settles it; one over is measured again.
            if over == 0 || over >= CONFIRMED {
                break;
            }
        }
        if over >= CONFIRMED {
            eprintln!(
                "channel flood: p95 over the {BUDGET_MS:.0} ms IPC budget of ADR-GRP-011 in {over} attempts"
            );
            return 1;
        }
        0
    }
}
