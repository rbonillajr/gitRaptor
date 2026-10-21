// TS-CKP-001 T003: pair states, the per-triple cache and the coalescing queue. Temporary repos
// only (NFR-01).

use std::path::{Path, PathBuf};

use gitraptor_api::messages::BaseStatusView;
use gitraptor_core::observe::UncommittedPaths;
use gitraptor_core::predict::queue::{PairQueue, Priority};
use gitraptor_core::predict::state::RepoPairs;
use gitraptor_core::predict::{
    BaseInput, PairKey, PairPrediction, PairSide, PairState, PendingReason, Triple, WorktreeInput,
};
use gitraptor_git::dry_merge::{DryMergeOutcome, MergeBaseUsed};
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

const REPO_ID: &str = "r";

struct Scene {
    f: Fixture,
    w1: PathBuf,
    w2: PathBuf,
}

impl Scene {
    /// `w1` changed `x.txt`, `w2` changed `y.txt`: pairs (w1,base), (w2,base) and (w1,w2).
    fn new() -> Self {
        let f = Fixture::with_commit(&git_from_path());
        f.git(&["branch", "w1", "main"]);
        f.git(&["branch", "w2", "main"]);
        let w1 = f.add_worktree("w1", "w1");
        let w2 = f.add_worktree("w2", "w2");
        let s = Self { f, w1, w2 };
        s.commit(&s.w1, "x.txt", "1\n");
        s.commit(&s.w2, "y.txt", "2\n");
        s
    }

    fn commit(&self, dir: &Path, file: &str, content: &str) {
        std::fs::write(dir.join(file), content).unwrap();
        self.f.git_in(dir, &["add", file]);
        self.f
            .git_in(dir, &["commit", "-q", "-m", &format!("{file} {content}")]);
    }

    fn tip(&self, dir: &Path) -> String {
        self.f.git_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn input(&self, root: &Path, uncommitted: &[&str], in_progress: bool) -> WorktreeInput {
        WorktreeInput {
            root: root.to_owned(),
            tip: Some(self.tip(root)),
            uncommitted: UncommittedPaths::from_paths(
                uncommitted.iter().map(|s| (*s).to_owned()),
                10_000,
            ),
            in_progress,
            session_present: false,
        }
    }

    fn plan_with(&self, w1: WorktreeInput, w2: WorktreeInput) -> Vec<PairPrediction> {
        let base = BaseInput {
            status: BaseStatusView::Confirmed,
            tip: Some(self.tip(&self.f.repo)),
            checked_out_at: Some(self.f.repo.clone()),
        };
        let reader = RepoReader::open(&self.f.repo, &ReaderOptions::default()).unwrap();
        gitraptor_core::predict::pairs::plan(&reader, REPO_ID, &base, &[w1, w2]).unwrap()
    }

    fn plan(&self) -> Vec<PairPrediction> {
        self.plan_with(
            self.input(&self.w1, &[], false),
            self.input(&self.w2, &[], false),
        )
    }
}

fn outcome() -> DryMergeOutcome {
    DryMergeOutcome {
        merge_base: MergeBaseUsed::None,
        files: Vec::new(),
        truncated: false,
        rewrites_escalated: false,
    }
}

fn involves(key: &PairKey, w: &Path) -> bool {
    key.a == w || matches!(&key.b, PairSide::Worktree(b) if b == w)
}

fn vs_base<'a>(pairs: &'a RepoPairs, w: &Path) -> &'a PairPrediction {
    pairs
        .pairs()
        .find(|p| p.key.a == w && matches!(p.key.b, PairSide::Base))
        .expect("pair against the base")
}

fn between<'a>(pairs: &'a RepoPairs, w1: &Path, w2: &Path) -> &'a PairPrediction {
    pairs
        .pairs()
        .find(|p| {
            matches!(p.key.b, PairSide::Worktree(_)) && involves(&p.key, w1) && involves(&p.key, w2)
        })
        .expect("pair between worktrees")
}

mod stale {
    use super::*;

    #[test]
    fn a_result_of_stale_inputs_is_never_current() {
        let s = Scene::new();
        let mut pairs = RepoPairs::new(REPO_ID);
        let queued = pairs.apply(s.plan(), 1_000);
        let (key, old) = queued
            .iter()
            .find(|(k, _)| k.a == s.w1 && matches!(k.b, PairSide::Base))
            .cloned()
            .unwrap();

        // The worktree moves on while the job runs.
        s.commit(&s.w1, "x2.txt", "again\n");
        let requeued = pairs.apply(s.plan(), 2_000);
        let (_, new) = requeued.iter().find(|(k, _)| *k == key).cloned().unwrap();
        assert_ne!(old, new);

        // The old job finishes: discarded, the pair is not current.
        assert!(!pairs.complete(&key, &old, Ok(outcome()), 3_000));
        let state = &pairs.get(&key).unwrap().state;
        assert!(!matches!(state, PairState::Current));
        assert!(pairs.get(&key).unwrap().conflict.is_none());

        // The result of the current triple is accepted.
        assert!(pairs.complete(&key, &new, Ok(outcome()), 4_000));
        assert!(matches!(pairs.get(&key).unwrap().state, PairState::Current));
    }
}

mod cache {
    use super::*;

    #[test]
    fn ckp001_cache_first_apply_queues_every_calculable_pair_and_the_same_plan_queues_none() {
        let s = Scene::new();
        let mut pairs = RepoPairs::new(REPO_ID);
        let first = pairs.apply(s.plan(), 1_000);
        assert_eq!(first.len(), 3);
        assert_eq!(pairs.pairs().count(), 3);
        let again = pairs.apply(s.plan(), 2_000);
        assert!(again.is_empty());
    }

    #[test]
    fn ckp001_cache_a_new_triple_queues_the_pair_and_recalculates_what_had_a_result() {
        let s = Scene::new();
        let mut pairs = RepoPairs::new(REPO_ID);
        for (k, t) in pairs.apply(s.plan(), 1_000) {
            assert!(pairs.complete(&k, &t, Ok(outcome()), 1_500));
        }
        assert!(pairs.pairs().all(|p| matches!(p.state, PairState::Current)));

        s.commit(&s.w1, "x2.txt", "again\n");
        let queued = pairs.apply(s.plan(), 2_000);
        let keys: Vec<_> = queued.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys.len(), 2, "(w1,base) and (w1,w2) only");
        assert!(keys.iter().all(|k| involves(k, &s.w1)));
        assert!(matches!(
            vs_base(&pairs, &s.w1).state,
            PairState::Recalculating
        ));
        assert!(matches!(
            between(&pairs, &s.w1, &s.w2).state,
            PairState::Recalculating
        ));
        assert!(matches!(vs_base(&pairs, &s.w2).state, PairState::Current));
    }

    #[test]
    fn ckp001_cache_a_change_of_uncommitted_paths_only_recomputes_the_overlap_without_queueing() {
        let s = Scene::new();
        let mut pairs = RepoPairs::new(REPO_ID);
        pairs.apply(s.plan(), 1_000);
        assert!(between(&pairs, &s.w1, &s.w2).overlap.files.is_empty());

        let queued = pairs.apply(
            s.plan_with(
                s.input(&s.w1, &[], false),
                s.input(&s.w2, &["x.txt"], false),
            ),
            2_000,
        );
        assert!(queued.is_empty());
        assert_eq!(
            between(&pairs, &s.w1, &s.w2).overlap.files,
            vec!["x.txt".to_owned()]
        );
    }
}

mod queue {
    use super::*;

    fn key(n: &str) -> PairKey {
        PairKey {
            repo_id: REPO_ID.to_owned(),
            a: PathBuf::from(format!("/wt/{n}")),
            b: PairSide::Base,
        }
    }

    fn triple(n: &str) -> Triple {
        Triple {
            tip_a: n.to_owned(),
            tip_b: "base".to_owned(),
            merge_base: None,
        }
    }

    #[test]
    fn ckp001_queue_keeps_one_job_per_pair_and_the_new_one_replaces_the_old() {
        let mut q = PairQueue::default();
        q.push(key("a"), triple("old"), Priority::Rest);
        q.push(key("a"), triple("new"), Priority::Rest);
        assert_eq!(q.len(), 1);
        let (k, t) = q.pop().unwrap();
        assert_eq!(k, key("a"));
        assert_eq!(t, triple("new"));
        assert!(q.pop().is_none());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn ckp001_queue_a_replacing_push_carries_its_own_priority() {
        let mut q = PairQueue::default();
        q.push(key("a"), triple("1"), Priority::Rest);
        q.push(key("b"), triple("1"), Priority::PresentSession);
        q.push(key("a"), triple("2"), Priority::ChangedVsBase);
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop().unwrap(), (key("a"), triple("2")));
        assert_eq!(q.pop().unwrap(), (key("b"), triple("1")));
    }

    #[test]
    fn ckp001_queue_pops_by_priority_then_arrival() {
        let mut q = PairQueue::default();
        q.push(key("r1"), triple("t"), Priority::Rest);
        q.push(key("s1"), triple("t"), Priority::PresentSession);
        q.push(key("c1"), triple("t"), Priority::ChangedVsBase);
        q.push(key("r2"), triple("t"), Priority::Rest);
        q.push(key("c2"), triple("t"), Priority::ChangedVsBase);
        q.push(key("s2"), triple("t"), Priority::PresentSession);
        let order: Vec<PairKey> = std::iter::from_fn(|| q.pop().map(|(k, _)| k)).collect();
        let want: Vec<PairKey> = ["c1", "c2", "s1", "s2", "r1", "r2"].map(key).into();
        assert_eq!(order, want);
    }
}

mod op_in_progress {
    use super::*;

    #[test]
    fn ckp001_opinprog_a_worktree_with_an_operation_leaves_its_pairs_pending_without_queueing() {
        let s = Scene::new();
        let mut pairs = RepoPairs::new(REPO_ID);
        let queued = pairs.apply(
            s.plan_with(s.input(&s.w1, &[], true), s.input(&s.w2, &[], false)),
            1_000,
        );
        assert!(queued.iter().all(|(k, _)| !involves(k, &s.w1)));
        assert_eq!(queued.len(), 1, "only (w2,base) is still calculable");
        assert!(matches!(
            vs_base(&pairs, &s.w1).state,
            PairState::Pending(PendingReason::OperationInProgress)
        ));
        assert!(matches!(
            between(&pairs, &s.w1, &s.w2).state,
            PairState::Pending(PendingReason::OperationInProgress)
        ));
    }
}

mod dormant {
    use super::*;

    #[test]
    fn ckp001_dormant_suspend_keeps_the_last_result_and_time_and_a_new_plan_recalculates_all() {
        let s = Scene::new();
        let mut pairs = RepoPairs::new(REPO_ID);
        let first = pairs.apply(s.plan(), 1_000);
        for (k, t) in &first {
            assert!(pairs.complete(k, t, Ok(outcome()), 1_234));
        }

        pairs.suspend(PendingReason::Dormant);
        assert_eq!(pairs.pairs().count(), 3);
        for p in pairs.pairs() {
            assert!(matches!(
                p.state,
                PairState::Pending(PendingReason::Dormant)
            ));
            assert_eq!(p.computed_at_utc_ms, Some(1_234));
            assert_eq!(p.conflict, Some(outcome()));
        }

        // Back from dormancy: the same plan queues every pair, as the initial calculation does.
        let queued = pairs.apply(s.plan(), 9_000);
        let mut want: Vec<PairKey> = first.iter().map(|(k, _)| k.clone()).collect();
        let mut got: Vec<PairKey> = queued.iter().map(|(k, _)| k.clone()).collect();
        want.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        got.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        assert_eq!(got, want);
        assert!(
            pairs
                .pairs()
                .all(|p| matches!(p.state, PairState::Calculating | PairState::Recalculating))
        );
    }
}
