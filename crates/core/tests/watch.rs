//! US-GRP-002: the change observer on temporary repos of the "intact repo"
//! harness (NFR-01), without the daemon: what it sees, how it names Git
//! events and how it recovers what it missed. Runs on every OS of the CI;
//! the timings are checked on macOS only (Pendiente: etapa de validación
//! multiplataforma).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{GitEventKind, WorktreeStatus};
use gitraptor_core::observe::{self, reconcile};
use gitraptor_core::profile::GapCause;
use gitraptor_core::watch::{ObservedBatch, Observer, RawEvent, WatchConfig};
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

fn fast() -> WatchConfig {
    WatchConfig::default()
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
    assert!(w.observer.worktrees("r").contains(&extra));
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

/// A change whose event was lost without a mark is recovered by the
/// periodic reconciliation, in a gap of its own (ADR-GRP-010 § 5).
#[test]
fn the_periodic_reconciliation_recovers_a_lost_change_in_a_gap() {
    let (f, wt) = demo();
    let config = WatchConfig {
        periodic: Duration::from_secs(1),
        ..WatchConfig::default()
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
    assert!(!all.iter().any(|e| e.kind == GitEventKind::BranchUpdate), "{all:#?}");
    assert_eq!(std::fs::read_to_string(wt.join("login.txt")).unwrap(), "user\n");

    // A reset that moves the branch: its branch-update only.
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    f.git_in(&wt, &["add", "a.txt"]);
    f.git_in(&wt, &["commit", "-q", "-m", "a"]);
    w.events_until(GitEventKind::Commit);
    f.git_in(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
    let all = w.events_until(GitEventKind::BranchUpdate);
    assert!(!all.iter().any(|e| e.kind == GitEventKind::Reset), "{all:#?}");
}
