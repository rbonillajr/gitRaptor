// US-GRP-002: the change observer on temporary repos of the "intact repo"
// harness (NFR-01), without the daemon: what it sees, how it names Git
// events and how it recovers what it missed. Runs on every OS of the CI;
// the timings are checked on macOS only (Pendiente: etapa de validación
// multiplataforma).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{GitEventKind, WorktreeStatus};
use gitraptor_core::observe::{self, reconcile};
use gitraptor_core::profile::GapCause;
use gitraptor_core::watch::{ObservedBatch, Observer, RawEvent, WatchBackend, WatchConfig};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

struct Watched {
    observer: Observer,
    rx: Receiver<ObservedBatch>,
}

fn watch(f: &Fixture, config: WatchConfig) -> Watched {
    let (tx, rx) = channel();
    let tx = Arc::new(Mutex::new(tx));
    let observer = Observer::start(
        config,
        Arc::new(move |b| {
            let _ = tx.lock().unwrap().send(b);
        }),
    );
    let common = observe::locate(&f.repo).unwrap();
    let read = reconcile(&common, &observe::base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);
    // Let the streams start and the first re-read settle.
    let w = Watched { observer, rx };
    w.drain(Duration::from_millis(400));
    w
}

impl Watched {
    fn drain(&self, quiet: Duration) -> Vec<ObservedBatch> {
        let mut out = Vec::new();
        while let Ok(b) = self.rx.recv_timeout(quiet) {
            out.push(b);
        }
        out
    }

    /// Batches until `done` holds on everything seen, or 10 s.
    fn until(&self, done: impl Fn(&[ObservedBatch]) -> bool) -> Vec<ObservedBatch> {
        let start = Instant::now();
        let mut seen = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(b) = self.rx.recv_timeout(Duration::from_millis(100)) {
                seen.push(b);
                if done(&seen) {
                    // Whatever belongs to the same windows.
                    seen.extend(self.drain(Duration::from_millis(300)));
                    return seen;
                }
            }
        }
        panic!("timed out: {seen:#?}");
    }

    fn events_until(&self, kind: GitEventKind) -> Vec<RawEvent> {
        let batches = self.until(|bs| events(bs).iter().any(|e| e.kind == kind));
        events(&batches)
    }
}

fn events(batches: &[ObservedBatch]) -> Vec<RawEvent> {
    batches.iter().flat_map(|b| b.events.clone()).collect()
}

fn canonical(p: &Path) -> PathBuf {
    // The engine's own canonical form (the drive form on Windows).
    gitraptor_core::observe::canonical(p)
}

fn config() -> WatchConfig {
    WatchConfig {
        backend: BACKEND,
        ..WatchConfig::default()
    }
}

fn fast() -> WatchConfig {
    config()
}

/// Whether this run leaves ignored folders out of the OS stream: only the engine's own FSEvents
/// stream does (ADR-GRP-010, Enmienda 2026-10-08).
fn excluding() -> bool {
    cfg!(target_os = "macos") && BACKEND == WatchBackend::Fsevents
}

/// "demo": `main` with `login.txt`, and a linked worktree on `feat-login`.
fn demo() -> (Fixture, PathBuf) {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    let wt = f.add_worktree("feat-login", "feat-login");
    (f, canonical(&wt))
}

fn only(events: &[RawEvent], kind: GitEventKind) -> RawEvent {
    let found: Vec<_> = events.iter().filter(|e| e.kind == kind).collect();
    assert_eq!(found.len(), 1, "{events:#?}");
    found[0].clone()
}

#[test]
fn a_file_change_reaches_its_worktree_task() {
    let (f, wt) = demo();
    let w = watch(&f, fast());
    std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();
    let t0 = gitraptor_api::clock::monotonic_ns();
    let batches = w.until(|bs| bs.iter().any(|b| !b.worktrees.is_empty()));
    let b = batches.iter().find(|b| !b.worktrees.is_empty()).unwrap();
    assert_eq!(b.worktrees[0].view.path.raw(), wt.to_str().unwrap());
    match &b.worktrees[0].view.status {
        WorktreeStatus::Ready { counts, .. } => assert_eq!(counts.unstaged, 1),
        other => panic!("{other:?}"),
    }
    assert!(b.marks.t_recv >= t0 || b.marks.t_recv > 0);
    assert!(b.marks.t_flush >= b.marks.t_recv && b.marks.t_computed >= b.marks.t_flush);
    // The effective window is about 75 ms (ADR-GRP-011 § 2).
    let window_ms = (b.marks.t_flush - b.marks.t_recv) / 1_000_000;
    assert!(window_ms >= 50, "window {window_ms} ms");
}

#[test]
fn commit_merge_rebase_and_push_are_named_by_the_reflog() {
    let (f, wt) = demo();
    let remote = f.root.join("remote.git");
    f.git(&["init", "-q", "--bare", remote.to_str().unwrap()]);
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let w = watch(&f, fast());

    // Two commits in one go: two events.
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    f.git_in(&wt, &["add", "a.txt"]);
    f.git_in(&wt, &["commit", "-q", "-m", "a"]);
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    f.git_in(&wt, &["add", "b.txt"]);
    f.git_in(&wt, &["commit", "-q", "-m", "b"]);
    let batches = w.until(|bs| {
        events(bs)
            .iter()
            .filter(|e| e.kind == GitEventKind::Commit)
            .count()
            >= 2
    });
    let commits: Vec<_> = events(&batches)
        .into_iter()
        .filter(|e| e.kind == GitEventKind::Commit)
        .collect();
    assert_eq!(commits.len(), 2, "{commits:#?}");
    for c in &commits {
        assert_eq!(c.worktree, wt);
        assert!(!c.details.worktree_inferred);
        assert_eq!(c.details.branch.as_ref().unwrap().raw(), "feat-login");
    }

    // Merge of main (with a commit of its own) into feat-login.
    f.write("m.txt", "m\n");
    f.git(&["add", "m.txt"]);
    f.git(&["commit", "-q", "-m", "m"]);
    w.drain(Duration::from_millis(300));
    f.git_in(&wt, &["merge", "-q", "--no-edit", "main"]);
    let merge = only(&w.events_until(GitEventKind::Merge), GitEventKind::Merge);
    assert_eq!(merge.worktree, wt);

    // Rebase onto a newer main: one rebase event, no branch switch.
    f.write("n.txt", "n\n");
    f.git(&["add", "n.txt"]);
    f.git(&["commit", "-q", "-m", "n"]);
    w.drain(Duration::from_millis(300));
    f.git_in(&wt, &["rebase", "-q", "main"]);
    let evs = w.events_until(GitEventKind::Rebase);
    assert_eq!(only(&evs, GitEventKind::Rebase).worktree, wt);
    assert!(
        evs.iter().all(|e| e.kind != GitEventKind::BranchSwitch),
        "{evs:#?}"
    );

    // Push with upstream: placed in the worktree of the local branch.
    w.drain(Duration::from_millis(300));
    f.git_in(&wt, &["push", "-q", "-u", "origin", "feat-login"]);
    let evs = w.events_until(GitEventKind::Push);
    let push = only(&evs, GitEventKind::Push);
    assert_eq!(push.worktree, wt);
    assert!(!push.details.worktree_inferred);
    assert_eq!(
        push.details.branch.as_ref().unwrap().raw(),
        "origin/feat-login"
    );
    assert!(evs.iter().all(|e| e.kind != GitEventKind::BranchCreate));
}

#[test]
fn branches_and_worktrees_are_followed() {
    let (f, wt) = demo();
    let w = watch(&f, fast());

    f.git_in(&wt, &["switch", "-q", "-c", "old-feature"]);
    let evs = w.events_until(GitEventKind::BranchSwitch);
    let evs = if evs.iter().any(|e| e.kind == GitEventKind::BranchCreate) {
        evs
    } else {
        let mut all = evs;
        all.extend(w.events_until(GitEventKind::BranchCreate));
        all
    };
    let created = only(&evs, GitEventKind::BranchCreate);
    assert_eq!(created.worktree, wt);
    assert!(!created.details.worktree_inferred);
    let switch = only(&evs, GitEventKind::BranchSwitch);
    assert_eq!(switch.details.from.as_ref().unwrap().raw(), "feat-login");
    assert_eq!(switch.details.branch.as_ref().unwrap().raw(), "old-feature");

    // Back, and delete it from the same worktree.
    f.git_in(&wt, &["switch", "-q", "feat-login"]);
    w.drain(Duration::from_millis(300));
    f.git_in(&wt, &["branch", "-q", "-D", "old-feature"]);
    let deleted = only(
        &w.events_until(GitEventKind::BranchDelete),
        GitEventKind::BranchDelete,
    );
    assert_eq!(deleted.worktree, wt);
    assert!(!deleted.details.worktree_inferred);

    // A worktree added and removed: its own events, its task follows.
    let extra = f.root.join("wt-extra");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "extra",
        extra.to_str().unwrap(),
    ]);
    let extra = canonical(&extra);
    let batches = w.until(|bs| {
        events(bs)
            .iter()
            .any(|e| e.kind == GitEventKind::WorktreeCreate)
    });
    assert_eq!(
        only(&events(&batches), GitEventKind::WorktreeCreate).worktree,
        extra
    );
    assert!(
        batches
            .iter()
            .flat_map(|b| &b.worktrees)
            .any(|r| r.view.path.raw() == extra.to_str().unwrap())
    );
    // The repo task registers the worktree right after it hands the batch over.
    wait_until("the worktree to be registered", || {
        w.observer.worktrees("r").contains(&extra)
    });
    f.git(&["worktree", "remove", extra.to_str().unwrap()]);
    let batches = w.until(|bs| {
        events(bs)
            .iter()
            .any(|e| e.kind == GitEventKind::WorktreeDelete)
    });
    assert_eq!(
        only(&events(&batches), GitEventKind::WorktreeDelete).worktree,
        extra
    );
    assert!(batches.iter().any(|b| b.gone.contains(&extra)));
    assert!(!w.observer.worktrees("r").contains(&extra));
    // No read of the removed worktree follows the batch that says it is
    // gone: it would bring it back as "missing".
    let gone_at = batches
        .iter()
        .position(|b| b.gone.contains(&extra))
        .unwrap();
    let later: Vec<_> = batches[gone_at..]
        .iter()
        .chain(&w.drain(Duration::from_millis(500)))
        .flat_map(|b| &b.worktrees)
        .filter(|r| r.view.path.raw() == extra.to_str().unwrap())
        .map(|r| r.view.status.clone())
        .collect();
    assert!(later.is_empty(), "{later:?}");
}

/// D6 fallback: Git does not say where `git branch x` ran; with two
/// worktrees at the same commit the event goes to the main worktree, marked
/// as inferred.
#[test]
fn a_branch_event_without_a_known_worktree_is_marked_inferred() {
    let (f, wt) = demo();
    let w = watch(&f, fast());
    f.git_in(&wt, &["branch", "nowhere"]);
    let created = only(
        &w.events_until(GitEventKind::BranchCreate),
        GitEventKind::BranchCreate,
    );
    assert_eq!(created.worktree, canonical(&f.repo));
    assert!(created.details.worktree_inferred);
}

/// Ten worktrees with a commit each at the same time: ten events, each in
/// its own worktree (NFR-05).
#[test]
fn ten_worktrees_commit_at_once_without_losing_events() {
    let f = Fixture::with_commit(&git_from_path());
    let mut wts = Vec::new();
    for i in 0..10 {
        let name = format!("w{i}");
        f.git(&["branch", &name]);
        wts.push(canonical(&f.add_worktree(&name, &name)));
    }
    let w = watch(&f, fast());
    std::thread::scope(|s| {
        for wt in &wts {
            let f = &f;
            s.spawn(move || {
                std::fs::write(wt.join("c.txt"), "c\n").unwrap();
                f.git_in(wt, &["add", "c.txt"]);
                f.git_in(wt, &["commit", "-q", "-m", "c"]);
            });
        }
    });
    let batches = w.until(|bs| {
        events(bs)
            .iter()
            .filter(|e| e.kind == GitEventKind::Commit)
            .count()
            >= 10
    });
    let mut places: Vec<_> = events(&batches)
        .into_iter()
        .filter(|e| e.kind == GitEventKind::Commit)
        .map(|e| e.worktree)
        .collect();
    places.sort();
    assert_eq!(places, wts);
}

/// A build writing into a directory Git ignores costs no recompute
/// (ADR-GRP-010 § 2, Enmienda 2026-10-05).
#[test]
fn a_burst_in_an_ignored_directory_costs_no_recompute() {
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(wt.join("target/debug")).unwrap();
    let w = watch(&f, fast());
    // Warm the cache with one write, then measure a sustained burst.
    std::fs::write(wt.join("target/debug/warm"), "x").unwrap();
    w.drain(Duration::from_millis(500));
    let before = w.observer.recomputes();
    let end = Instant::now() + Duration::from_secs(2);
    let mut i = 0;
    while Instant::now() < end {
        std::fs::write(wt.join(format!("target/debug/o{}", i % 50)), i.to_string()).unwrap();
        i += 1;
        std::thread::sleep(Duration::from_millis(2));
    }
    w.drain(Duration::from_millis(500));
    let during = w.observer.recomputes() - before;
    assert_eq!(during, 0, "{i} writes caused {during} recomputes");

    // A change outside it still arrives.
    std::fs::write(wt.join("login.txt"), "changed\n").unwrap();
    w.until(|bs| bs.iter().any(|b| !b.worktrees.is_empty()));
}

/// RES-01: the router drops the events of a folder its task already found ignored. NFR-01: once
/// `.gitignore` stops ignoring it, the files written while it was dropped are seen.
#[test]
fn a_folder_dropped_by_the_router_is_seen_once_no_longer_ignored() {
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(wt.join("target/debug")).unwrap();
    let w = watch(&f, fast());
    // The task learns the folder is ignored; from then on the router drops it.
    std::fs::write(wt.join("target/debug/warm"), "x").unwrap();
    w.drain(Duration::from_millis(500));
    let before = w.observer.recomputes();
    for i in 0..50 {
        std::fs::write(wt.join(format!("target/debug/o{i}")), "o").unwrap();
    }
    w.drain(Duration::from_millis(500));
    assert_eq!(w.observer.recomputes(), before);
    std::fs::write(wt.join(".gitignore"), "").unwrap();
    let untracked = |b: &ObservedBatch| {
        b.worktrees.iter().any(|r| {
            r.view.path.raw() == wt.to_str().unwrap()
                && matches!(&r.view.status, WorktreeStatus::Ready { counts, .. } if counts.untracked >= 2)
        })
    };
    w.until(|bs| bs.iter().any(untracked));
}

/// A change whose event was lost without a mark is recovered by the
/// periodic reconciliation, in a gap of its own (ADR-GRP-010 § 5).
#[test]
fn the_periodic_reconciliation_recovers_a_lost_change_in_a_gap() {
    let (f, wt) = demo();
    let config = WatchConfig {
        periodic: Duration::from_secs(1),
        ..config()
    };
    let w = watch(&f, config);
    w.observer.simulate_lost_events(true);
    std::fs::write(wt.join("login.txt"), "lost\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    w.observer.simulate_lost_events(false);
    let batches = w.until(|bs| bs.iter().any(|b| b.gap.is_some()));
    let b = batches.iter().find(|b| b.gap.is_some()).unwrap();
    let gap = b.gap.unwrap();
    assert_eq!(gap.cause, GapCause::PeriodicReconciliation);
    assert!(gap.started_ms <= gap.ended_ms);
    assert_eq!(b.worktrees[0].view.path.raw(), wt.to_str().unwrap());
    assert_eq!(only(&b.events, GitEventKind::Reconciled).worktree, wt);
}

/// An overflow always opens a gap, and what changed is reconciled into it
/// (ADR-GRP-013 § 5).
#[test]
fn an_overflow_reconciles_in_a_gap() {
    let (f, wt) = demo();
    let w = watch(&f, fast());
    w.observer.simulate_lost_events(true);
    std::fs::write(wt.join("login.txt"), "lost\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    w.observer.simulate_lost_events(false);
    w.observer.simulate_overflow();
    let batches = w.until(|bs| {
        bs.iter()
            .any(|b| b.gap.is_some() && !b.worktrees.is_empty())
    });
    let gaps: Vec<_> = batches.iter().filter_map(|b| b.gap).collect();
    assert!(gaps.iter().all(|g| g.cause == GapCause::WatcherOverflow));
    // Every task opened its gap, changed or not.
    assert!(gaps.len() >= 3, "{gaps:?}");
    let changed = batches
        .iter()
        .find(|b| b.gap.is_some() && !b.worktrees.is_empty())
        .unwrap();
    assert_eq!(only(&changed.events, GitEventKind::Reconciled).worktree, wt);
}

/// Claude Code nests worktrees in `.claude/worktrees/`: a change there
/// belongs to the nested worktree, by the longest watched prefix.
#[test]
fn a_nested_worktree_gets_its_own_changes() {
    let f = Fixture::with_commit(&git_from_path());
    f.write(".gitignore", ".claude/\n");
    f.git(&["add", ".gitignore"]);
    f.git(&["commit", "-q", "-m", "ignore"]);
    let nested = f.repo.join(".claude/worktrees/agent");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "agent",
        nested.to_str().unwrap(),
    ]);
    let nested = canonical(&nested);
    let w = watch(&f, fast());
    std::fs::write(nested.join("a.txt"), "agent\n").unwrap();
    let batches = w.until(|bs| bs.iter().any(|b| !b.worktrees.is_empty()));
    let paths: Vec<_> = batches
        .iter()
        .flat_map(|b| &b.worktrees)
        .map(|r| r.view.path.raw().to_owned())
        .collect();
    assert!(
        paths.contains(&nested.to_str().unwrap().to_owned()),
        "{paths:?}"
    );
}

/// Watching never writes to the repo or its worktrees (INF-GRP-001).
#[test]
fn repo_intact_watching_writes_nothing() {
    let (f, wt) = demo();
    let report = check("US-GRP-002 watch", &f, &Exceptions::none(), || {
        let w = watch(&f, fast());
        w.observer.simulate_overflow();
        w.drain(Duration::from_millis(500));
        drop(w);
    });
    report.assert_intact();
    let _ = wt;
}

/// Runs `args` in `wt` and stops Git right after it moves `branch`, with a
/// `reference-transaction` hook, so the view read then is the one between
/// Git's steps. Returns the events of a window flushed at that moment.
#[cfg(unix)]
fn events_while_git_finishes(f: &Fixture, wt: &Path, branch: &str, args: &[&str]) -> Vec<RawEvent> {
    use gitraptor_core::watch::{RefsView, classify};
    use std::os::unix::fs::PermissionsExt;

    let common = observe::locate(&f.repo).unwrap();
    let old = RefsView::read(&common);
    let paused = f.root.join("paused");
    let go = f.root.join("go");
    let hook = common.join("hooks/reference-transaction");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\n\
             [ \"$1\" = committed ] || exit 0\n\
             grep -q ' refs/heads/{branch}$' || exit 0\n\
             touch '{paused}'\n\
             i=0\n\
             while [ ! -f '{go}' ] && [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done\n",
            paused = paused.display(),
            go = go.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::thread::scope(|s| {
        let git = s.spawn(|| f.git_in(wt, args));
        let start = Instant::now();
        while !paused.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Git never paused"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let new = RefsView::read(&common);
        let in_progress = &new.worktrees[wt];
        // The race itself: the branch moved, the operation is not over.
        assert_eq!(in_progress.branch, None, "{in_progress:?}");
        let events = classify(&common, &old, &new, (0, 0));
        std::fs::write(&go, "").unwrap();
        git.join().unwrap();
        std::fs::remove_file(&hook).unwrap();
        events
    })
}

/// "demo" with a commit in "feat-login" and another in `main`.
#[cfg(unix)]
fn diverged() -> (Fixture, PathBuf) {
    let (f, wt) = demo();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    f.git_in(&wt, &["add", "a.txt"]);
    f.git_in(&wt, &["commit", "-q", "-m", "a"]);
    f.write("b.txt", "b\n");
    f.git(&["add", "b.txt"]);
    f.git(&["commit", "-q", "-m", "b"]);
    (f, wt)
}

/// D6: Git moves the branch of a rebase before it reattaches `HEAD`; a
/// window that closes in between places the rebase in its worktree, not
/// inferred.
#[cfg(unix)]
#[test]
fn a_rebase_seen_before_git_reattaches_head_is_placed_in_its_worktree() {
    let (f, wt) = diverged();
    let events = events_while_git_finishes(&f, &wt, "feat-login", &["rebase", "-q", "main"]);
    let rebase = only(&events, GitEventKind::Rebase);
    assert_eq!(rebase.worktree, wt);
    assert!(!rebase.details.worktree_inferred, "{rebase:#?}");
}

/// D6: the same for a merge, whose `MERGE_HEAD` is still there when Git
/// moves the branch.
#[cfg(unix)]
#[test]
fn a_merge_seen_before_git_clears_its_markers_is_placed_in_its_worktree() {
    let (f, wt) = diverged();
    let events =
        events_while_git_finishes(&f, &wt, "feat-login", &["merge", "-q", "--no-edit", "main"]);
    let merge = only(&events, GitEventKind::Merge);
    assert_eq!(merge.worktree, wt);
    assert!(!merge.details.worktree_inferred, "{merge:#?}");
}

/// A branch that moves under its worktree without a write in the
/// worktree's own Git directory (a commit's last step, or `update-ref`)
/// wakes the worktree's task: its state follows without waiting for the
/// periodic reconciliation (ADR-GRP-010 § 5).
#[test]
fn a_branch_moved_under_its_worktree_is_read_again() {
    let (f, wt) = demo();
    f.write("b.txt", "b\n");
    f.git(&["add", "b.txt"]);
    f.git(&["commit", "-q", "-m", "b"]);
    let tip = f.git(&["rev-parse", "main"]).trim().to_owned();
    let w = watch(&f, fast());
    f.git(&["update-ref", "refs/heads/feat-login", &tip]);
    w.until(|bs| {
        bs.iter().flat_map(|b| &b.worktrees).any(|r| {
            r.view.path.raw() == wt.to_str().unwrap() && r.head_commit.as_deref() == Some(&tip)
        })
    });
}

/// US-TMC-004: a batch carries the `HEAD` read before the worktree. When a
/// `git` renames `HEAD` in between, the read already shows the new state and
/// the next window reads nothing new: it still hands the new `HEAD` over, or
/// the Time Machine waits for a calm that never comes (`repo-busy`). Here
/// `HEAD` changes its bytes and nothing a read shows: same branch, no final
/// newline.
#[test]
fn a_head_that_changes_after_the_read_is_handed_over() {
    let (f, wt) = demo();
    let git_dir = gitraptor_core::timemachine::engine::git_dir_of(&wt).unwrap();
    let w = watch(&f, fast());
    let head = b"ref: refs/heads/feat-login".to_vec();
    let tmp = git_dir.join("HEAD.lock");
    std::fs::write(&tmp, &head).unwrap();
    std::fs::rename(&tmp, git_dir.join("HEAD")).unwrap();
    let batches = w.until(|bs| {
        bs.iter()
            .flat_map(|b| &b.heads)
            .any(|(root, h)| root == &wt && h == &head)
    });
    // A `HEAD` handed over again, not a change: no read and no event.
    assert!(
        batches.iter().all(|b| b.worktrees.is_empty()),
        "{batches:#?}"
    );
    assert!(events(&batches).is_empty(), "{batches:#?}");
}

/// The reflog is read after the view of the refs: a commit that lands in
/// between is left to the next window, which names it, instead of being
/// named twice.
#[test]
fn a_commit_after_the_view_is_named_once_by_the_next_window() {
    use gitraptor_core::watch::{RefsView, classify};

    let (f, wt) = demo();
    let common = observe::locate(&f.repo).unwrap();
    let commit = |file: &str| {
        std::fs::write(wt.join(file), "x\n").unwrap();
        f.git_in(&wt, &["add", file]);
        f.git_in(&wt, &["commit", "-q", "-m", file]);
    };
    let first = RefsView::read(&common);
    commit("a.txt");
    let second = RefsView::read(&common);
    commit("b.txt");
    let third = RefsView::read(&common);
    let named: Vec<_> = [(&first, &second), (&second, &third)]
        .iter()
        .flat_map(|(old, new)| classify(&common, old, new, (0, 0)))
        .filter(|e| e.kind == GitEventKind::Commit)
        .map(|e| e.details.new_commit.unwrap())
        .collect();
    assert_eq!(
        named,
        [
            second.branches["feat-login"].clone(),
            third.branches["feat-login"].clone()
        ]
    );
}

/// US-TMC-004: a `reset --hard` that keeps the branch where it is only
/// shows in the worktree's `HEAD` reflog; it is a `reset` event, once. A
/// reset that moves the branch stays a `branch-update`, never both.
#[test]
fn a_reset_that_moves_no_branch_is_a_reset_event() {
    let (f, wt) = demo();
    let w = watch(&f, fast());
    std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();
    f.git_in(&wt, &["reset", "-q", "--hard"]);
    let all = w.events_until(GitEventKind::Reset);
    let e = only(&all, GitEventKind::Reset);
    assert_eq!(e.worktree, wt);
    assert_eq!(e.details.branch.as_ref().unwrap().raw(), "feat-login");
    assert_eq!(e.details.old_commit, e.details.new_commit);
    assert!(
        !all.iter().any(|e| e.kind == GitEventKind::BranchUpdate),
        "{all:#?}"
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("login.txt")).unwrap(),
        "user\n"
    );

    // A reset that moves the branch: its branch-update only.
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    f.git_in(&wt, &["add", "a.txt"]);
    f.git_in(&wt, &["commit", "-q", "-m", "a"]);
    w.events_until(GitEventKind::Commit);
    f.git_in(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
    let all = w.events_until(GitEventKind::BranchUpdate);
    assert!(
        !all.iter().any(|e| e.kind == GitEventKind::Reset),
        "{all:#?}"
    );
}

// --- Ignored folders and the OS stream (ADR-GRP-010, Enmienda 2026-10-08) ---

/// Polls `cond` until it holds, or 10 s.
fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn counts_of(b: &ObservedBatch, wt: &Path) -> Option<(u32, u32)> {
    b.worktrees.iter().find_map(|r| match &r.view.status {
        WorktreeStatus::Ready { counts, .. } if r.view.path.raw() == wt.to_str().unwrap() => {
            Some((counts.unstaged, counts.untracked))
        }
        _ => None,
    })
}

fn burst(dir: &Path, files: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for i in 0..files {
        std::fs::write(dir.join(format!("o{i}")), "o").unwrap();
    }
}

/// Learns that `target/` is ignored and has the router count a sustained burst under it.
fn heat(wt: &Path) {
    std::fs::create_dir_all(wt.join("target/debug")).unwrap();
    std::fs::write(wt.join("target/debug/warm"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    burst(&wt.join("target/debug"), 600);
}

/// NFR-01 (E3): Git keeps following a tracked file under an ignored folder, so a change to it is
/// seen even when the router already dropped the folder's other events.
#[test]
fn a_tracked_file_under_an_ignored_folder_is_seen() {
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(wt.join("target/debug")).unwrap();
    std::fs::write(wt.join("target/debug/keep"), "v1\n").unwrap();
    f.git_in(&wt, &["add", "-f", "target/debug/keep", ".gitignore"]);
    f.git_in(&wt, &["commit", "-q", "-m", "track a file under target"]);
    let w = watch(&f, fast());
    std::fs::write(wt.join("target/debug/warm"), "x").unwrap();
    w.drain(Duration::from_millis(500));
    std::fs::write(wt.join("target/debug/keep"), "v2\n").unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1))
    });
}

/// NFR-01 (E5): `git add -f` of a file under a folder the router already drops makes the folder
/// followed again, and the next change to the file is seen.
#[test]
fn a_file_added_with_force_under_a_dropped_folder_is_seen() {
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(wt.join("target/debug")).unwrap();
    let w = watch(&f, fast());
    std::fs::write(wt.join("target/debug/warm"), "x").unwrap();
    w.drain(Duration::from_millis(500));
    std::fs::write(wt.join("target/debug/late"), "v1\n").unwrap();
    f.git_in(&wt, &["add", "-f", "target/debug/late"]);
    w.drain(Duration::from_millis(500));
    std::fs::write(wt.join("target/debug/late"), "v2\n").unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1))
    });
}

/// E3: a folder with tracked entries is never a candidate to leave the stream.
#[cfg(target_os = "macos")]
#[test]
fn a_folder_with_tracked_entries_never_leaves_the_stream() {
    if !excluding() {
        return;
    }
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(wt.join("target/debug")).unwrap();
    std::fs::write(wt.join("target/debug/keep"), "v1\n").unwrap();
    f.git_in(&wt, &["add", "-f", "target/debug/keep", ".gitignore"]);
    f.git_in(&wt, &["commit", "-q", "-m", "track a file under target"]);
    let w = watch(&f, fast());
    heat(&wt);
    w.drain(Duration::from_secs(1));
    assert_eq!(w.observer.excluded_folders(&wt), Vec::<PathBuf>::new());
    std::fs::write(wt.join("target/debug/keep"), "v2\n").unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1))
    });
}

/// E3: an ignored folder under sustained churn leaves the root's stream.
#[cfg(target_os = "macos")]
#[test]
fn a_folder_with_sustained_churn_leaves_the_stream() {
    if !excluding() {
        return;
    }
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    let w = watch(&f, fast());
    heat(&wt);
    wait_until("the exclusion", || {
        w.observer.excluded_folders(&wt) == vec![wt.join("target")]
    });
    // A change outside it still arrives, through the new stream, as fresh as before: from the
    // write to the stream's callback (`t_recv`) and to the computed state (ADR-GRP-011).
    w.drain(Duration::from_millis(500));
    let t0 = gitraptor_api::clock::monotonic_ns();
    std::fs::write(wt.join("login.txt"), "changed\n").unwrap();
    let batches = w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1))
    });
    let b = batches
        .iter()
        .find(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1))
        .unwrap();
    let detection_ms = b.marks.t_recv.saturating_sub(t0) / 1_000_000;
    let state_ms = b.marks.t_computed.saturating_sub(t0) / 1_000_000;
    eprintln!(
        "freshness with the folder left out: detection {detection_ms} ms, state {state_ms} ms"
    );
    // Debug build on a shared machine: a loose bound; the gate is the engine bench.
    assert!(state_ms < 500, "state {state_ms} ms after the write");
}

/// NFR-01 (E5): a folder that stops being ignored while left out of the stream is followed again,
/// and what was written while it was out is seen.
#[cfg(target_os = "macos")]
#[test]
fn a_left_out_folder_that_stops_being_ignored_is_seen_again() {
    if !excluding() {
        return;
    }
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    let w = watch(&f, fast());
    heat(&wt);
    wait_until("the exclusion", || {
        !w.observer.excluded_folders(&wt).is_empty()
    });
    // Written while the folder is out of the stream: no event of it was delivered.
    burst(&wt.join("target/out"), 5);
    std::fs::write(wt.join(".gitignore"), "").unwrap();
    wait_until("the exclusion to end", || {
        w.observer.excluded_folders(&wt).is_empty()
    });
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.1 >= 600))
    });
    // The stream without the exclusion delivers the folder again.
    std::fs::write(wt.join("target/debug/after"), "x").unwrap();
    std::fs::write(wt.join("login.txt"), "changed\n").unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.1 >= 607))
    });
}

/// NFR-01 (E5): `git add -f` under a folder left out of the stream takes it back, and the next
/// change to the file is seen.
#[cfg(target_os = "macos")]
#[test]
fn a_file_added_with_force_under_a_left_out_folder_is_seen() {
    if !excluding() {
        return;
    }
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    let w = watch(&f, fast());
    heat(&wt);
    wait_until("the exclusion", || {
        !w.observer.excluded_folders(&wt).is_empty()
    });
    f.git_in(&wt, &["add", "-f", "target/debug/o1"]);
    wait_until("the exclusion to end", || {
        w.observer.excluded_folders(&wt).is_empty()
    });
    w.drain(Duration::from_millis(500));
    std::fs::write(wt.join("target/debug/o1"), "changed\n").unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1))
    });
}

/// NFR-01 (E5): a rule that changes outside the repo (`core.excludesFile`) reaches no event; the
/// periodic reconciliation checks the left-out folders again and takes the folder back.
#[test]
fn the_periodic_reconciliation_takes_back_a_folder_no_longer_ignored() {
    let (f, wt) = demo();
    let rules = f.root.join("global-ignore");
    std::fs::write(&rules, "target/\n").unwrap();
    f.git_in(
        &wt,
        &["config", "core.excludesFile", rules.to_str().unwrap()],
    );
    let config = WatchConfig {
        periodic: Duration::from_secs(1),
        ..config()
    };
    let w = watch(&f, config);
    heat(&wt);
    if excluding() {
        wait_until("the exclusion", || {
        !w.observer.excluded_folders(&wt).is_empty()
    });
    }
    w.drain(Duration::from_millis(500));
    std::fs::write(&rules, "").unwrap();
    if excluding() {
        wait_until("the exclusion to end", || {
        w.observer.excluded_folders(&wt).is_empty()
    });
    }
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.1 >= 600))
    });
    std::fs::write(wt.join("target/debug/after"), "x").unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.1 >= 602))
    });
}

/// E4: the stream that takes over from another starts where the old one stopped, so writes outside
/// the folder during the replacement are all seen (0 lost).
#[cfg(target_os = "macos")]
#[test]
fn replacing_the_stream_under_writes_loses_nothing() {
    if !excluding() {
        return;
    }
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(wt.join("gen")).unwrap();
    let w = watch(&f, fast());
    let writer = {
        let dir = wt.join("gen");
        std::thread::spawn(move || {
            for i in 0..1_500 {
                std::fs::write(dir.join(format!("g{i}")), "g").unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    heat(&wt);
    wait_until("the exclusion", || {
        !w.observer.excluded_folders(&wt).is_empty()
    });
    writer.join().unwrap();
    w.until(|bs| {
        bs.iter()
            .any(|b| counts_of(b, &wt).is_some_and(|c| c.1 == 1_501))
    });
}

/// The `notify` fallback (`engine.watcher.backend = "notify"`) is what ran before the engine's own
/// stream: it never leaves a folder out, and the router still drops what is ignored.
#[cfg(target_os = "macos")]
#[test]
fn the_notify_backend_never_leaves_a_folder_out_of_the_stream() {
    if BACKEND != WatchBackend::Notify {
        return;
    }
    let (f, wt) = demo();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    let w = watch(&f, fast());
    assert_eq!(w.observer.backend_name(), "notify");
    heat(&wt);
    let before = w.observer.recomputes();
    w.drain(Duration::from_secs(1));
    assert_eq!(w.observer.excluded_folders(&wt), Vec::<PathBuf>::new());
    assert_eq!(w.observer.recomputes(), before);
    // And what is outside still arrives.
    std::fs::write(wt.join("login.txt"), "changed\n").unwrap();
    w.until(|bs| bs.iter().any(|b| counts_of(b, &wt).is_some_and(|c| c.0 >= 1)));
}

/// The backend the observer reports is the one it was configured with.
#[test]
fn the_observer_reports_its_backend() {
    let (f, _wt) = demo();
    let w = watch(&f, fast());
    let expected = if excluding() { "fsevents" } else { "notify" };
    assert_eq!(w.observer.backend_name(), expected);
}
