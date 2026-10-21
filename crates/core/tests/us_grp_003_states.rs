// US-GRP-003, scenario 1: the special states of a worktree, on temporary repos
// of the "intact repo" harness (NFR-01), without the daemon. Every observation
// is wrapped in `check(..).assert_intact()` and the marker of the operation is
// still there afterwards (BR-CONS-001).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{
    ChangeKindView, HeadView, OperationKindView, WorktreeStatus, WorktreeView,
};
use gitraptor_core::observe::{self, base_branch, locate, reconcile};
use gitraptor_core::watch::{ObservedBatch, Observer, WatchConfig};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

const DEADLINE: Duration = Duration::from_secs(10);

/// `main` and `topic` both rewrite the same line of `a.txt` and `b.txt`.
fn conflicting() -> Fixture {
    let f = Fixture::new(&git_from_path());
    f.write("a.txt", "base\n");
    f.write("b.txt", "base\n");
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "base"]);
    f.git(&["checkout", "-q", "-b", "topic"]);
    f.write("a.txt", "topic\n");
    f.write("b.txt", "topic\n");
    f.git(&["commit", "-q", "-am", "topic"]);
    f.git(&["checkout", "-q", "main"]);
    f.write("a.txt", "main\n");
    f.write("b.txt", "main\n");
    f.git(&["commit", "-q", "-am", "main"]);
    f
}

/// Runs a Git command that is expected to stop on a conflict.
fn stops(f: &Fixture, args: &[&str]) {
    let out = f.git_command(&f.repo, args).output().unwrap();
    assert!(!out.status.success(), "git {args:?} should have stopped");
}

fn root_view(f: &Fixture) -> WorktreeView {
    let common = locate(&f.repo).unwrap();
    let read = reconcile(&common, &base_branch(None)).unwrap();
    read.views().into_iter().next().unwrap()
}

/// Reads the main worktree inside the intact check.
fn observed(name: &str, f: &Fixture) -> WorktreeView {
    let mut view = None;
    check(name, f, &Exceptions::none(), || {
        view = Some(root_view(f));
    })
    .assert_intact();
    view.unwrap()
}

fn git_dir(f: &Fixture, rela: &str) -> PathBuf {
    f.repo.join(".git").join(rela)
}

fn conflicted_paths(w: &WorktreeView) -> Vec<String> {
    match &w.status {
        WorktreeStatus::Ready { changes, .. } => changes
            .iter()
            .filter(|c| c.kind == ChangeKindView::Conflicted)
            .map(|c| c.path.raw().to_owned())
            .collect(),
        other => panic!("not ready: {other:?}"),
    }
}

#[test]
fn a_stopped_rebase_is_reported_and_left_as_is() {
    let f = conflicting();
    f.git(&["checkout", "-q", "topic"]);
    stops(&f, &["rebase", "main"]);
    assert!(git_dir(&f, "rebase-merge").is_dir());

    let w = observed("STATES-REBASE", &f);

    assert_eq!(w.operation, Some(OperationKindView::Rebase));
    assert_eq!(w.unmerged_paths, 2);
    assert!(git_dir(&f, "rebase-merge").is_dir(), "the rebase was touched");
    assert!(f.git(&["status"]).contains("rebase in progress"));
}

#[test]
fn a_conflicted_merge_reports_its_unmerged_paths() {
    let f = conflicting();
    stops(&f, &["merge", "topic"]);
    assert!(git_dir(&f, "MERGE_HEAD").is_file());

    let w = observed("STATES-MERGE", &f);

    assert_eq!(w.operation, Some(OperationKindView::Merge));
    assert_eq!(w.unmerged_paths, 2, "once per path, not once per index stage");
    let mut paths = conflicted_paths(&w);
    paths.sort();
    assert_eq!(paths, ["a.txt", "b.txt"]);
    assert!(git_dir(&f, "MERGE_HEAD").is_file(), "the merge was touched");
    assert!(f.git(&["status"]).contains("You have unmerged paths"));
}

#[test]
fn a_detached_head_is_reported_without_an_operation() {
    let f = conflicting();
    let commit = f.git(&["rev-parse", "HEAD"]);
    f.git(&["checkout", "-q", "--detach", commit.trim()]);

    let w = observed("STATES-DETACHED", &f);

    assert!(matches!(
        &w.status,
        WorktreeStatus::Ready {
            head: HeadView::Detached,
            ..
        }
    ));
    assert_eq!(w.operation, None);
    assert_eq!(w.unmerged_paths, 0);
    assert!(f.git(&["status"]).contains("HEAD detached"));
}

#[test]
fn unmerged_paths_without_an_operation() {
    let f = conflicting();
    f.write("a.txt", "wip\n");
    f.git(&["stash", "-q"]);
    f.write("a.txt", "newer\n");
    f.git(&["commit", "-q", "-am", "newer"]);
    stops(&f, &["stash", "pop"]);
    assert!(!git_dir(&f, "MERGE_HEAD").exists());

    let w = observed("STATES-UNMERGED", &f);

    assert_eq!(w.operation, None);
    assert_eq!(w.unmerged_paths, 1);
    assert_eq!(conflicted_paths(&w), ["a.txt"]);
    assert!(f.git(&["status"]).contains("Unmerged paths"));
}

#[test]
fn cherry_pick_and_bisect_have_their_kind() {
    let f = conflicting();
    stops(&f, &["cherry-pick", "topic"]);
    assert!(git_dir(&f, "CHERRY_PICK_HEAD").is_file());

    let w = observed("STATES-KINDS cherry-pick", &f);

    assert_eq!(w.operation, Some(OperationKindView::CherryPick));
    assert_eq!(w.unmerged_paths, 2);
    assert!(git_dir(&f, "CHERRY_PICK_HEAD").is_file());
    assert!(f.git(&["status"]).contains("You are currently cherry-picking"));

    let g = conflicting();
    g.git(&["bisect", "start"]);
    assert!(git_dir(&g, "BISECT_LOG").is_file());

    let w = observed("STATES-KINDS bisect", &g);

    assert_eq!(w.operation, Some(OperationKindView::Bisect));
    assert_eq!(w.unmerged_paths, 0);
    assert!(git_dir(&g, "BISECT_LOG").is_file());
    assert!(g.git(&["status"]).contains("You are currently bisecting"));
}

struct Watched {
    _observer: Observer,
    rx: Receiver<ObservedBatch>,
}

fn watch(f: &Fixture) -> Watched {
    let (tx, rx) = channel();
    let tx = Arc::new(Mutex::new(tx));
    let observer = Observer::start(
        WatchConfig::default(),
        Arc::new(move |b| {
            let _ = tx.lock().unwrap().send(b);
        }),
    );
    let common = locate(&f.repo).unwrap();
    let read = reconcile(&common, &base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);
    Watched {
        _observer: observer,
        rx,
    }
}

impl Watched {
    /// Waits for a batch whose view of `root` satisfies `want`.
    fn until(&self, root: &Path, want: impl Fn(&WorktreeView) -> bool) {
        let root = observe::canonical(root);
        let start = Instant::now();
        let mut seen = Vec::new();
        while start.elapsed() < DEADLINE {
            if let Ok(b) = self.rx.recv_timeout(Duration::from_millis(100)) {
                for r in &b.worktrees {
                    if Path::new(r.view.path.raw()) == root && want(&r.view) {
                        return;
                    }
                }
                seen.push(b);
            }
        }
        panic!("timed out; batches seen: {seen:#?}");
    }
}

#[test]
fn the_observer_publishes_entering_and_leaving_a_merge() {
    let f = conflicting();
    let w = watch(&f);

    stops(&f, &["merge", "topic"]);
    check("STATES-LIVE enter", &f, &Exceptions::none(), || {
        w.until(&f.repo, |v| {
            v.operation == Some(OperationKindView::Merge) && v.unmerged_paths == 2
        });
    })
    .assert_intact();
    assert!(git_dir(&f, "MERGE_HEAD").is_file(), "the merge was touched");
    assert!(f.git(&["status"]).contains("You have unmerged paths"));

    f.git(&["merge", "--abort"]);
    w.until(&f.repo, |v| v.operation.is_none() && v.unmerged_paths == 0);
    assert!(!git_dir(&f, "MERGE_HEAD").exists());
}
