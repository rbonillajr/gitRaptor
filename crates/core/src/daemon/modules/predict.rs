//! The conflict predictor as a module of the daemon.
//!
//! One thread, off the observer's path: the hooks only send a message. For each repo it keeps
//! the pairs and their cache, recomputing the pairs and their overlap when a worktree's tip
//! moves, its uncommitted paths change or the repo wakes. Each recompute opens a short-lived
//! reader for merge-bases and changed paths and drops it after. The dry merges run in the
//! predictor's pool, whose results come back to this thread as messages; every change of a
//! repo's pairs is published as `prediction.changed`, and a change of its known conflicts as
//! `repo.attention`, both recorded in the [`PredictionStore`] the channel reads.
//!
//! With the predictor off there is no pool: nothing is merged nor published. In power saving
//! every pair waits with its last result; back on mains the repos are planned again and only the
//! changed triples are queued. A dormant repo keeps no cache nor path sets, only its pairs' last
//! results as `Pending(Dormant)`; a retired repo forgets its pairs. A read error skips the
//! recompute and a panic drops the repos' state: neither takes the daemon down.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::event::REPO_ATTENTION;
use gitraptor_api::framing::MAX_MESSAGE_BYTES;
use gitraptor_api::methods::PREDICTION_CHANGED;
use gitraptor_api::prediction::{PairDetail, PredictionChanged, split_changed};
use gitraptor_api::scope::{AttentionView, RepoAttentionData, Scope};
use gitraptor_git::{ReadError, ReaderOptions, RepoReader};
use gitraptor_policy::team::BaseBranch;

use super::{Daemon, DaemonModule};
use crate::channel::EventBus;
use crate::daemon::{Logger, now_ms};
use crate::observe::{self, UncommittedPaths};
use crate::power::PowerMode;
use crate::predict::merger::{DryMerger, InProcessMerger};
use crate::predict::pool::{Done, Job, PredictPool};
use crate::predict::queue::Priority;
use crate::predict::state::RepoPairs;
use crate::predict::store::{self, PredictionStore};
use crate::predict::worker::WorkerMerger;
use crate::predict::{
    BaseInput, PairKey, PairSide, PendingReason, PredictMerger, WorktreeInput, pairs,
};
use crate::timemachine::protected::TmRepos;
use crate::watch::{ObserverHooks, Tier};

/// Longest the power watcher waits on one change before it looks at its stop flag. Far under
/// the 60 s in which a change of the power-saving mode must apply.
const POWER_RECHECK: Duration = Duration::from_secs(1);

/// Bytes a `prediction.changed` data may take: the frame less room for the notification around.
const CHANGED_BUDGET: usize = MAX_MESSAGE_BYTES - 64 * 1024;

enum Msg {
    /// A Git event of a worktree: its tip may have moved.
    GitEvent(String, PathBuf),
    Recomputed(String, PathBuf, UncommittedPaths),
    Tier(String, Tier),
    Retired(String),
    /// A dry merge of the pool ended.
    Done(Done),
    /// The power-saving mode changed: whether it is on now.
    Power(bool),
    /// Plan every observed repo: the initial calculation, since a repo's worktrees may never
    /// change.
    PlanAll,
    Stop,
}

struct Predict {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
    /// Forwards the pool's results to the module's thread; ends with the pool.
    bridge: Option<JoinHandle<()>>,
    /// Ends the power watcher, which is not waited for: it notices within [`POWER_RECHECK`].
    watcher_stop: Arc<AtomicBool>,
}

pub(super) fn start(daemon: &Daemon) -> Option<Box<dyn DaemonModule>> {
    let (tx, rx) = channel();
    let power = daemon.power();
    // Read before the mode, so a change in between is seen again by the watcher.
    let version = power.version();
    let saving = power.is_saving();
    let merger: Option<Arc<dyn DryMerger>> = match &daemon.config.predict.merger {
        PredictMerger::Off => {
            daemon.logger.info("predict_off", &[]);
            None
        }
        PredictMerger::InProcess => Some(Arc::new(InProcessMerger)),
        PredictMerger::Worker(config) => Some(Arc::new(WorkerMerger::new(config.clone()))),
    };
    let mut bridge = None;
    let pool = merger.and_then(|merger| {
        let (done_tx, done_rx) = channel::<Done>();
        let Ok(pool) = PredictPool::start(merger, power.clone(), done_tx) else {
            daemon.logger.warn("predict_pool_unavailable", &[]);
            return None;
        };
        let forward = tx.clone();
        bridge = std::thread::Builder::new()
            .name("raptor-predict-done".into())
            .spawn(move || {
                for done in done_rx {
                    if forward.send(Msg::Done(done)).is_err() {
                        break;
                    }
                }
            })
            .ok();
        // Without the bridge no result would come back: better no pool than a silent one.
        if bridge.is_none() {
            daemon.logger.warn("predict_pool_unavailable", &[]);
            return None;
        }
        Some(pool)
    });
    let watcher_stop = Arc::new(AtomicBool::new(false));
    if pool.is_some() {
        let (tx, stop) = (tx.clone(), Arc::clone(&watcher_stop));
        let power = power.clone();
        let watcher = std::thread::Builder::new()
            .name("raptor-predict-power".into())
            .spawn(move || watch_power(&power, version, saving, &tx, &stop));
        if watcher.is_err() {
            // The pool still pauses itself; only the pairs' `power-saving` state is lost.
            daemon.logger.warn("predict_power_watch_unavailable", &[]);
        }
    }
    let worker = Worker {
        tm: Arc::clone(&daemon.tm),
        logger: daemon.logger.clone(),
        bus: Arc::clone(&daemon.bus),
        store: Arc::clone(&daemon.prediction),
        saving: saving && pool.is_some(),
        pool,
        repos: HashMap::new(),
        deferred: BTreeMap::new(),
    };
    let join = std::thread::Builder::new()
        .name("raptor-predict".into())
        .spawn(move || worker.run(&rx));
    let Ok(join) = join else {
        watcher_stop.store(true, Ordering::Release);
        daemon.logger.warn("predict_unavailable", &[]);
        return None;
    };
    Some(Box::new(Predict {
        tx,
        join: Some(join),
        bridge,
        watcher_stop,
    }))
}

/// Sends the module every change of the power-saving mode, until `stop`.
fn watch_power(
    power: &PowerMode,
    mut seen: u64,
    mut saving: bool,
    tx: &Sender<Msg>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Acquire) {
        seen = power.wait_change(seen, POWER_RECHECK);
        let now = power.is_saving();
        if now != saving {
            saving = now;
            if tx.send(Msg::Power(now)).is_err() {
                break;
            }
        }
    }
}

impl DaemonModule for Predict {
    fn serving(&self) {
        // The initial calculation starts once clients can watch it, so they see it go from
        // calculating to its results.
        let _ = self.tx.send(Msg::PlanAll);
    }

    fn repo_retired(&self, repo_id: &str) {
        let _ = self.tx.send(Msg::Retired(repo_id.to_owned()));
    }

    fn observer_hooks(&self) -> Option<Arc<dyn ObserverHooks>> {
        Some(Arc::new(Hooks(self.tx.clone())))
    }

    fn git_event(&self, repo_id: &str, worktree: &Path, _seq: i64) {
        let _ = self
            .tx
            .send(Msg::GitEvent(repo_id.to_owned(), worktree.to_owned()));
    }

    fn stop(mut self: Box<Self>) {
        self.watcher_stop.store(true, Ordering::Release);
        let _ = self.tx.send(Msg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        // The pool went with the module's thread, and with it the bridge's source.
        if let Some(bridge) = self.bridge.take() {
            let _ = bridge.join();
        }
    }
}

/// What the observer tells the predictor: a message each, never work on its thread.
struct Hooks(Sender<Msg>);

impl ObserverHooks for Hooks {
    fn worktree_touched(&self, _repo_id: &str, _root: &Path) {}

    fn git_dir_touched(&self, _repo_id: &str, _t_recv: u64) {}

    fn worktree_recomputed(&self, repo_id: &str, root: &Path, paths: &UncommittedPaths) {
        let _ = self.0.send(Msg::Recomputed(
            repo_id.to_owned(),
            root.to_owned(),
            paths.clone(),
        ));
    }

    fn repo_tier_changed(&self, repo_id: &str, tier: Tier) {
        let _ = self.0.send(Msg::Tier(repo_id.to_owned(), tier));
    }
}

/// What the predictor keeps of an active repo.
struct RepoState {
    pairs: RepoPairs,
    /// The last uncommitted paths of each worktree, by canonical root.
    uncommitted: HashMap<PathBuf, UncommittedPaths>,
    dormant: bool,
}

struct Worker {
    tm: Arc<TmRepos>,
    logger: Logger,
    bus: Arc<EventBus>,
    store: Arc<PredictionStore>,
    /// `None` with the predictor off: nothing is merged nor published.
    pool: Option<PredictPool>,
    /// The power-saving mode is on: nothing is planned, every pair waits.
    saving: bool,
    repos: HashMap<String, RepoState>,
    /// The worktrees that changed while saving, by repo: planned again back on mains.
    deferred: BTreeMap<String, BTreeSet<PathBuf>>,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        while let Ok(first) = rx.recv() {
            // A burst is handled as one: every message first, then one recompute and one
            // publication per repo.
            let mut dirty: BTreeMap<String, BTreeSet<PathBuf>> = BTreeMap::new();
            let mut touched: BTreeSet<String> = BTreeSet::new();
            for msg in std::iter::once(first).chain(rx.try_iter()) {
                if matches!(msg, Msg::Stop) {
                    return;
                }
                self.guarded(|w| w.take(msg, &mut dirty, &mut touched));
            }
            for (repo_id, changed) in dirty {
                let id = repo_id.clone();
                self.guarded(move |w| w.recompute(&id, &changed));
                if let Some(state) = self.repos.get(&repo_id)
                    && state.pairs.pairs().next().is_none()
                    && state.uncommitted.is_empty()
                {
                    self.repos.remove(&repo_id);
                }
                touched.insert(repo_id);
            }
            for repo_id in touched {
                self.guarded(|w| w.publish(&repo_id));
            }
        }
    }

    /// Runs `f`; a panic inside it drops every repo's state rather than keep one half updated.
    fn guarded(&mut self, f: impl FnOnce(&mut Self)) {
        if catch_unwind(AssertUnwindSafe(|| f(self))).is_err() {
            self.repos.clear();
            self.logger.warn("predict_panicked", &[]);
        }
    }

    /// Applies one message; the repos to recompute go to `dirty`, with the worktrees whose tip
    /// may have moved, and the repos whose pairs changed to `touched`.
    fn take(
        &mut self,
        msg: Msg,
        dirty: &mut BTreeMap<String, BTreeSet<PathBuf>>,
        touched: &mut BTreeSet<String>,
    ) {
        match msg {
            Msg::GitEvent(repo_id, worktree) => {
                if self.repos.get(&repo_id).is_some_and(|s| s.dormant) {
                    return;
                }
                dirty
                    .entry(repo_id)
                    .or_default()
                    .insert(observe::canonical(&worktree));
            }
            Msg::Recomputed(repo_id, root, paths) => {
                let state = self.state(&repo_id);
                if state.dormant {
                    return;
                }
                state.uncommitted.insert(observe::canonical(&root), paths);
                dirty.entry(repo_id).or_default();
            }
            Msg::Tier(repo_id, Tier::Dormant) => {
                if let Some(pool) = &self.pool {
                    pool.forget_repo(&repo_id);
                }
                if let Some(state) = self.repos.get_mut(&repo_id) {
                    state.pairs.suspend(PendingReason::Dormant);
                    state.uncommitted = HashMap::new();
                    state.dormant = true;
                    touched.insert(repo_id.clone());
                }
                self.deferred.remove(&repo_id);
                dirty.remove(&repo_id);
            }
            Msg::Tier(repo_id, Tier::Active) => {
                self.state(&repo_id).dormant = false;
                dirty.entry(repo_id).or_default();
            }
            Msg::PlanAll => {
                let observed = self.bus.snapshot().1.repos;
                for repo in observed {
                    if !self.repos.get(&repo.repo_id).is_some_and(|s| s.dormant) {
                        dirty.entry(repo.repo_id).or_default();
                    }
                }
            }
            Msg::Tier(_, Tier::Waking) | Msg::Stop => {}
            Msg::Retired(repo_id) => {
                if let Some(pool) = &self.pool {
                    pool.forget_repo(&repo_id);
                }
                self.repos.remove(&repo_id);
                self.deferred.remove(&repo_id);
                self.store.forget(&repo_id);
                dirty.remove(&repo_id);
                touched.remove(&repo_id);
            }
            Msg::Done(done) => {
                let repo_id = done.key.repo_id.clone();
                if let Some(state) = self.repos.get_mut(&repo_id)
                    && state.pairs.complete(
                        &done.key,
                        &done.triple,
                        done.result,
                        done.computed_at_utc_ms,
                    )
                {
                    touched.insert(repo_id);
                }
            }
            Msg::Power(saving) => {
                self.saving = saving;
                let active = self.repos.iter_mut().filter(|(_, s)| !s.dormant);
                if saving {
                    for (repo_id, state) in active {
                        state.pairs.suspend(PendingReason::PowerSaving);
                        touched.insert(repo_id.clone());
                    }
                } else {
                    for (repo_id, _) in active {
                        dirty.entry(repo_id.clone()).or_default();
                    }
                    for (repo_id, changed) in std::mem::take(&mut self.deferred) {
                        dirty.entry(repo_id).or_default().extend(changed);
                    }
                }
            }
        }
    }

    fn state(&mut self, repo_id: &str) -> &mut RepoState {
        self.repos
            .entry(repo_id.to_owned())
            .or_insert_with(|| RepoState {
                pairs: RepoPairs::new(repo_id),
                uncommitted: HashMap::new(),
                dormant: false,
            })
    }

    /// Plans the repo again and queues the pairs whose triple changed; in power saving it only
    /// remembers which worktrees changed.
    fn recompute(&mut self, repo_id: &str, changed: &BTreeSet<PathBuf>) {
        if self.saving {
            self.deferred
                .entry(repo_id.to_owned())
                .or_default()
                .extend(changed.iter().cloned());
            return;
        }
        let Some(common) = self.tm.common_dir(repo_id) else {
            return;
        };
        if self.repos.get(repo_id).is_some_and(|s| s.dormant) {
            return;
        }
        let uncommitted = self
            .repos
            .get(repo_id)
            .map(|s| s.uncommitted.clone())
            .unwrap_or_default();
        let base = self.base(repo_id);
        let (plan, sessions) = match read_plan(&common, repo_id, &uncommitted, &base) {
            Ok(plan) => plan,
            Err(_) => {
                self.logger.warn("predict_read_failed", &[]);
                return;
            }
        };
        let queued = self.state(repo_id).pairs.apply(plan, now_ms());
        let Some(pool) = &self.pool else {
            return;
        };
        let repo = observe::canonical(&common);
        for (key, triple) in queued {
            let p = priority(&key, changed, &sessions);
            let job = Job {
                repo: repo.clone(),
                key,
                triple,
            };
            pool.submit(job, p);
        }
    }

    /// The repo's base branch as the engine last read it: the confirmed one once confirmed.
    fn base(&self, repo_id: &str) -> BaseBranch {
        let scope = Scope::Repo {
            repo_id: repo_id.to_owned(),
        };
        self.bus
            .scope_read(&scope, |_, engine| {
                engine.divergence.get(repo_id).map(|d| d.base.clone())
            })
            .flatten()
            .unwrap_or_else(|| observe::base_branch(None))
    }

    /// Publishes what changed in the repo's pairs since the last publication, in batches that
    /// fit a frame, then its known conflicts if they changed. Nothing with the predictor off.
    fn publish(&self, repo_id: &str) {
        if self.pool.is_none() {
            return;
        }
        let published = self.store.details(repo_id);
        let fresh: Vec<(PairKey, PairDetail)> = self
            .repos
            .get(repo_id)
            .map(|s| {
                s.pairs
                    .pairs()
                    .map(|p| (p.key.clone(), store::detail(p, 0)))
                    .collect()
            })
            .unwrap_or_default();
        let mut removed: Vec<PairKey> = published
            .keys()
            .filter(|k| !fresh.iter().any(|(f, _)| f == *k))
            .cloned()
            .collect();
        let removed_ids = removed
            .iter()
            .filter_map(|k| published.get(k).map(|d| d.summary.pair_id.clone()))
            .collect();
        let mut summaries = Vec::new();
        let mut by_id: HashMap<String, (PairKey, PairDetail)> = HashMap::new();
        for (key, mut detail) in fresh {
            if let Some(old) = published.get(&key) {
                detail.summary.rev = old.summary.rev;
                if *old == detail {
                    continue;
                }
            }
            summaries.push(detail.summary.clone());
            by_id.insert(detail.summary.pair_id.clone(), (key, detail));
        }
        if !summaries.is_empty() || !removed.is_empty() {
            let changed = PredictionChanged {
                repo_id: repo_id.to_owned(),
                pairs: summaries,
                removed: removed_ids,
            };
            let scope = Scope::Repo {
                repo_id: repo_id.to_owned(),
            };
            for mut batch in split_changed(changed, CHANGED_BUDGET) {
                let mut entries: Vec<(PairKey, PairDetail)> = batch
                    .pairs
                    .iter()
                    .filter_map(|p| by_id.remove(&p.pair_id))
                    .collect();
                let gone = std::mem::take(&mut removed);
                let store = &self.store;
                // The rev is the batch's own scope_seq, recorded under the lock that assigns it.
                self.bus
                    .publish_in_scope(PREDICTION_CHANGED, &scope, |_, rev| {
                        for p in &mut batch.pairs {
                            p.rev = rev;
                        }
                        for (_, d) in &mut entries {
                            d.summary.rev = rev;
                        }
                        store.record(repo_id, entries, &gone);
                        batch
                    });
            }
        }
        let conflicts = store::conflicts(&self.store.summaries(repo_id));
        let attention = self.store.attention(repo_id);
        if attention.conflicts != conflicts {
            let store = &self.store;
            let data = RepoAttentionData {
                repo_id: repo_id.to_owned(),
                attention: AttentionView {
                    conflicts,
                    ..attention
                },
            };
            self.bus.publish_with(REPO_ATTENTION, None, |_| {
                store.record_conflicts(repo_id, conflicts);
                data
            });
        }
    }
}

/// A plan of the repo at `common` against `base`, read with readers dropped before it returns,
/// and the roots with a session present.
fn read_plan(
    common: &Path,
    repo_id: &str,
    uncommitted: &HashMap<PathBuf, UncommittedPaths>,
    base: &BaseBranch,
) -> Result<(Vec<crate::predict::PairPrediction>, BTreeSet<PathBuf>), ReadError> {
    let reader = RepoReader::open(common, &ReaderOptions::default())?;
    let common = observe::canonical(common);
    let mut roots: Vec<(PathBuf, bool, Option<String>)> = Vec::new();
    if !reader.is_bare()
        && let Some(main) = reader.workdir()
    {
        roots.push((observe::canonical(&main), true, None));
    }
    for w in reader.worktrees()? {
        roots.push((observe::canonical(&w.path), false, Some(w.id)));
    }
    let base_name = base.name.clone();
    let mut inputs = Vec::new();
    let mut checked_out_at = None;
    for (root, main, admin) in roots {
        // A worktree that cannot be read now is left out of this plan, not guessed.
        let Ok(wt) = observe::open_worktree(&common, &root, main, admin.as_deref()) else {
            continue;
        };
        let Ok(head) = wt.head() else {
            continue;
        };
        let in_progress = wt.in_progress().is_some();
        if !head.detached
            && head.branch.is_some()
            && head.branch.as_deref() == base_name.as_ref().map(|n| n.as_str())
        {
            checked_out_at = Some(root.clone());
        }
        inputs.push(WorktreeInput {
            uncommitted: uncommitted.get(&root).cloned().unwrap_or_default(),
            root,
            tip: head.commit,
            in_progress,
            session_present: false,
        });
    }
    let tip = match &base_name {
        Some(name) => reader.resolve_ref(name)?,
        None => None,
    };
    let base = BaseInput {
        status: observe::base_view(base).status,
        tip,
        checked_out_at,
    };
    let sessions = inputs
        .iter()
        .filter(|w| w.session_present)
        .map(|w| w.root.clone())
        .collect();
    let plan = pairs::plan(&reader, repo_id, &base, &inputs)?;
    Ok((plan, sessions))
}

/// First the pairs against the base of a worktree that changed, then those with a session
/// present, then the rest. In the initial calculation nothing changed, so sessions go first.
fn priority(key: &PairKey, changed: &BTreeSet<PathBuf>, sessions: &BTreeSet<PathBuf>) -> Priority {
    let other = match &key.b {
        PairSide::Base => None,
        PairSide::Worktree(b) => Some(b),
    };
    if other.is_none() && changed.contains(&key.a) {
        Priority::ChangedVsBase
    } else if sessions.contains(&key.a) || other.is_some_and(|b| sessions.contains(b)) {
        Priority::PresentSession
    } else {
        Priority::Rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(a: &str, b: Option<&str>) -> PairKey {
        PairKey {
            repo_id: "r".into(),
            a: PathBuf::from(a),
            b: b.map_or(PairSide::Base, |b| PairSide::Worktree(PathBuf::from(b))),
        }
    }

    #[test]
    fn the_changed_worktree_against_the_base_goes_first_then_sessions() {
        let changed: BTreeSet<PathBuf> = [PathBuf::from("/w1")].into();
        let sessions: BTreeSet<PathBuf> = [PathBuf::from("/w2")].into();
        let p = |k| priority(&k, &changed, &sessions);
        assert_eq!(p(key("/w1", None)), Priority::ChangedVsBase);
        assert_eq!(p(key("/w1", Some("/w2"))), Priority::PresentSession);
        assert_eq!(p(key("/w2", None)), Priority::PresentSession);
        assert_eq!(p(key("/w3", None)), Priority::Rest);
        assert_eq!(p(key("/w1", Some("/w3"))), Priority::Rest);
    }
}
