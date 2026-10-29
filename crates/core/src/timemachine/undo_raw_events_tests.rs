//! `raw_of_all` reports whether it holds the events of every registered worktree: without an
//! engine, when the worktrees cannot be listed or when the events of one cannot be read, it says
//! so (the overlap check then fails closed for an unattributed reference), never a silent skip.
//! Temporary repos and profile only. Child module of `timemachine::undo`.

use std::sync::Arc;

use super::*;
use crate::daemon::{LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::engine::EngineLink;
use crate::timemachine::protected::TmRepos;
use gitraptor_testkit::Fixture;

const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

/// An engine that reads the events of every worktree except `blind_to`.
struct Engine {
    blind_to: Option<PathBuf>,
}

impl EngineLink for Engine {
    fn mark(&self, _repo_id: &str) -> i64 {
        1
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
        Some(1)
    }
    fn raw_events(&self, _repo_id: &str, worktree: &Path) -> Option<Vec<RawGitEvent>> {
        if self.blind_to.as_deref() == Some(worktree) {
            None
        } else {
            Some(Vec::new())
        }
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, _repo_id: &str) -> u64 {
        0
    }
}

struct Rig {
    f: Fixture,
    dirs: ProfileDirs,
    common: PathBuf,
    linked: PathBuf,
}

fn rig() -> Rig {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    f.git(&["branch", "linked-branch"]);
    let linked = f.add_worktree("linked", "linked-branch");
    let dirs = ProfileDirs::under_root(&f.profile);
    let common = crate::observe::locate(&f.repo).unwrap();
    let linked = registered_worktrees(&common)
        .unwrap()
        .into_iter()
        .map(|(root, _)| root)
        .find(|root| root.ends_with(linked.file_name().unwrap()))
        .expect("the linked worktree is registered");
    Rig {
        f,
        dirs,
        common,
        linked,
    }
}

impl Rig {
    fn deps(&self, blind_to: Option<PathBuf>) -> CaptureDeps {
        CaptureDeps {
            repos: Arc::new(TmRepos::new(self.dirs.clone())),
            engine: Arc::new(Engine { blind_to }),
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer: None,
            free_space_floor: None,
            after_mark: None,
        }
    }

    fn own(&self) -> PathBuf {
        registered_worktrees(&self.common)
            .unwrap()
            .into_iter()
            .map(|(root, _)| root)
            .find(|root| *root != self.linked)
            .expect("the main worktree is registered")
    }
}

#[test]
fn every_worktree_read_is_complete() {
    let r = rig();
    let deps = r.deps(None);
    let (raw, complete) = raw_of_all(Some(&deps), REPO, &r.common, &r.own());
    assert!(complete);
    assert!(raw.contains_key(&r.own()) && raw.contains_key(&r.linked));
}

#[test]
fn without_an_engine_the_events_are_incomplete() {
    let r = rig();
    let (_, complete) = raw_of_all(None, REPO, &r.common, &r.own());
    assert!(!complete, "no engine: no event of any worktree is known");
}

#[test]
fn a_registered_worktree_without_events_makes_them_incomplete() {
    let r = rig();
    let deps = r.deps(Some(r.linked.clone()));
    let (raw, complete) = raw_of_all(Some(&deps), REPO, &r.common, &r.own());
    assert!(
        !complete,
        "the events of a registered worktree could not be read: never skipped silently"
    );
    assert!(
        raw.contains_key(&r.linked),
        "it is still listed, with no events"
    );
}

#[test]
fn worktrees_that_cannot_be_listed_make_the_events_incomplete() {
    let r = rig();
    let deps = r.deps(None);
    let missing = r.f.profile.join("no-such-common-dir");
    let (_, complete) = raw_of_all(Some(&deps), REPO, &missing, &r.own());
    assert!(!complete, "the registered worktrees are unknown");
}
