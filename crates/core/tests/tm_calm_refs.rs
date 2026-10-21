//! The engine's calm covers the refs already moved on disk (fix/tm-restore-calm-mark): a worktree
//! is calm only if the repo's local branches on disk are those of the last persisted batch.
//!
//! Every test runs on a testkit fixture (temporary repo, home and profile; Git with the fixture's
//! home and no system config), never this repo or the real profile (NFR-01). The order of the
//! engine's batches is injected by calling `set_persisted` / `set_head_logs` at fixed points, and
//! the calm is read with `settle(ZERO)` at each point, so nothing waits on time.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gitraptor_core::timemachine::engine::{RepoMarks, git_dir_of};
use gitraptor_core::watch::{RefsView, read_local_branches};
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;

const REPO: &str = "r";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn common(f: &Fixture) -> PathBuf {
    gitraptor_core::observe::locate(&f.repo).unwrap()
}

/// The repo's local branches as Git lists them, `name → commit`.
fn branches(f: &Fixture) -> BTreeMap<String, String> {
    f.git(&[
        "for-each-ref",
        "--format=%(refname:short) %(objectname)",
        "refs/heads",
    ])
    .lines()
    .filter_map(|l| l.split_once(' '))
    .map(|(n, c)| (n.to_owned(), c.to_owned()))
    .collect()
}

/// What a worktree task read for a batch: the size of its `HEAD` reflog and its `HEAD`.
fn publish_head(marks: &RepoMarks, wt: &Path) {
    let git_dir = git_dir_of(wt).unwrap();
    let len = std::fs::metadata(git_dir.join("logs/HEAD")).map_or(0, |m| m.len());
    marks.set_head_logs(REPO, &[(wt.to_path_buf(), len)]);
    marks.set_heads(
        REPO,
        &[(
            wt.to_path_buf(),
            std::fs::read(git_dir.join("HEAD")).unwrap(),
        )],
    );
}

fn settle_now(marks: &RepoMarks, wt: &Path) -> Option<i64> {
    marks.settle(REPO, &[wt.to_path_buf()], Duration::ZERO)
}

/// A `git update-ref` moves a branch without writing the `HEAD` reflog: until the engine persists
/// the batch that read it, the repo is not calm.
#[test]
fn settle_waits_for_a_ref_move_the_engine_has_not_persisted() {
    let f = Fixture::with_commit(&git());
    let (marks, common) = (RepoMarks::default(), common(&f));
    let wt = f.repo.clone();
    marks.set_persisted(REPO, 1, &common, branches(&f));
    publish_head(&marks, &wt);
    assert_eq!(settle_now(&marks, &wt), Some(1), "calm before the move");
    let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.git(&["update-ref", "refs/heads/x", &head]);
    assert_eq!(
        settle_now(&marks, &wt),
        None,
        "a branch moved on disk that no persisted batch read"
    );
    // The late batch that read it.
    marks.set_persisted(REPO, 3, &common, branches(&f));
    assert_eq!(settle_now(&marks, &wt), Some(3));
}

/// Packing the refs changes the files, not the branches: the calm holds.
#[test]
fn packing_refs_between_the_batch_and_settle_keeps_the_calm() {
    let f = Fixture::with_commit(&git());
    let (marks, common) = (RepoMarks::default(), common(&f));
    let wt = f.repo.clone();
    f.git(&["branch", "side"]);
    marks.set_persisted(REPO, 1, &common, branches(&f));
    publish_head(&marks, &wt);
    f.git(&["pack-refs", "--all"]);
    assert_eq!(settle_now(&marks, &wt), Some(1));
}

/// A commit in a linked worktree: `git` writes the `HEAD` reflog before it moves the branch, so
/// the engine can persist a batch with the new reflog size and the old branches (H1). That batch
/// does not calm the worktree; the one with the new branches does.
#[test]
fn a_heads_only_batch_does_not_calm_a_commit_whose_ref_is_unpersisted() {
    let f = Fixture::with_commit(&git());
    f.git(&["branch", "feat"]);
    let wt = gitraptor_testkit::fixture::canonical_dir(&f.add_worktree("feat", "feat"));
    let (marks, common) = (RepoMarks::default(), common(&f));
    let before = branches(&f);
    marks.set_persisted(REPO, 1, &common, before.clone());
    publish_head(&marks, &wt);
    assert_eq!(settle_now(&marks, &wt), Some(1), "calm before the commit");
    std::fs::write(wt.join("feat.txt"), "feat\n").unwrap();
    f.git_in(&wt, &["add", "feat.txt"]);
    f.git_in(&wt, &["commit", "-q", "-m", "feat"]);
    // The heads-only batch: the new reflog size, no events, the branches as before.
    publish_head(&marks, &wt);
    marks.set_seq(REPO, 1);
    assert_ne!(branches(&f), before, "the commit moved `feat`");
    assert_eq!(
        settle_now(&marks, &wt),
        None,
        "the reflog is persisted but the branch it moved is not"
    );
    marks.set_persisted(REPO, 3, &common, branches(&f));
    assert_eq!(settle_now(&marks, &wt), Some(3));
}

/// A repo whose engine never published branches keeps the time rule.
#[test]
fn settle_without_published_refs_keeps_the_quiet_rule() {
    let f = Fixture::with_commit(&git());
    let wt = f.repo.clone();
    let marks = RepoMarks::default();
    // Touched "now" for the whole test: a touch ahead of the clock never reads as quiet, so the
    // result does not depend on how long the machine takes to reach `settle`.
    let ahead = u64::try_from(Duration::from_secs(3600).as_nanos()).unwrap();
    marks.touch(REPO, gitraptor_api::clock::monotonic_ns() + ahead);
    assert_eq!(
        marks.settle(REPO, std::slice::from_ref(&wt), Duration::from_millis(30)),
        None
    );
    let marks = RepoMarks::default();
    marks.touch(REPO, 0);
    assert_eq!(
        marks.settle(REPO, &[wt], Duration::from_millis(30)),
        Some(0)
    );
}

/// A repo whose common directory cannot be read is never calm (fail closed).
#[test]
fn an_unreadable_common_dir_is_never_calm() {
    let f = Fixture::with_commit(&git());
    let (marks, common) = (RepoMarks::default(), common(&f));
    let wt = f.repo.clone();
    marks.set_persisted(REPO, 1, &common, BTreeMap::new());
    std::fs::remove_dir_all(&common).unwrap();
    assert_eq!(
        marks.settle(REPO, &[wt], Duration::from_millis(30)),
        None,
        "the refs could not be read"
    );
}

/// `remove` forgets the persisted refs with the rest of the repo: back to the time rule.
#[test]
fn remove_forgets_the_persisted_refs() {
    let f = Fixture::with_commit(&git());
    let (marks, common) = (RepoMarks::default(), common(&f));
    let wt = f.repo.clone();
    marks.set_persisted(REPO, 1, &common, branches(&f));
    let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.git(&["update-ref", "refs/heads/x", &head]);
    assert_eq!(
        settle_now(&marks, &wt),
        None,
        "the persisted refs hold the calm before `remove`"
    );
    marks.remove(REPO);
    assert_eq!(settle_now(&marks, &wt), Some(0));
}

/// A loose ref with garbage reads the same in the refs view and in the calm: both fail, so the
/// repo is not calm.
#[test]
fn a_corrupt_loose_ref_reads_the_same_in_the_view_and_the_calm() {
    let f = Fixture::with_commit(&git());
    let (marks, common) = (RepoMarks::default(), common(&f));
    let wt = f.repo.clone();
    marks.set_persisted(REPO, 1, &common, branches(&f));
    publish_head(&marks, &wt);
    std::fs::write(common.join("refs/heads/roto"), "not a ref\n").unwrap();
    let view = RefsView::read(&common).map(|v| v.branches);
    let reader = RepoReader::open(&common, &ReaderOptions::default()).unwrap();
    let helper = read_local_branches(&reader);
    assert!(view.is_err(), "the refs view reads a corrupt ref: {view:?}");
    assert_eq!(helper, view, "the calm and the view read the refs alike");
    assert_eq!(
        settle_now(&marks, &wt),
        None,
        "an unreadable ref is not calm"
    );
}
