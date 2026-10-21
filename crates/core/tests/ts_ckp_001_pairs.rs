// TS-CKP-001 T002: the pairs of a repo, each worktree's own work and the overlap of every pair.
// Temporary repos only (NFR-01).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gitraptor_api::messages::BaseStatusView;
use gitraptor_core::observe::UncommittedPaths;
use gitraptor_core::predict::overlap::OwnWork;
use gitraptor_core::predict::{
    BaseInput, NotComputableReason, PairPrediction, PairSide, PairState, PendingReason,
    WorktreeInput,
};
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

const REPO_ID: &str = "r";

struct Scene {
    f: Fixture,
}

impl Scene {
    fn new() -> Self {
        let f = Fixture::with_commit(&git_from_path());
        Self { f }
    }

    fn reader(&self) -> RepoReader {
        RepoReader::open(&self.f.repo, &ReaderOptions::default()).unwrap()
    }

    fn tip(&self, dir: &Path) -> String {
        self.f.git_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// A linked worktree on a new branch cut from the current `main`.
    fn worktree(&self, name: &str) -> PathBuf {
        self.f.git(&["branch", name, "main"]);
        self.f.add_worktree(name, name)
    }

    fn commit(&self, dir: &Path, file: &str, content: &str) {
        std::fs::write(dir.join(file), content).unwrap();
        self.f.git_in(dir, &["add", file]);
        self.f
            .git_in(dir, &["commit", "-q", "-m", &format!("{file} {content}")]);
    }

    fn base(&self) -> BaseInput {
        BaseInput {
            status: BaseStatusView::Confirmed,
            tip: Some(self.tip(&self.f.repo)),
            checked_out_at: Some(self.f.repo.clone()),
        }
    }

    fn input(&self, root: &Path, uncommitted: &[&str]) -> WorktreeInput {
        WorktreeInput {
            root: root.to_owned(),
            tip: Some(self.tip(root)),
            uncommitted: UncommittedPaths::from_paths(
                uncommitted.iter().map(|s| (*s).to_owned()),
                10_000,
            ),
            in_progress: false,
            session_present: false,
        }
    }

    fn plan(&self, base: &BaseInput, wts: &[WorktreeInput]) -> Vec<PairPrediction> {
        gitraptor_core::predict::pairs::plan(&self.reader(), REPO_ID, base, wts).unwrap()
    }
}

/// Pair identity independent of which side the plan calls `a`.
fn ident(p: &PairPrediction) -> (BTreeSet<PathBuf>, bool) {
    match &p.key.b {
        PairSide::Base => ([p.key.a.clone()].into(), true),
        PairSide::Worktree(b) => ([p.key.a.clone(), b.clone()].into(), false),
    }
}

fn by_ident(plan: &[PairPrediction]) -> BTreeMap<(BTreeSet<PathBuf>, bool), &PairPrediction> {
    plan.iter().map(|p| (ident(p), p)).collect()
}

fn vs_base(w: &Path) -> (BTreeSet<PathBuf>, bool) {
    ([w.to_owned()].into(), true)
}

fn between(a: &Path, b: &Path) -> (BTreeSet<PathBuf>, bool) {
    ([a.to_owned(), b.to_owned()].into(), false)
}

mod pairs {
    use super::*;

    #[test]
    fn ckp001_pairs_forms_each_worktree_with_own_work_and_skips_the_one_with_the_base() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        let w2 = s.worktree("w2");
        let idle = s.worktree("idle");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&w2, "y.txt", "2\n");
        // The main worktree has the base checked out and an uncommitted file of its own.
        let main_wt = s.f.repo.clone();
        let wts = [
            s.input(&main_wt, &["m.txt"]),
            s.input(&w1, &[]),
            s.input(&w2, &[]),
            s.input(&idle, &[]),
        ];
        let plan = s.plan(&s.base(), &wts);
        let got: BTreeSet<_> = plan.iter().map(ident).collect();
        let want: BTreeSet<_> = [
            vs_base(&w1),
            vs_base(&w2),
            between(&main_wt, &w1),
            between(&main_wt, &w2),
            between(&w1, &w2),
        ]
        .into();
        assert_eq!(got, want);
        assert_eq!(plan.len(), want.len(), "no pair twice");
        assert!(plan.iter().all(|p| p.key.repo_id == REPO_ID));
        for p in &plan {
            assert!(matches!(p.state, PairState::Calculating));
            assert!(p.triple.is_some());
        }
    }

    #[test]
    fn ckp001_pairs_between_worktrees_name_the_lowest_path_a() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        let w2 = s.worktree("w2");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&w2, "y.txt", "2\n");
        let wts = [s.input(&w2, &[]), s.input(&w1, &[])];
        let plan = s.plan(&s.base(), &wts);
        let pair = plan
            .iter()
            .find(|p| matches!(p.key.b, PairSide::Worktree(_)))
            .unwrap();
        let PairSide::Worktree(b) = &pair.key.b else {
            unreachable!()
        };
        assert!(pair.key.a.as_os_str().as_encoded_bytes() < b.as_os_str().as_encoded_bytes());
    }

    #[test]
    fn ckp001_pairs_a_worktree_without_own_work_forms_none() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        let idle = s.worktree("idle");
        s.commit(&w1, "x.txt", "1\n");
        let wts = [s.input(&w1, &[]), s.input(&idle, &[])];
        let plan = s.plan(&s.base(), &wts);
        assert!(
            plan.iter()
                .all(|p| ident(p).0.iter().all(|r| r.as_path() != idle.as_path())),
            "an idle worktree formed a pair"
        );
        assert_eq!(plan.len(), 1);
    }
}

mod old_base {
    use super::*;

    #[test]
    fn a_worktree_on_an_older_base_does_not_overlap_by_base_commits() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&w1, "y.txt", "1\n");
        // The base moves on after the fork: a.txt is only the base's, y.txt is shared.
        s.commit(&s.f.repo, "a.txt", "base changed\n");
        s.commit(&s.f.repo, "y.txt", "base y\n");
        let plan = s.plan(&s.base(), &[s.input(&w1, &[])]);
        let map = by_ident(&plan);
        let pair = map[&vs_base(&w1)];
        assert_eq!(pair.overlap.files, vec!["y.txt".to_owned()]);
        assert!(!pair.overlap.partial);
    }
}

mod overlap {
    use super::*;

    fn own(paths: &[&str], partial: bool) -> OwnWork {
        OwnWork {
            paths: paths.iter().map(|s| (*s).to_owned()).collect(),
            partial,
        }
    }

    #[test]
    fn ckp001_overlap_is_the_intersection_sorted() {
        let o = gitraptor_core::predict::overlap::intersect(
            &own(&["c", "a", "b"], false),
            &own(&["b", "c", "d"], false),
        );
        assert_eq!(o.files, vec!["b".to_owned(), "c".to_owned()]);
        assert!(!o.partial);
    }

    #[test]
    fn ckp001_overlap_disjoint_is_empty_and_complete() {
        let o =
            gitraptor_core::predict::overlap::intersect(&own(&["a"], false), &own(&["b"], false));
        assert!(o.files.is_empty());
        assert!(!o.partial);
    }

    #[test]
    fn ckp001_overlap_a_partial_input_gives_a_partial_result() {
        let o =
            gitraptor_core::predict::overlap::intersect(&own(&["a"], true), &own(&["b"], false));
        assert!(o.partial);
        let o =
            gitraptor_core::predict::overlap::intersect(&own(&["a"], false), &own(&["a"], true));
        assert!(o.partial);
        assert_eq!(o.files, vec!["a".to_owned()]);
    }

    #[test]
    fn ckp001_overlap_counts_committed_and_uncommitted_work() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        let w2 = s.worktree("w2");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&w2, "y.txt", "2\n");
        let wts = [s.input(&w1, &[]), s.input(&w2, &["x.txt"])];
        let plan = s.plan(&s.base(), &wts);
        let map = by_ident(&plan);
        let pair = map[&between(&w1, &w2)];
        assert_eq!(pair.overlap.files, vec!["x.txt".to_owned()]);
        assert!(!pair.overlap.partial);
    }

    #[test]
    fn ckp001_overlap_a_partial_uncommitted_set_gives_a_partial_overlap() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        let w2 = s.worktree("w2");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&w2, "y.txt", "2\n");
        let mut partial = s.input(&w2, &[]);
        partial.uncommitted = UncommittedPaths::from_paths((0..5).map(|i| format!("u{i}.txt")), 2);
        assert!(partial.uncommitted.partial);
        let plan = s.plan(&s.base(), &[s.input(&w1, &[]), partial]);
        let map = by_ident(&plan);
        assert!(map[&between(&w1, &w2)].overlap.partial);
        assert!(!map[&vs_base(&w1)].overlap.partial);
    }

    #[test]
    fn ckp001_overlap_worktree_against_base_uses_committed_and_uncommitted_work() {
        let s = Scene::new();
        let w1 = s.worktree("w1");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&s.f.repo, "x.txt", "base x\n");
        s.commit(&s.f.repo, "z.txt", "base z\n");
        let plan = s.plan(&s.base(), &[s.input(&w1, &["z.txt"])]);
        let map = by_ident(&plan);
        let pair = map[&vs_base(&w1)];
        assert_eq!(
            pair.overlap.files,
            vec!["x.txt".to_owned(), "z.txt".to_owned()]
        );
    }
}

mod base_states {
    use super::*;

    fn two(s: &Scene) -> (PathBuf, PathBuf, [WorktreeInput; 2]) {
        let w1 = s.worktree("w1");
        let w2 = s.worktree("w2");
        s.commit(&w1, "x.txt", "1\n");
        s.commit(&w2, "y.txt", "2\n");
        let wts = [s.input(&w1, &[]), s.input(&w2, &[])];
        (w1, w2, wts)
    }

    #[test]
    fn ckp001_base_states_unconfirmed_leaves_base_pairs_pending_and_worktree_pairs_going() {
        let s = Scene::new();
        let (w1, w2, wts) = two(&s);
        let mut base = s.base();
        base.status = BaseStatusView::Unconfirmed;
        let plan = s.plan(&base, &wts);
        let map = by_ident(&plan);
        for w in [&w1, &w2] {
            assert!(matches!(
                map[&vs_base(w)].state,
                PairState::Pending(PendingReason::BaseUnconfirmed)
            ));
        }
        assert!(matches!(
            map[&between(&w1, &w2)].state,
            PairState::Calculating
        ));
    }

    #[test]
    fn ckp001_base_states_invalid_is_not_computable() {
        let s = Scene::new();
        let (w1, w2, wts) = two(&s);
        let mut base = s.base();
        base.status = BaseStatusView::Invalid;
        let plan = s.plan(&base, &wts);
        let map = by_ident(&plan);
        for w in [&w1, &w2] {
            assert!(matches!(
                map[&vs_base(w)].state,
                PairState::NotComputable(NotComputableReason::BaseMissing)
            ));
        }
    }

    #[test]
    fn ckp001_base_states_a_base_branch_without_ref_is_not_computable() {
        let s = Scene::new();
        let (w1, _w2, wts) = two(&s);
        let mut base = s.base();
        base.tip = None;
        let plan = s.plan(&base, &wts);
        let map = by_ident(&plan);
        assert!(matches!(
            map[&vs_base(&w1)].state,
            PairState::NotComputable(NotComputableReason::BaseMissing)
        ));
    }
}
