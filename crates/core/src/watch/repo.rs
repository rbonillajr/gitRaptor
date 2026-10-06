//! The task of one repo: refs, reflogs and `.git/worktrees/` (ADR-GRP-010
//! § 2), and the Git events they mean (US-GRP-002 Dev Spec, D5 and D6).
//!
//! Each window compares the previous view of the repo with a new one and
//! names what changed by reading the reflog of each ref since its previous
//! tip: one event per entry, so two commits in one window are two events.
//! The backup poll (every 30 s) does the same, and also compares a cheap
//! fingerprint of each worktree (`HEAD`, index, operation markers) to wake
//! a worktree whose event was lost.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Instant;

use gitraptor_api::UntrustedName;
use gitraptor_api::clock;
use gitraptor_api::messages::{GitEventDetails, GitEventKind};
use gitraptor_git::{ReaderOptions, RepoReader};

use super::{GapMark, Marks, ObservedBatch, RawEvent, RepoMsg, Shared, WtMsg, wall_now};
use crate::observe::{self, linked_is_trusted};
use crate::profile::GapCause;

/// Most reflog entries read per ref and window.
const MAX_REFLOG_ENTRIES: usize = 64;

/// Most bytes of a `HEAD` reflog read per window: only its new tail.
const MAX_HEAD_LOG_TAIL: u64 = 64 * 1024;

/// One worktree as the repo task sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRefs {
    pub main: bool,
    pub admin: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    /// Last entry of its `HEAD` reflog (`checkout: moving from a to b`).
    pub head_reflog: Option<String>,
    /// Size of its `HEAD` reflog file: what grew since the previous view is
    /// read for resets that move no branch (US-TMC-004).
    pub head_log_len: u64,
    /// Branch its operation in progress works on: Git moves the branch
    /// before it reattaches `HEAD` or removes the markers.
    pub operating_on: Option<String>,
    /// Cheap fingerprint for the backup poll.
    pub fingerprint: String,
}

/// The refs side of a repo at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefsView {
    pub branches: BTreeMap<String, String>,
    pub remotes: BTreeMap<String, String>,
    /// By canonical root. Untrusted links are left out (SEC-11).
    pub worktrees: BTreeMap<PathBuf, WorktreeRefs>,
}

impl RefsView {
    /// Size of each worktree's `HEAD` reflog.
    pub fn head_logs(&self) -> Vec<(PathBuf, u64)> {
        self.worktrees
            .iter()
            .map(|(root, w)| (root.clone(), w.head_log_len))
            .collect()
    }

    /// Reads the view; what cannot be read is left out.
    pub fn read(common: &Path) -> Self {
        let mut view = Self::default();
        let Ok(reader) = RepoReader::open(common, &ReaderOptions::default()) else {
            return view;
        };
        for b in reader.local_branches().unwrap_or_default() {
            view.branches.insert(b.name, b.commit);
        }
        for b in reader.remote_branches().unwrap_or_default() {
            view.remotes.insert(b.name, b.commit);
        }
        if !reader.is_bare()
            && let Some(main) = reader.workdir()
        {
            let root = observe::canonical(&main);
            view.worktrees
                .insert(root.clone(), worktree_refs(&root, common, true, None));
        }
        for w in reader.worktrees().unwrap_or_default() {
            let root = observe::canonical(&w.path);
            if linked_is_trusted(common, &w.id, &root) {
                let refs = worktree_refs(
                    &root,
                    &common.join("worktrees").join(&w.id),
                    false,
                    Some(w.id),
                );
                view.worktrees.insert(root, refs);
            }
        }
        view
    }

    /// Where a branch's event happened (D6): the worktree that has it in
    /// `HEAD`; else the one whose operation in progress works on it; else
    /// the one whose last `HEAD` move names it; else the only one at its
    /// commit (inferred); else the main one (inferred).
    pub fn place(&self, branch: &str, commit: Option<&str>) -> Option<EventPlace> {
        let place = |root: &PathBuf, inferred| EventPlace {
            worktree: root.clone(),
            inferred,
        };
        if let Some((root, _)) = self
            .worktrees
            .iter()
            .find(|(_, w)| w.branch.as_deref() == Some(branch))
        {
            return Some(place(root, false));
        }
        if let Some((root, _)) = self
            .worktrees
            .iter()
            .find(|(_, w)| w.operating_on.as_deref() == Some(branch))
        {
            return Some(place(root, false));
        }
        if let Some((root, _)) = self.worktrees.iter().find(|(_, w)| {
            w.head_reflog
                .as_deref()
                .is_some_and(|m| names_branch(m, branch))
        }) {
            return Some(place(root, false));
        }
        if let Some(commit) = commit {
            let at: Vec<_> = self
                .worktrees
                .iter()
                .filter(|(_, w)| w.commit.as_deref() == Some(commit))
                .collect();
            if let [(root, _)] = at.as_slice() {
                return Some(place(root, true));
            }
        }
        self.worktrees
            .iter()
            .find(|(_, w)| w.main)
            .or_else(|| self.worktrees.iter().next())
            .map(|(root, _)| place(root, true))
    }
}

/// The worktree an event is placed in, and whether it was inferred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPlace {
    pub worktree: PathBuf,
    pub inferred: bool,
}

fn worktree_refs(root: &Path, git_dir: &Path, main: bool, admin: Option<String>) -> WorktreeRefs {
    let (branch, commit, head_reflog, operating_on) =
        match RepoReader::open(root, &ReaderOptions::default()) {
            Ok(r) => {
                let head = r.head().ok();
                let in_progress = r.in_progress().is_some();
                let symbolic = head
                    .as_ref()
                    .filter(|h| !h.detached)
                    .and_then(|h| h.branch.clone());
                let operating_on = if in_progress {
                    r.rebase_branch().or_else(|| symbolic.clone())
                } else {
                    None
                };
                (
                    symbolic.filter(|_| !in_progress),
                    head.and_then(|h| h.commit),
                    r.reflog_last("HEAD").ok().flatten().map(|e| e.message),
                    operating_on,
                )
            }
            Err(_) => (None, None, None, None),
        };
    WorktreeRefs {
        main,
        admin,
        branch,
        commit,
        head_reflog,
        operating_on,
        head_log_len: head_log_len(git_dir),
        fingerprint: fingerprint(git_dir),
    }
}

/// Size of a worktree's `HEAD` reflog, without following links; 0 if
/// there is none.
fn head_log_len(git_dir: &Path) -> u64 {
    std::fs::symlink_metadata(git_dir.join("logs").join("HEAD"))
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |m| m.len())
}

/// The entries appended to a worktree's `HEAD` reflog from byte `from` on,
/// at most [`MAX_HEAD_LOG_TAIL`] bytes of them, as `(old, new, message)`.
/// Metadata only: the file is the repo's, read without following links.
fn head_log_tail(git_dir: &Path, from: u64, to: u64) -> Vec<(String, String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let path = git_dir.join("logs").join("HEAD");
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) || to <= from {
        return Vec::new();
    }
    let start = from.max(to.saturating_sub(MAX_HEAD_LOG_TAIL));
    let Ok(mut file) = std::fs::File::open(&path) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err()
        || file.take(to - start).read_to_end(&mut buf).is_err()
    {
        return Vec::new();
    }
    // A cut tail starts in the middle of a line: skip to the next one.
    if start > from {
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                buf.drain(..=i);
            }
            None => return Vec::new(),
        }
    }
    String::from_utf8_lossy(&buf)
        .lines()
        .filter_map(|line| {
            let (head, message) = line.split_once('\t')?;
            let mut parts = head.split(' ');
            let old = parts.next()?.to_owned();
            let new = parts.next()?.to_owned();
            Some((old, new, message.to_owned()))
        })
        .collect()
}

/// `HEAD`, size and mtime of the index, and which operation markers exist
/// (ADR-GRP-010 § 5). Metadata only.
fn fingerprint(git_dir: &Path) -> String {
    let mut out = std::fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();
    if let Ok(meta) = std::fs::metadata(git_dir.join("index")) {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        out.push_str(&format!("|{}|{mtime}", meta.len()));
    }
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
    ] {
        if git_dir.join(marker).exists() {
            out.push('|');
            out.push_str(marker);
        }
    }
    out
}

/// `checkout: moving from a to b` names both `a` and `b`.
fn names_branch(message: &str, branch: &str) -> bool {
    let Some(rest) = message.strip_prefix("checkout: moving from ") else {
        return false;
    };
    match rest.rsplit_once(" to ") {
        Some((from, to)) => from == branch || to == branch,
        None => false,
    }
}

/// What the reflog says one entry was.
fn kind_of(message: &str) -> GitEventKind {
    if message.starts_with("commit (merge)") || message.starts_with("merge ") {
        GitEventKind::Merge
    } else if message.starts_with("commit") || message.starts_with("cherry-pick") {
        GitEventKind::Commit
    } else if message.starts_with("rebase") {
        GitEventKind::Rebase
    } else {
        GitEventKind::BranchUpdate
    }
}

/// The Git events between two views of a repo (D5, D6). `common` is read
/// for the reflogs of the refs that moved.
pub fn classify(common: &Path, old: &RefsView, new: &RefsView, now: (i64, i32)) -> Vec<RawEvent> {
    let reader = RepoReader::open(common, &ReaderOptions::default()).ok();
    let mut events = Vec::new();
    let mut push = |place: Option<EventPlace>, kind, mut details: GitEventDetails| {
        if let Some(place) = place {
            details.worktree_inferred = place.inferred;
            events.push(RawEvent {
                worktree: place.worktree,
                kind,
                details,
                observed_ms: now.0,
                offset_s: now.1,
            });
        }
    };
    let here = |root: &PathBuf| {
        Some(EventPlace {
            worktree: root.clone(),
            inferred: false,
        })
    };

    for (root, w) in &new.worktrees {
        if !old.worktrees.contains_key(root) {
            push(
                here(root),
                GitEventKind::WorktreeCreate,
                GitEventDetails {
                    branch: w.branch.clone().map(UntrustedName::new),
                    new_commit: w.commit.clone(),
                    ..GitEventDetails::default()
                },
            );
        }
    }
    for (root, w) in &old.worktrees {
        if !new.worktrees.contains_key(root) {
            push(
                here(root),
                GitEventKind::WorktreeDelete,
                GitEventDetails {
                    branch: w.branch.clone().map(UntrustedName::new),
                    old_commit: w.commit.clone(),
                    ..GitEventDetails::default()
                },
            );
        }
    }

    // A reset that moves no branch, or with `HEAD` detached, only shows in
    // the `HEAD` reflog of its worktree (US-TMC-004). One that moves a
    // branch is that branch's `branch-update`, below.
    for (root, w) in &new.worktrees {
        let Some(before) = old.worktrees.get(root) else {
            continue;
        };
        if w.head_log_len <= before.head_log_len {
            continue;
        }
        let git_dir = match (&w.main, &w.admin) {
            (true, _) => common.to_path_buf(),
            (false, Some(admin)) => common.join("worktrees").join(admin),
            (false, None) => continue,
        };
        for (old_commit, new_commit, message) in
            head_log_tail(&git_dir, before.head_log_len, w.head_log_len)
        {
            if message.starts_with("reset:") && (old_commit == new_commit || w.branch.is_none()) {
                push(
                    here(root),
                    GitEventKind::Reset,
                    GitEventDetails {
                        branch: w.branch.clone().map(UntrustedName::new),
                        old_commit: Some(old_commit),
                        new_commit: Some(new_commit),
                        ..GitEventDetails::default()
                    },
                );
            }
        }
    }

    for (name, tip) in &new.branches {
        let details = |old_commit: Option<&String>, new_commit: &str| GitEventDetails {
            branch: Some(UntrustedName::new(name.clone())),
            old_commit: old_commit.cloned(),
            new_commit: Some(new_commit.to_owned()),
            ..GitEventDetails::default()
        };
        match old.branches.get(name) {
            None => push(
                new.place(name, Some(tip)),
                GitEventKind::BranchCreate,
                details(None, tip),
            ),
            Some(before) if before != tip => {
                let mut entries = reader
                    .as_ref()
                    .and_then(|r| {
                        r.reflog_since(
                            &format!("refs/heads/{name}"),
                            Some(before),
                            MAX_REFLOG_ENTRIES,
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                // The reflog is read after the view: entries newer than the
                // view's tip belong to the next window, which names them.
                if let Some(i) = entries.iter().position(|e| e.new == *tip) {
                    entries.drain(..i);
                }
                let place = new.place(name, Some(tip));
                if entries.is_empty() {
                    push(
                        place,
                        GitEventKind::BranchUpdate,
                        details(Some(before), tip),
                    );
                    continue;
                }
                for e in entries.iter().rev() {
                    push(
                        place.clone(),
                        kind_of(&e.message),
                        details(Some(&e.old), &e.new),
                    );
                }
            }
            Some(_) => {}
        }
    }
    for (name, tip) in &old.branches {
        if !new.branches.contains_key(name) {
            // Placed by the view before: the branch is gone from the new one.
            let mut place = new.place(name, Some(tip));
            if place.as_ref().is_none_or(|p| p.inferred)
                && let Some(p) = old.place(name, Some(tip)).filter(|p| !p.inferred)
                && new.worktrees.contains_key(&p.worktree)
            {
                place = Some(p);
            }
            push(
                place,
                GitEventKind::BranchDelete,
                GitEventDetails {
                    branch: Some(UntrustedName::new(name.clone())),
                    old_commit: Some(tip.clone()),
                    ..GitEventDetails::default()
                },
            );
        }
    }

    // A remote-tracking branch moved by `git push` (never a branch-create);
    // a fetch is not an event of this story.
    for (name, tip) in &new.remotes {
        let before = old.remotes.get(name);
        if before == Some(tip) {
            continue;
        }
        let pushed = reader
            .as_ref()
            .and_then(|r| {
                r.reflog_since(
                    &format!("refs/remotes/{name}"),
                    before.map(String::as_str),
                    MAX_REFLOG_ENTRIES,
                )
                .ok()
            })
            .unwrap_or_default()
            .iter()
            .any(|e| e.message == "update by push");
        if !pushed {
            continue;
        }
        let local = reader
            .as_ref()
            .and_then(|r| {
                new.branches
                    .keys()
                    .find(|b| r.branch_upstream(b).as_deref() == Some(name.as_str()))
                    .cloned()
            })
            .or_else(|| {
                let short = name.split_once('/').map(|(_, b)| b)?;
                new.branches.contains_key(short).then(|| short.to_owned())
            });
        let place = match &local {
            Some(b) => new.place(b, new.branches.get(b).map(String::as_str)),
            None => new.place(name, Some(tip)),
        };
        push(
            place,
            GitEventKind::Push,
            GitEventDetails {
                branch: Some(UntrustedName::new(name.clone())),
                old_commit: before.cloned(),
                new_commit: Some(tip.clone()),
                ..GitEventDetails::default()
            },
        );
    }
    events
}

struct Task {
    shared: Arc<Shared>,
    repo_id: String,
    common: PathBuf,
    view: RefsView,
    window: Option<(Instant, u64, bool)>,
    last_event_ms: i64,
    next_poll: Instant,
    /// A `HEAD` reflog grew alone in the last flush: its window was
    /// extended once.
    heads_deferred: bool,
}

pub(super) fn run(
    shared: Arc<Shared>,
    repo_id: String,
    common: PathBuf,
    view: RefsView,
    rx: Receiver<RepoMsg>,
) {
    let config = shared.config;
    let mut task = Task {
        shared,
        repo_id,
        common,
        view,
        window: None,
        last_event_ms: wall_now().0,
        next_poll: Instant::now() + config.backup_poll,
        heads_deferred: false,
    };
    let len = config.window.saturating_sub(config.timer_slack);
    loop {
        let deadline = task.window.map_or(task.next_poll, |(d, _, _)| d);
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(RepoMsg::Paths(t_recv)) => {
                task.last_event_ms = wall_now().0;
                task.window
                    .get_or_insert((Instant::now() + len, t_recv, false));
            }
            Ok(RepoMsg::Rescan(t_recv)) => {
                let w = task
                    .window
                    .get_or_insert((Instant::now() + len, t_recv, false));
                w.2 = true;
            }
            Ok(RepoMsg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if let Some((due, t_recv, overflow)) = task.window
            && due <= now
        {
            task.window = None;
            task.flush(t_recv, overflow);
        } else if task.window.is_none() && task.next_poll <= now {
            task.next_poll = now + config.backup_poll;
            task.poll();
        }
    }
}

impl Task {
    fn flush(&mut self, t_recv: u64, overflow: bool) {
        let t_flush = clock::monotonic_ns();
        let new = RefsView::read(&self.common);
        let started = self.last_event_ms;
        self.apply(
            new,
            Marks {
                t_recv,
                t_flush,
                t_computed: 0,
            },
            overflow.then_some(started),
        );
    }

    /// Backup poll: refs, `HEAD`s, indexes and markers, compared with what
    /// the task last saw. A worktree whose fingerprint moved without an
    /// event is reconciled by its own task.
    fn poll(&mut self) {
        let t_flush = clock::monotonic_ns();
        let new = RefsView::read(&self.common);
        let moved: Vec<PathBuf> = new
            .worktrees
            .iter()
            .filter(|(root, w)| {
                self.view
                    .worktrees
                    .get(*root)
                    .is_some_and(|old| old.fingerprint != w.fingerprint)
            })
            .map(|(root, _)| root.clone())
            .collect();
        for wt in self.shared.worktrees_of(&self.repo_id) {
            if moved.contains(&wt.root) {
                let _ = wt.tx.send(WtMsg::Reconcile(t_flush));
            }
        }
        self.apply(
            new,
            Marks {
                t_recv: t_flush,
                t_flush,
                t_computed: 0,
            },
            None,
        );
    }

    fn apply(&mut self, new: RefsView, mut marks: Marks, overflow_from: Option<i64>) {
        let now = wall_now();
        let events = classify(&self.common, &self.view, &new, now);
        // Worktrees added and removed: their tasks and watches follow.
        let mut created = Vec::new();
        for (root, w) in &new.worktrees {
            if !self.view.worktrees.contains_key(root) {
                let read = observe::read_worktree(root, w.main, w.admin.as_deref());
                created.push((read, w.admin.clone()));
            }
        }
        let gone: Vec<PathBuf> = self
            .view
            .worktrees
            .keys()
            .filter(|root| !new.worktrees.contains_key(*root))
            .cloned()
            .collect();
        marks.t_computed = clock::monotonic_ns();
        let refs_changed = new.branches != self.view.branches;
        // A worktree whose `HEAD` commit moved: a commit writes the index
        // and then the branch, which only this task sees. Its own task may
        // have read in between, so it reads again.
        let moved: Vec<PathBuf> = new
            .worktrees
            .iter()
            .filter(|(root, w)| {
                self.view
                    .worktrees
                    .get(*root)
                    .is_some_and(|old| old.commit != w.commit)
            })
            .map(|(root, _)| root.clone())
            .collect();
        let mut tips: Vec<String> = new
            .branches
            .iter()
            .map(|(n, c)| format!("{n} {c}"))
            .collect();
        tips.sort();
        let gap = overflow_from.map(|started_ms| GapMark {
            cause: GapCause::WatcherOverflow,
            started_ms,
            ended_ms: now.0,
        });
        // A `HEAD` reflog that grew without an event (a detached commit, a
        // checkout of the same branch) still tells the Time Machine the
        // engine saw it.
        let heads_changed = new.head_logs() != self.view.head_logs();
        let others = !events.is_empty()
            || !created.is_empty()
            || !gone.is_empty()
            || gap.is_some()
            || refs_changed;
        // A `HEAD` reflog that grew alone is often a `git` half-way: it
        // appends to the reflog before it renames the ref. The window is
        // extended once, from the same first event, so the commit and its
        // reflog come in one batch (and the S3 sample of that first event
        // still covers it); if it is still alone, it is sent.
        if heads_changed && !others && !self.heads_deferred {
            self.heads_deferred = true;
            let config = self.shared.config;
            let len = config.window.saturating_sub(config.timer_slack);
            self.window = Some((Instant::now() + len, marks.t_recv, overflow_from.is_some()));
            return;
        }
        self.heads_deferred = false;
        let send = others || heads_changed;
        let head_logs = new.head_logs();
        self.view = new;
        // Their tasks stop before the batch that says they are gone, so no
        // read of theirs can follow it and bring them back.
        for root in &gone {
            self.shared.stop_worktree(&self.repo_id, root);
        }
        if send {
            self.shared.send(ObservedBatch {
                repo_id: self.repo_id.clone(),
                worktrees: created.iter().map(|(r, _)| r.clone()).collect(),
                gone: gone.clone(),
                events,
                gap,
                refs: Some(tips.join("\n")),
                head_logs,
                heads: Vec::new(),
                marks,
            });
        }
        for wt in self.shared.worktrees_of(&self.repo_id) {
            if moved.contains(&wt.root) {
                let _ = wt.tx.send(WtMsg::Reconcile(marks.t_recv));
            }
        }
        for (read, admin) in created {
            if !matches!(
                read.view.status,
                gitraptor_api::messages::WorktreeStatus::Ready { .. }
            ) {
                continue;
            }
            let git_dir = match admin {
                Some(id) => self.common.join("worktrees").join(id),
                None => self.common.clone(),
            };
            let _ = self.shared.start_worktree(&self.repo_id, read, git_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflog_messages_name_the_event() {
        assert_eq!(kind_of("commit: login"), GitEventKind::Commit);
        assert_eq!(kind_of("commit (amend): login"), GitEventKind::Commit);
        assert_eq!(kind_of("commit (initial): x"), GitEventKind::Commit);
        assert_eq!(
            kind_of("commit (merge): Merge branch 'a'"),
            GitEventKind::Merge
        );
        assert_eq!(kind_of("merge feat: Fast-forward"), GitEventKind::Merge);
        assert_eq!(
            kind_of("rebase (finish): refs/heads/feat onto abc"),
            GitEventKind::Rebase
        );
        assert_eq!(
            kind_of("reset: moving to HEAD~1"),
            GitEventKind::BranchUpdate
        );
    }

    #[test]
    fn checkout_messages_name_both_branches() {
        let m = "checkout: moving from old-feature to feat-login";
        assert!(names_branch(m, "old-feature"));
        assert!(names_branch(m, "feat-login"));
        assert!(!names_branch(m, "feat"));
        assert!(!names_branch("commit: x", "x"));
    }

    fn wt(main: bool, branch: Option<&str>, commit: &str, reflog: Option<&str>) -> WorktreeRefs {
        WorktreeRefs {
            main,
            admin: None,
            branch: branch.map(str::to_owned),
            commit: Some(commit.to_owned()),
            head_reflog: reflog.map(str::to_owned),
            operating_on: None,
            head_log_len: 0,
            fingerprint: String::new(),
        }
    }

    /// D6: checked out, then named by the last `HEAD` move, then the only
    /// one at the commit (inferred), then the main worktree (inferred).
    #[test]
    fn events_are_placed_by_the_d6_rules() {
        let mut view = RefsView::default();
        view.worktrees
            .insert("/w/main".into(), wt(true, Some("main"), "c1", None));
        view.worktrees.insert(
            "/w/feat".into(),
            wt(
                false,
                Some("feat"),
                "c2",
                Some("checkout: moving from tmp to feat"),
            ),
        );
        view.worktrees
            .insert("/w/other".into(), wt(false, Some("other"), "c3", None));
        let at = |b: &str, c: Option<&str>| view.place(b, c).unwrap();
        assert_eq!(
            at("feat", None),
            EventPlace {
                worktree: "/w/feat".into(),
                inferred: false
            }
        );
        assert_eq!(
            at("tmp", Some("zz")),
            EventPlace {
                worktree: "/w/feat".into(),
                inferred: false
            }
        );
        assert_eq!(
            at("x", Some("c3")),
            EventPlace {
                worktree: "/w/other".into(),
                inferred: true
            }
        );
        assert_eq!(
            at("x", Some("c9")),
            EventPlace {
                worktree: "/w/main".into(),
                inferred: true
            }
        );
    }

    /// A worktree in the middle of a rebase of `feat` (detached, no
    /// `checkout` in its reflog) is where `feat`'s events happen.
    #[test]
    fn an_operation_in_progress_places_the_events_of_its_branch() {
        let mut view = RefsView::default();
        view.worktrees
            .insert("/w/main".into(), wt(true, Some("main"), "c1", None));
        let mut rebasing = wt(false, None, "c2", Some("rebase (pick): a"));
        rebasing.operating_on = Some("feat".into());
        view.worktrees.insert("/w/feat".into(), rebasing);
        assert_eq!(
            view.place("feat", Some("c2")),
            Some(EventPlace {
                worktree: "/w/feat".into(),
                inferred: false
            })
        );
    }
}
