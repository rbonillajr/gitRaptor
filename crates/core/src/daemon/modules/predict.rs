//! The conflict predictor as a module of the daemon.
//!
//! One thread, off the observer's path: the hooks only send a message. For each repo it keeps
//! the pairs, their cache and the queue of dry merges, recomputing the pairs and their overlap
//! when a worktree's tip moves, its uncommitted paths change or the repo wakes. Each recompute
//! opens a short-lived reader for merge-bases and changed paths and drops it after. Nothing here
//! runs a dry merge or launches a process: the queue waits for the pool that runs it, and
//! nothing is published yet.
//!
//! A dormant repo keeps no cache nor path sets, only its pairs' last results as
//! `Pending(Dormant)`; a retired repo forgets its pairs. A read error skips the recompute and a
//! panic drops the repo's state: neither takes the daemon down.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use gitraptor_git::{ReadError, ReaderOptions, RepoReader};

use super::{Daemon, DaemonModule};
use crate::daemon::{Logger, now_ms};
use crate::observe::{self, UncommittedPaths};
use crate::predict::queue::{PairQueue, Priority};
use crate::predict::state::RepoPairs;
use crate::predict::{BaseInput, PairKey, PairSide, PendingReason, WorktreeInput, pairs};
use crate::timemachine::protected::TmRepos;
use crate::watch::{ObserverHooks, Tier};

enum Msg {
    /// A Git event of a worktree: its tip may have moved.
    GitEvent(String, PathBuf),
    Recomputed(String, PathBuf, UncommittedPaths),
    Tier(String, Tier),
    Retired(String),
    Stop,
}

struct Predict {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

pub(super) fn start(daemon: &Daemon) -> Option<Box<dyn DaemonModule>> {
    let (tx, rx) = channel();
    let worker = Worker {
        tm: Arc::clone(&daemon.tm),
        logger: daemon.logger.clone(),
        repos: HashMap::new(),
    };
    let join = std::thread::Builder::new()
        .name("raptor-predict".into())
        .spawn(move || worker.run(&rx));
    let Ok(join) = join else {
        daemon.logger.warn("predict_unavailable", &[]);
        return None;
    };
    Some(Box::new(Predict {
        tx,
        join: Some(join),
    }))
}

impl DaemonModule for Predict {
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
        let _ = self.tx.send(Msg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
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
    queue: PairQueue,
    /// The last uncommitted paths of each worktree, by canonical root.
    uncommitted: HashMap<PathBuf, UncommittedPaths>,
    dormant: bool,
}

struct Worker {
    tm: Arc<TmRepos>,
    logger: Logger,
    repos: HashMap<String, RepoState>,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Msg>) {
        while let Ok(first) = rx.recv() {
            // A burst is handled as one: every message first, then one recompute per repo.
            let mut dirty: BTreeMap<String, BTreeSet<PathBuf>> = BTreeMap::new();
            for msg in std::iter::once(first).chain(rx.try_iter()) {
                if matches!(msg, Msg::Stop) {
                    return;
                }
                self.guarded(|w| w.take(msg, &mut dirty));
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
    /// may have moved.
    fn take(&mut self, msg: Msg, dirty: &mut BTreeMap<String, BTreeSet<PathBuf>>) {
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
                if let Some(state) = self.repos.get_mut(&repo_id) {
                    state.pairs.suspend(PendingReason::Dormant);
                    state.queue = PairQueue::default();
                    state.uncommitted = HashMap::new();
                    state.dormant = true;
                }
                dirty.remove(&repo_id);
            }
            Msg::Tier(repo_id, Tier::Active) => {
                self.state(&repo_id).dormant = false;
                dirty.entry(repo_id).or_default();
            }
            Msg::Tier(_, Tier::Waking) | Msg::Stop => {}
            Msg::Retired(repo_id) => {
                self.repos.remove(&repo_id);
                dirty.remove(&repo_id);
            }
        }
    }

    fn state(&mut self, repo_id: &str) -> &mut RepoState {
        self.repos
            .entry(repo_id.to_owned())
            .or_insert_with(|| RepoState {
                pairs: RepoPairs::new(repo_id),
                queue: PairQueue::default(),
                uncommitted: HashMap::new(),
                dormant: false,
            })
    }

    /// Plans the repo again and queues the pairs whose triple changed.
    fn recompute(&mut self, repo_id: &str, changed: &BTreeSet<PathBuf>) {
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
        let plan = match read_plan(&common, repo_id, &uncommitted) {
            Ok(plan) => plan,
            Err(_) => {
                self.logger.warn("predict_read_failed", &[]);
                return;
            }
        };
        let (plan, sessions) = plan;
        let state = self.state(repo_id);
        for (key, triple) in state.pairs.apply(plan, now_ms()) {
            let p = priority(&key, changed, &sessions);
            state.queue.push(key, triple, p);
        }
    }
}

/// A plan of the repo at `common`, read with readers dropped before it returns, and the roots
/// with a session present.
fn read_plan(
    common: &Path,
    repo_id: &str,
    uncommitted: &HashMap<PathBuf, UncommittedPaths>,
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
    // The confirmed base lives in the repo's store, which the daemon loop owns: until it is
    // handed over, the base counts as unconfirmed and its pairs wait (never a wrong "current").
    let base = observe::base_branch(None);
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
        status: observe::base_view(&base).status,
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
