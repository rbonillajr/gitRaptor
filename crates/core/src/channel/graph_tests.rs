//! US-CKP-022 D9, D17: the per-lane cache of `graph.lanes` (`LaneCache`, in `channel/graph.rs`).
//! The expert mounts this file with `#[cfg(test)] #[path = "graph_tests.rs"] mod tests;` in
//! `graph.rs`.
//!
//! API assumed (documented in the hand-back; the DS fixes the shape of the cache, not its
//! methods), all `pub(crate)` on `LaneCache`:
//!
//! ```ignore
//! impl LaneCache {
//!     /// The lane of `root` for `(head, base_tip)`: the cached one when the key is the same,
//!     /// else `walk()` (the walk of `RepoReader::lane`), whose `Ok` result is stored.
//!     /// An `Err(GitLaneGap)` is returned and not stored.
//!     fn lane(&self, repo_id: &str, root: &str, head: &str, base_tip: &str,
//!             walk: impl FnOnce() -> Result<GitLane, GitLaneGap>) -> Result<GitLane, GitLaneGap>;
//!     /// Keeps only the lanes of `roots` for the repo (the lanes of the last response).
//!     fn prune(&self, repo_id: &str, roots: &[&str]);
//! }
//! ```
//!
//! The "counter of walks" is the closure of each call: a hit never calls it. `GitLane` is
//! `Clone`.

use std::cell::Cell;

use gitraptor_git::Count;
use gitraptor_git::lane::{GitLane, GitLaneCommit, GitLaneGap};

use super::LaneCache;

fn lane_of(tag: &str) -> GitLane {
    GitLane {
        merge_base: format!("base-{tag}"),
        commits: vec![GitLaneCommit {
            id: format!("commit-{tag}"),
            summary: format!("subject {tag}"),
            committed_s: 1,
            merge: false,
        }],
        total: Count::Exact(1),
    }
}

/// Asks for a lane and counts the walks it needed. Returns the lane and whether it walked.
fn ask(
    cache: &LaneCache,
    repo: &str,
    root: &str,
    head: &str,
    base: &str,
    tag: &str,
) -> (GitLane, bool) {
    let walked = Cell::new(false);
    let lane = cache
        .lane(repo, root, head, base, || {
            walked.set(true);
            Ok(lane_of(tag))
        })
        .unwrap_or_else(|_| panic!("the walk was not a gap"));
    (lane, walked.get())
}

#[test]
fn second_refresh_hits_the_lane_cache() {
    let cache = LaneCache::default();

    // First refresh: nothing cached, the lane is walked.
    let (first, walked) = ask(&cache, "r1", "/w/feat", "head-1", "base-1", "first");
    assert!(walked, "an empty cache must walk the lane");
    assert_eq!(first.merge_base, "base-first");

    // Second refresh, same HEAD and same base: a hit that walks nothing and returns the same lane.
    let (second, walked) = ask(&cache, "r1", "/w/feat", "head-1", "base-1", "second");
    assert!(
        !walked,
        "same HEAD and same base tip: the lane must come from the cache, not from a new walk (D9)"
    );
    assert_eq!(
        second.merge_base, "base-first",
        "a hit returns what the first walk stored"
    );
    assert_eq!(second.commits[0].id, "commit-first");

    // Another lane of the same repo is its own entry.
    let (_, walked) = ask(&cache, "r1", "/w/login", "head-9", "base-1", "login");
    assert!(walked, "a lane never seen is walked");
    let (_, walked) = ask(&cache, "r1", "/w/feat", "head-1", "base-1", "again");
    assert!(!walked, "walking another lane does not evict this one");

    // A new HEAD (a commit or an --amend) invalidates that lane only.
    let (moved, walked) = ask(&cache, "r1", "/w/feat", "head-2", "base-1", "moved");
    assert!(walked, "a different HEAD must walk again");
    assert_eq!(moved.merge_base, "base-moved");
    let (_, walked) = ask(&cache, "r1", "/w/login", "head-9", "base-1", "login2");
    assert!(!walked, "the lane whose HEAD did not change stays cached");

    // A new base tip empties the repo: every lane is walked again.
    let (_, walked) = ask(&cache, "r1", "/w/login", "head-9", "base-2", "login3");
    assert!(
        walked,
        "a different base tip invalidates the lanes of the repo"
    );
    let (_, walked) = ask(&cache, "r1", "/w/feat", "head-2", "base-2", "feat3");
    assert!(
        walked,
        "the base moved: the lane that was cached for the old base is gone too"
    );

    // Another repo has its own cache.
    let (_, walked) = ask(&cache, "r2", "/w/feat", "head-2", "base-2", "other-repo");
    assert!(walked, "the cache is per repo");
}

#[test]
fn a_gap_is_not_cached_and_pruning_keeps_only_the_last_response() {
    let cache = LaneCache::default();

    // A walk that ended as a gap (too slow) is not stored: the next refresh tries again.
    let gap = cache.lane("r1", "/w/feat", "h", "b", || Err(GitLaneGap::TooSlow));
    assert!(matches!(gap, Err(GitLaneGap::TooSlow)));
    let (_, walked) = ask(&cache, "r1", "/w/feat", "h", "b", "retry");
    assert!(walked, "a too-slow lane must not be served from the cache");

    // Pruning to the lanes of the last response bounds the cache (D12: 560 KiB per repo).
    let (_, _) = ask(&cache, "r1", "/w/gone", "h", "b", "gone");
    cache.prune("r1", &["/w/feat"]);
    let (_, walked) = ask(&cache, "r1", "/w/feat", "h", "b", "kept");
    assert!(!walked, "a lane of the last response survives the prune");
    let (_, walked) = ask(&cache, "r1", "/w/gone", "h", "b", "gone-again");
    assert!(
        walked,
        "a lane that was not in the last response is dropped"
    );
}
