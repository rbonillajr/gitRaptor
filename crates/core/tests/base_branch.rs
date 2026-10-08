//! US-GRP-012: the ahead/behind of each worktree against the base branch of
//! its repo, on temporary repos of the "intact repo" harness (NFR-01). Runs
//! on every OS of the CI; the end-to-end scenarios are in
//! `apps/cli/tests/base_branch.rs`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Stdio;

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{
    BaseStatusView, CommitCountView, DivergenceView, MAX_DIVERGENCE_WALK, RepoStateView, RepoView,
    WorktreeStatus, WorktreeView,
};
use gitraptor_core::observe::{
    DivergenceCache, RepoRead, base_branch, base_view, locate, reconcile, refresh_divergence,
};
use gitraptor_git::RefName;
use gitraptor_policy::team::{BaseStatus, Confirmed, ConfirmedFloor};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

fn divergence(w: &WorktreeView) -> &DivergenceView {
    match &w.status {
        WorktreeStatus::Ready { divergence, .. } => divergence,
        other => panic!("not ready: {other:?}"),
    }
}

fn counted(ahead: u64, behind: u64) -> DivergenceView {
    let exact = |count| CommitCountView { count, exact: true };
    DivergenceView::Counted {
        ahead: exact(ahead),
        behind: exact(behind),
    }
}

/// `n` empty commits in `dir`, each with a message no other commit has, so
/// two branches never share one by accident.
fn commit(f: &Fixture, dir: &std::path::Path, n: usize) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    for _ in 0..n {
        let i = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        f.git_in(
            dir,
            &["commit", "-q", "--allow-empty", "-m", &format!("c{i}")],
        );
    }
}

/// "demo": `main` with one commit and "feat-login" with 3 commits that are
/// not in `main`, which has 1 that "feat-login" lacks.
fn demo() -> (Fixture, PathBuf) {
    let f = Fixture::with_commit(&git_from_path());
    f.git(&["branch", "feat-login"]);
    let wt = f.add_worktree("feat-login", "feat-login");
    commit(&f, &wt, 3);
    commit(&f, &f.repo, 1);
    (f, wt)
}

fn read(f: &Fixture) -> RepoRead {
    reconcile(&locate(&f.repo).unwrap(), &base_branch(None)).unwrap()
}

#[test]
fn repo_intact_without_a_confirmation_the_base_branch_is_main_unconfirmed() {
    let (f, _wt) = demo();
    let report = check("divergence", &f, &Exceptions::none(), || {
        let read = read(&f);
        assert_eq!(read.base.status, BaseStatus::Unconfirmed);
        let view = base_view(&read.base);
        assert_eq!(view.name.unwrap().raw(), "main");
        assert_eq!(view.status, BaseStatusView::Unconfirmed);
        let wts = read.views();
        assert_eq!(divergence(&wts[0]), &counted(0, 0));
        assert_eq!(divergence(&wts[1]), &counted(3, 1));
    });
    report.assert_intact();
}

#[test]
fn a_confirmed_base_branch_is_the_one_counted_against() {
    let (f, _wt) = demo();
    f.git(&["branch", "develop", "feat-login"]);
    let confirmed = Confirmed {
        base_branch: RefName::new("develop").unwrap(),
        floor: ConfirmedFloor::Absent,
    };
    let base = base_branch(Some(&confirmed));
    assert_eq!(base.status, BaseStatus::Confirmed);
    let read = reconcile(&locate(&f.repo).unwrap(), &base).unwrap();
    let wts = read.views();
    assert_eq!(divergence(&wts[0]), &counted(1, 3));
    assert_eq!(divergence(&wts[1]), &counted(0, 0));

    // Counting again against another base changes every worktree.
    let mut read = read;
    read.set_base(base_branch(None));
    assert_eq!(divergence(&read.views()[1]), &counted(3, 1));
}

#[test]
fn repo_intact_a_missing_base_branch_is_not_replaced_by_another() {
    let f = Fixture::with_commit(&git_from_path());
    f.git(&["branch", "-m", "trunk"]);
    // A known remote copy of `main` is another branch, not the base (Q42).
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["update-ref", "refs/remotes/origin/main", head.trim()]);
    f.git(&["tag", "main"]);
    let report = check("divergence", &f, &Exceptions::none(), || {
        let read = read(&f);
        assert_eq!(divergence(&read.views()[0]), &DivergenceView::BaseMissing);
    });
    report.assert_intact();
}

#[test]
fn detached_and_unborn_heads() {
    let (f, wt) = demo();
    let tip = f.git_in(&wt, &["rev-parse", "HEAD~1"]);
    f.git_in(&wt, &["checkout", "-q", "--detach", tip.trim()]);
    f.git(&["checkout", "-q", "--orphan", "fresh"]);
    let wts = read(&f).views();
    assert_eq!(divergence(&wts[0]), &DivergenceView::NoCommits);
    assert_eq!(divergence(&wts[1]), &counted(2, 1));
}

#[test]
fn the_walk_is_bounded() {
    let f = Fixture::with_commit(&git_from_path());
    let start = f.git(&["rev-parse", "HEAD"]);
    let total = MAX_DIVERGENCE_WALK + 1;
    let mut stream = String::new();
    for i in 1..=total {
        stream.push_str(&format!(
            "commit refs/heads/long\nmark :{i}\ncommitter T <t@e> 0 +0000\ndata 0\n"
        ));
        match i {
            1 => stream.push_str(&format!("from {}\n", start.trim())),
            _ => stream.push_str(&format!("from :{}\n", i - 1)),
        }
    }
    let mut import = f
        .git_command(&f.repo, &["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    import
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(import.wait().unwrap().success());
    f.git(&["checkout", "-q", "long"]);
    match divergence(&read(&f).views()[0]) {
        DivergenceView::Counted { ahead, behind } => {
            assert_eq!(
                *ahead,
                CommitCountView {
                    count: MAX_DIVERGENCE_WALK,
                    exact: false
                }
            );
            assert_eq!(behind.count, 0);
        }
        other => panic!("{other:?}"),
    }
}

/// D5: the snapshot counts against the base branch as it is now, and each
/// row from the branch it names, never from a `HEAD` it does not show.
#[test]
fn repo_intact_the_snapshot_follows_the_base_branch_but_keeps_the_heads() {
    let (f, wt) = demo();
    let read = read(&f);
    let mut repos = vec![RepoView {
        fetched_utc_ms: None,
        repo_id: "r1".into(),
        state: RepoStateView::Observed,
        path: Untrusted::from_os(read.common_dir.as_os_str()),
        base: base_view(&read.base),
        worktrees: read.views(),
        tier: None,
        checked_utc_ms: None,
        kept_temps: None,
    }];
    let inputs = BTreeMap::from([("r1".to_owned(), read.divergence_inputs())]);
    let cache = DivergenceCache::default();

    // `main` advances and the "feat-login" worktree detaches without a
    // reconciliation: its row still names the branch "feat-login", so it
    // counts from that branch; the main worktree follows `main`.
    commit(&f, &f.repo, 1);
    f.git_in(&wt, &["checkout", "-q", "--detach", "HEAD~2"]);
    let report = check("refresh", &f, &Exceptions::none(), || {
        refresh_divergence(&mut repos, &inputs, &cache);
    });
    report.assert_intact();
    assert_eq!(divergence(&repos[0].worktrees[1]), &counted(3, 2));
    assert_eq!(divergence(&repos[0].worktrees[0]), &counted(0, 0));
    assert_eq!(cache.len(), 2);

    // The same pairs again come from the cache.
    refresh_divergence(&mut repos, &inputs, &cache);
    assert_eq!(cache.len(), 2);
    assert_eq!(divergence(&repos[0].worktrees[1]), &counted(3, 2));

    // The branch the row names moves back one commit.
    f.git(&["branch", "-f", "feat-login", "feat-login~1"]);
    refresh_divergence(&mut repos, &inputs, &cache);
    assert_eq!(divergence(&repos[0].worktrees[1]), &counted(2, 2));
}
