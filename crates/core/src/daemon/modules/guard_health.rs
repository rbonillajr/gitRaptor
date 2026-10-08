//! The check that the hook layer of every protected repo is still active (US-GRD-004,
//! ADR-GRD-005 § 4) as a module of the daemon.
//!
//! One thread. Every `check_every` (60 s with a jitter), and when a repo has a Git event (at
//! most every few seconds), it looks at the installs the loop published in the registry. It is
//! cheap by construction: a fingerprint of the stat of what the check reads first, and the full
//! check (gix and the hashes of the dispatchers) only when the fingerprint moved or every
//! few minutes. It never opens a repo store nor wakes a dormant repo: it only tells the loop
//! when the state it finds is not the one followed, and the loop logs and alerts.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::guard::HooksStatus;

use super::{Daemon, DaemonModule};
use crate::daemon::{HealthReport, ShutdownHandle};
use crate::guardrails::GuardRegistry;
use crate::guardrails::health;
use crate::guardrails::protection::{EVENT_EVERY, FULL_EVERY, Seen, check_every};

enum Msg {
    /// A Git event of this repo.
    Event(String),
    Stop,
}

struct GuardHealth {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

pub(super) fn start(daemon: &Daemon) -> Option<Box<dyn DaemonModule>> {
    let (tx, rx) = channel();
    let worker = Worker {
        registry: Arc::clone(&daemon.guard),
        handle: daemon.handle.clone(),
        every: check_every(),
        last_event: HashMap::new(),
    };
    let join = std::thread::Builder::new()
        .name("raptor-guard-health".into())
        .spawn(move || worker.run(&rx));
    let Ok(join) = join else {
        daemon.logger.warn("guard_health_unavailable", &[]);
        return None;
    };
    Some(Box::new(GuardHealth {
        tx,
        join: Some(join),
    }))
}

impl DaemonModule for GuardHealth {
    fn git_event(&self, repo_id: &str, _worktree: &std::path::Path, _seq: i64) {
        let _ = self.tx.send(Msg::Event(repo_id.to_owned()));
    }

    fn stop(mut self: Box<Self>) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct Worker {
    registry: Arc<GuardRegistry>,
    handle: ShutdownHandle,
    every: Duration,
    last_event: HashMap<String, Instant>,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        // The first check is the startup one: the loop already published the installs.
        self.check_all();
        loop {
            match rx.recv_timeout(self.wait()) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Msg::Event(repo_id)) => {
                    let now = Instant::now();
                    let due = self
                        .last_event
                        .get(&repo_id)
                        .is_none_or(|at| now.duration_since(*at) >= self.event_every());
                    if due {
                        self.last_event.insert(repo_id.clone(), now);
                        self.check(&repo_id);
                    }
                }
                Err(RecvTimeoutError::Timeout) => self.check_all(),
            }
        }
    }

    /// The interval and up to a tenth more, so the checks of many daemons do not line up.
    fn wait(&self) -> Duration {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        self.every + self.every.mul_f64(f64::from(nanos % 1000) / 10_000.0)
    }

    fn event_every(&self) -> Duration {
        EVENT_EVERY.min(self.every)
    }

    fn check_all(&self) {
        // Collected first: the registry's lock is not held while a repo is checked.
        let watched = self.registry.protection().watched();
        for (repo_id, _) in watched {
            self.check(&repo_id);
        }
    }

    fn check(&self, repo_id: &str) {
        let Some(watched) = self.registry.protection().get(repo_id) else {
            return;
        };
        let fingerprint = health::fingerprint(&watched.common, &watched.journal);
        let now = Instant::now();
        let seen = self.registry.protection().seen(repo_id);
        let full = seen.is_none_or(|s| {
            s.fingerprint != fingerprint || now.duration_since(s.full_at) >= FULL_EVERY
        });
        if !full {
            return;
        }
        let mut layer = health::check(&watched.common, Some(&watched.journal)).hooks;
        self.registry.protection().set_seen(
            repo_id,
            Seen {
                fingerprint,
                full_at: now,
            },
        );
        if self.registry.protection().current(repo_id) == layer {
            return;
        }
        if layer.status == HooksStatus::Inactive {
            // A tool that rewrites a file in place can be caught half way: read again before
            // saying the protection is gone.
            std::thread::sleep(self.every.min(Duration::from_secs(1)));
            layer = health::check(&watched.common, Some(&watched.journal)).hooks;
        }
        if self.registry.protection().current(repo_id) != layer {
            let _ = self.handle.guard_health(HealthReport {
                repo_id: repo_id.to_owned(),
            });
        }
    }
}
