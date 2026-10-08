//! XP-01 (DS-TS-GRP-004 § 8): the channel over the Windows named pipe, with a
//! real daemon (in-process) and the real client library, over a temporary
//! profile (NFR-01). The pipe's own guarantees (DACL, first instance,
//! deadlines) are tested in `gitraptor-winsys`.
#![cfg(windows)]

mod common;

use std::io::{BufRead, BufReader};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::messages::{ClientKind, Snapshot, SubscribeResult};
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, Timings, clock, methods};
use gitraptor_core::channel::{ChannelConfig, ChannelLimits, EventBus, transport};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use gitraptor_winsys::pipe::PipeListener;
use serde_json::json;

fn no_git() -> ResolveConfig {
    ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    }
}

fn config(dirs: &ProfileDirs, channel: ChannelConfig) -> DaemonConfig {
    DaemonConfig {
        dirs: dirs.clone(),
        env: DaemonEnv::from_vars(Vec::new()),
        git: no_git(),
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel,
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        tm_capture: Default::default(),
    }
}

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    bus: std::sync::Arc<EventBus>,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs, channel: ChannelConfig) -> Self {
        let daemon = Daemon::start(config(&dirs, channel)).unwrap();
        let handle = daemon.shutdown_handle();
        let bus = daemon.events();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            dirs,
            handle,
            bus,
            join: Some(join),
        }
    }

    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(client) => return client,
                Err(ClientError::NotRunning) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::yield_now();
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    fn runtime(&self) -> std::path::PathBuf {
        self.dirs.runtime.clone().unwrap()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn change_timings(batch: u64) -> Timings {
    let now = clock::monotonic_ns();
    Timings {
        batch_id: batch,
        t_recv: now,
        t_flush: now,
        t_computed: now,
        t_persisted: now,
        t_published: 0,
    }
}

#[test]
fn a_client_greets_calls_and_receives_events_over_the_pipe() {
    let tp = TempProfile::new();
    let r = Running::start(tp.dirs(), ChannelConfig::default());
    let mut client = r.client();
    assert_eq!(client.hello().daemon_pid, std::process::id());
    let pong: String = client.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");

    let snap: Snapshot = client.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
    let _: SubscribeResult = client
        .call(
            methods::EVENTS_SUBSCRIBE,
            json!({"from_seq": snap.seq + 1, "run_id": snap.run_id}),
        )
        .unwrap();
    for n in 0..20 {
        r.bus.publish(
            "git.event",
            json!({"n": n}),
            Some(change_timings(n)),
            |_| {},
        );
    }
    let mut seqs = Vec::new();
    while seqs.len() < 20 {
        let note = client
            .next_notification(Duration::from_secs(5))
            .unwrap()
            .expect("event");
        seqs.push(note.params["event"]["seq"].as_u64().unwrap());
    }
    let expected: Vec<u64> = (snap.seq + 1..=snap.seq + 20).collect();
    assert_eq!(seqs, expected);
}

#[test]
fn a_pipe_name_taken_beforehand_makes_the_daemon_fail_closed() {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let name = transport::pipe_path(dirs.runtime.as_deref().unwrap()).unwrap();
    // A squatter's pipe, open to everyone, created before the daemon.
    let _squatter = PipeListener::bind(&name, "D:(A;;GA;;;WD)", 4).unwrap();
    assert!(Daemon::start(config(&dirs, ChannelConfig::default())).is_err());
    let log = std::fs::read_to_string(dirs.state.join("daemon.log")).unwrap_or_default();
    assert!(log.contains("channel_bind_failed"), "{log}");
}

#[test]
fn a_pipe_this_user_cannot_open_is_rejected_by_the_client() {
    let tp = TempProfile::new();
    let dirs = tp.dirs();
    let name = transport::pipe_path(dirs.runtime.as_deref().unwrap()).unwrap();
    // A pipe whose DACL is someone else's: only SYSTEM may open it.
    let _foreign = PipeListener::bind(&name, "D:P(A;;GA;;;SY)", 4).unwrap();
    match Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION) {
        Err(ClientError::ChannelRejected) => {}
        Err(other) => panic!("expected a rejection, got {other}"),
        Ok(_) => panic!("connected to a foreign pipe"),
    }
}

#[test]
fn no_daemon_is_not_running() {
    let tp = TempProfile::new();
    assert!(matches!(
        Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION),
        Err(ClientError::NotRunning)
    ));
}

#[test]
fn connections_beyond_the_limit_are_refused_and_the_pipe_stays_usable() {
    let tp = TempProfile::new();
    let limits = ChannelLimits {
        max_connections: 2,
        terminal_slots: 0,
        per_client: 64,
        ..ChannelLimits::default()
    };
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            limits,
            ..ChannelConfig::default()
        },
    );
    let mut first = r.client();
    let runtime = r.runtime();
    // Raw connections that never greet: they hold their slot until the
    // handshake deadline.
    let mut held = Vec::new();
    let mut limited = 0;
    for _ in 0..6 {
        let stream = transport::connect(&runtime).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() && !line.is_empty() {
            let message: serde_json::Value = serde_json::from_str(&line).unwrap();
            if message["error"]["code"] == code::LIMIT_REACHED {
                limited += 1;
            }
        }
        held.push(stream);
    }
    assert!(
        limited >= 4,
        "the connection limit never applied ({limited})"
    );
    let pong: String = first.call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
    // Once they are gone, new clients get in again.
    drop(held);
    drop(first);
    let start = Instant::now();
    loop {
        match Client::connect(&r.dirs, ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(_) => break,
            Err(_) if start.elapsed() < Duration::from_secs(5) => std::thread::yield_now(),
            Err(err) => panic!("the pipe is not usable again: {err}"),
        }
    }
}
