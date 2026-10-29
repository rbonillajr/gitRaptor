//! The `utility` pool of the predictor (T007, ADR-CKP-001 § 5, ADR-GRP-015 § 1 and § 2): one
//! thread of [`Pool::Predictor`](gitraptor_api::resources::Pool) that takes the queued dry
//! merges one at a time, by priority, and runs them through a [`DryMerger`]. In power saving it
//! takes nothing (D5).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::resources::{Pool, WorkClass};
use gitraptor_git::dry_merge::{DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome};

use super::merger::DryMerger;
use super::queue::{PairQueue, Priority};
use super::worker::PAIR_DEADLINE;
use super::{PairKey, Triple};
use crate::power::PowerMode;
use crate::work_class;

/// Jobs taken at a time (§ Valores numéricos).
pub const PREDICT_CONCURRENCY: usize = 1;

/// Longest the thread waits on one signal before it looks at the other: a change of the
/// power-saving mode while it waits for work, or a stop while it waits for the mode. Far under
/// the 60 s in which a change of mode must apply.
const RECHECK: Duration = Duration::from_secs(1);

/// One dry merge to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The repo's common dir.
    pub repo: PathBuf,
    pub key: PairKey,
    pub triple: Triple,
}

/// The result of a [`Job`], sent back to the predictor.
#[derive(Debug)]
pub struct Done {
    pub key: PairKey,
    pub triple: Triple,
    pub result: Result<DryMergeOutcome, DryMergeError>,
    pub computed_at_utc_ms: i64,
}

/// What the pool's thread and its handle share.
struct Shared {
    state: Mutex<State>,
    /// Signalled on every submit and on stop.
    work: Condvar,
    paused: AtomicBool,
}

#[derive(Default)]
struct State {
    queue: PairQueue,
    /// The common dir of each queued pair's repo.
    repos: HashMap<PairKey, PathBuf>,
    stop: bool,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the thread does next.
enum Step {
    Run(Job),
    /// Nothing queued for a while: the merger may release its worker.
    Idle,
    /// Look at the power-saving mode again.
    Recheck,
    Stop,
}

/// The pool: its thread and its queue (one job per pair, the newest wins).
pub struct PredictPool {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl PredictPool {
    /// Starts the thread. Results go to `done`, in completion order.
    ///
    /// # Errors
    ///
    /// The thread could not be spawned.
    pub fn start(
        merger: Arc<dyn DryMerger>,
        power: PowerMode,
        done: Sender<Done>,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            work: Condvar::new(),
            paused: AtomicBool::new(power.is_saving()),
        });
        let thread_shared = Arc::clone(&shared);
        let thread = work_class::spawn(
            Pool::Predictor,
            WorkClass::Utility,
            "raptor-predict-pool",
            move || run(&thread_shared, merger.as_ref(), &power, &done),
        )?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Queues `job`, replacing a queued job of the same pair.
    pub fn submit(&self, job: Job, priority: Priority) {
        let mut state = self.shared.state();
        if state.stop {
            return;
        }
        state.repos.insert(job.key.clone(), job.repo);
        state.queue.push(job.key, job.triple, priority);
        drop(state);
        self.shared.work.notify_all();
    }

    /// Drops every queued job of `repo_id` (dormant or retired repo).
    pub fn forget_repo(&self, repo_id: &str) {
        let mut state = self.shared.state();
        state.queue.forget_repo(repo_id);
        state.repos.retain(|key, _| key.repo_id != repo_id);
    }

    /// Jobs waiting.
    pub fn queued(&self) -> usize {
        self.shared.state().queue.len()
    }

    /// The pool holds its queue because the power-saving mode is on.
    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Acquire)
    }

    /// Stops the thread once its current job ends; the queue is dropped.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for PredictPool {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state();
            state.stop = true;
            state.queue = PairQueue::default();
            state.repos.clear();
        }
        self.shared.work.notify_all();
        if let Some(thread) = self.thread.take() {
            // A panic of the pool's thread already ended its work; nothing to recover here.
            let _ = thread.join();
        }
    }
}

/// The pool's thread: one job at a time, nothing while the power-saving mode is on.
fn run(shared: &Shared, merger: &dyn DryMerger, power: &PowerMode, done: &Sender<Done>) {
    let mut seen = power.version();
    loop {
        if power.is_saving() {
            if !shared.paused.swap(true, Ordering::AcqRel) {
                // The worker's memory must not count while the pool holds its queue.
                merger.close();
            }
            if shared.state().stop {
                break;
            }
            seen = power.wait_change(seen, RECHECK);
            continue;
        }
        shared.paused.store(false, Ordering::Release);
        match next_step(shared, power) {
            Step::Stop => break,
            Step::Recheck => {}
            Step::Idle => {
                merger.close_if_idle(Instant::now());
            }
            Step::Run(job) => {
                if done.send(merge(merger, job)).is_err() {
                    // Nobody takes the results any more.
                    break;
                }
            }
        }
    }
    merger.close();
}

/// Waits for a job, a stop or the end of [`RECHECK`].
fn next_step(shared: &Shared, power: &PowerMode) -> Step {
    let mut state = shared.state();
    loop {
        if state.stop {
            return Step::Stop;
        }
        if power.is_saving() {
            return Step::Recheck;
        }
        if let Some((key, triple)) = state.queue.pop() {
            let Some(repo) = state.repos.remove(&key) else {
                // Every push records its repo first: a job without one cannot be run.
                continue;
            };
            return Step::Run(Job { repo, key, triple });
        }
        let (next, wait) = shared
            .work
            .wait_timeout(state, RECHECK)
            .unwrap_or_else(PoisonError::into_inner);
        state = next;
        if wait.timed_out() && state.queue.is_empty() && !state.stop {
            return Step::Idle;
        }
    }
}

fn merge(merger: &dyn DryMerger, job: Job) -> Done {
    let input = DryMergeInput {
        ours: job.triple.tip_a.clone(),
        theirs: job.triple.tip_b.clone(),
    };
    let deadline = Instant::now() + PAIR_DEADLINE;
    let result = merger.merge(&job.repo, &input, &DryMergeLimits::default(), deadline);
    Done {
        key: job.key,
        triple: job.triple,
        result,
        computed_at_utc_ms: crate::daemon::now_ms(),
    }
}

#[cfg(test)]
mod tests;
