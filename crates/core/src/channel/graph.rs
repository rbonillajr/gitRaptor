//! `graph.lanes`: resolves the confirmed base and the worktrees in the engine's state, queues the
//! walk of each lane in the query pool and answers, out of order, to the connection that asked.
//!
//! The answer carries commit subjects: it goes only to that connection's outbox. It is never
//! logged, audited, published on the bus or written to the profile. The log line of a query holds
//! the repo id, the duration, the number of lanes and the cut, nothing else.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::UntrustedName;
use gitraptor_api::graph::{
    BaseGap, GRAPH_DEADLINE_MS, GraphBase, GraphCut, GraphLanes, GraphParams, Lane, LaneCommit,
    LaneGap, LaneHead, LaneState, MAX_GRAPH_LANES, MAX_GRAPH_RESPONSE_BYTES, MAX_LANE_COMMITS,
    MAX_LANE_SUMMARY_BYTES,
};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, CommitCountView, EngineStateView, HeadView, RepoStateView,
    WorktreeStatus,
};
use gitraptor_api::rpc::{ErrorObject, Request, code};
use gitraptor_git::lane::{GitLane, GitLaneGap, LaneLimits};
use gitraptor_git::{Count, ReadError, ReaderOptions, RefName, RepoReader};

use super::ServerCtx;
use super::bus::Outbox;
use super::query::QueryTable;
use crate::daemon::{Field, now_ms};

/// Slack under [`MAX_GRAPH_RESPONSE_BYTES`] for the envelope of the response.
const ENVELOPE_MARGIN: usize = 256;
/// Commits visited to count the total of a lane before saying "at least".
const MAX_WALK: u64 = gitraptor_api::messages::MAX_DIVERGENCE_WALK;

/// The lanes already walked, per repo: the walk of a lane is skipped while its `HEAD` and the tip
/// of the base are the ones it was walked with. Bounded by the limits of a response (32 lanes of
/// 50 commits) because it is pruned to the lanes of the last response. Lives in the channel and
/// is never persisted.
#[derive(Debug, Default)]
pub(crate) struct LaneCache {
    repos: Mutex<HashMap<String, RepoLanes>>,
}

#[derive(Debug)]
struct RepoLanes {
    base_tip: String,
    /// Root of the worktree to the `HEAD` it was walked at and what the walk gave.
    lanes: HashMap<String, (String, GitLane)>,
}

impl LaneCache {
    /// The lane of `root` for `(head, base_tip)`: the cached one when both match, else what
    /// `walk` returns, which is stored if it is a lane. A gap is returned and never stored, so
    /// the next refresh tries again. `walk` runs without the lock held.
    pub(crate) fn lane(
        &self,
        repo_id: &str,
        root: &str,
        head: &str,
        base_tip: &str,
        walk: impl FnOnce() -> Result<GitLane, GitLaneGap>,
    ) -> Result<GitLane, GitLaneGap> {
        {
            let repos = self.repos.lock().unwrap_or_else(PoisonError::into_inner);
            let hit = repos
                .get(repo_id)
                .filter(|r| r.base_tip == base_tip)
                .and_then(|r| r.lanes.get(root))
                .filter(|(h, _)| h == head);
            if let Some((_, lane)) = hit {
                return Ok(lane.clone());
            }
        }
        let lane = walk()?;
        let mut repos = self.repos.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = repos
            .entry(repo_id.to_owned())
            .or_insert_with(|| RepoLanes {
                base_tip: base_tip.to_owned(),
                lanes: HashMap::new(),
            });
        if entry.base_tip != base_tip {
            entry.base_tip = base_tip.to_owned();
            entry.lanes.clear();
        }
        entry
            .lanes
            .insert(root.to_owned(), (head.to_owned(), lane.clone()));
        Ok(lane)
    }

    /// Keeps only the lanes of `roots` for the repo: the ones of the last response.
    pub(crate) fn prune(&self, repo_id: &str, roots: &[&str]) {
        let mut repos = self.repos.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = repos.get_mut(repo_id) {
            entry.lanes.retain(|root, _| roots.contains(&root.as_str()));
        }
    }
}

fn invalid() -> ErrorObject {
    ErrorObject::new(code::INVALID_PARAMS, "invalid params")
}

fn not_found() -> ErrorObject {
    ErrorObject::new(code::NOT_FOUND, "not found")
}

/// The base as the engine published it, before any Git is read.
enum BaseState {
    Gap(BaseGap),
    Named { name: String, tip: RefName },
}

/// One worktree that will be a lane: owned data only, so the job outlives the connection's reader.
struct LaneRef {
    root: PathBuf,
    root_text: String,
    main: bool,
    admin_name: Option<String>,
    head: LaneHead,
    /// The engine could not read it when it last looked.
    unavailable: bool,
}

/// What the arm of `dispatch` resolved.
struct Resolved {
    repo_id: String,
    common_dir: PathBuf,
    base: BaseState,
    lanes: Vec<LaneRef>,
    /// Lanes of the repo, also the ones that did not fit in `lanes`.
    lanes_total: u32,
    cut: Option<GraphCut>,
}

fn resolve(ctx: &ServerCtx, request: &Request) -> Result<Resolved, ErrorObject> {
    let params: GraphParams = request.params()?;
    if !super::diff::valid_repo_id(&params.repo_id) {
        return Err(invalid());
    }
    let unavailable = |repo_id: &str, common_dir: PathBuf| Resolved {
        repo_id: repo_id.to_owned(),
        common_dir,
        base: BaseState::Gap(BaseGap::RepoUnavailable),
        lanes: Vec::new(),
        lanes_total: 0,
        cut: None,
    };
    let (_, shared) = ctx.bus.snapshot();
    let Some(repo) = shared.repos.iter().find(|r| r.repo_id == params.repo_id) else {
        // The engine has not observed anything yet: the repo may be about to appear.
        return if shared.engine.state == EngineStateView::Observing {
            Err(not_found())
        } else {
            Ok(unavailable(&params.repo_id, PathBuf::new()))
        };
    };
    let common_dir = PathBuf::from(repo.path.raw());
    if repo.state != RepoStateView::Observed || repo.worktrees.is_empty() {
        return Ok(unavailable(&repo.repo_id, common_dir));
    }
    let BaseBranchView { name, status } = &repo.base;
    let base = match name.as_ref().map(UntrustedName::raw) {
        None => BaseState::Gap(BaseGap::NoBase),
        Some(n) if *status != BaseStatusView::Confirmed => {
            BaseState::Gap(BaseGap::BaseUnconfirmed {
                name: UntrustedName::new(n),
            })
        }
        Some(n) => match RefName::new(&format!("refs/heads/{n}")) {
            Ok(tip) => BaseState::Named {
                name: n.to_owned(),
                tip,
            },
            Err(_) => BaseState::Gap(BaseGap::NoBase),
        },
    };
    let base_name = match &base {
        BaseState::Named { name, .. } => Some(name.as_str()),
        BaseState::Gap(_) => None,
    };
    let mut lanes: Vec<LaneRef> = repo
        .worktrees
        .iter()
        .filter(|w| {
            // The worktree with the base checked out is the trunk, not a lane.
            !matches!(
                &w.status,
                WorktreeStatus::Ready { head: HeadView::Branch { name }, .. }
                    if Some(name.raw()) == base_name
            )
        })
        .map(|w| LaneRef {
            root: PathBuf::from(w.path.raw()),
            root_text: w.path.raw().to_owned(),
            main: w.main,
            admin_name: w.admin_name.as_ref().map(|n| n.raw().to_owned()),
            head: match &w.status {
                WorktreeStatus::Ready {
                    head: HeadView::Branch { name } | HeadView::Unborn { name },
                    ..
                } => LaneHead::Branch { name: name.clone() },
                _ => LaneHead::Detached,
            },
            unavailable: matches!(w.status, WorktreeStatus::Unavailable { .. }),
        })
        .collect();
    let lanes_total = u32::try_from(lanes.len()).unwrap_or(u32::MAX);
    let mut cut = None;
    if lanes.len() > MAX_GRAPH_LANES as usize {
        lanes.truncate(MAX_GRAPH_LANES as usize);
        cut = Some(GraphCut::Lanes);
    }
    Ok(Resolved {
        repo_id: repo.repo_id.clone(),
        common_dir,
        base,
        lanes,
        lanes_total,
        cut,
    })
}

/// Serves `graph.lanes`: `Err` is the answer, at once; `Ok` means the query is queued and its
/// answer will come from the pool.
pub(super) fn start(
    ctx: &ServerCtx,
    queries: &QueryTable,
    outbox: &Arc<Outbox>,
    request: &Request,
) -> Result<(), ErrorObject> {
    let resolved = resolve(ctx, request)?;
    let token = queries.register(&request.id).map_err(|busy| busy.error())?;
    let (table, outbox, id) = (queries.clone(), Arc::clone(outbox), request.id.clone());
    let logger = ctx.logger.clone();
    let job_token = token.clone();
    let deadline = Duration::from_millis(GRAPH_DEADLINE_MS);
    #[cfg(debug_assertions)]
    let deadline = ctx.config.test_diff_deadline.unwrap_or(deadline);
    let cache = Arc::clone(&ctx.graph_cache);
    let job = Box::new(move || {
        let started = Instant::now();
        table.run(&outbox, id.clone(), &job_token, |token| {
            let graph = compute(&resolved, &cache, deadline, &|| token.is_cancelled())?;
            logger.info(
                "graph_lanes",
                &[
                    ("repo", Field::Id(resolved.repo_id.clone())),
                    (
                        "ms",
                        Field::Int(
                            i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
                        ),
                    ),
                    ("lanes", Field::Int(i64::from(graph.lanes_total))),
                    (
                        "cut",
                        Field::Text(match graph.cut {
                            None => "none",
                            Some(GraphCut::Lanes) => "lanes",
                            Some(GraphCut::Bytes) => "bytes",
                        }),
                    ),
                ],
            );
            Ok(graph)
        });
    });
    if let Err(busy) = ctx.query.submit(job) {
        queries.finish(&request.id);
        return Err(busy.error());
    }
    Ok(())
}

fn failed() -> ErrorObject {
    ErrorObject::new(code::INTERNAL, "graph failed")
}

fn compute(
    resolved: &Resolved,
    cache: &LaneCache,
    deadline: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<GraphLanes, ErrorObject> {
    let limit = Instant::now() + deadline;
    let empty = |reason: BaseGap| GraphLanes {
        computed_at_utc_ms: now_ms(),
        base: GraphBase::NotComputable { reason },
        lanes: Vec::new(),
        lanes_total: 0,
        cut: None,
    };
    let (name, tip) = match &resolved.base {
        BaseState::Gap(gap) => return Ok(empty(gap.clone())),
        BaseState::Named { name, tip } => (name, tip),
    };
    let common =
        RepoReader::open(&resolved.common_dir, &ReaderOptions::default()).map_err(|_| failed())?;
    let Some(base_tip) = common.resolve_ref(tip).map_err(|_| failed())? else {
        return Ok(empty(BaseGap::BaseMissing {
            name: UntrustedName::new(name.as_str()),
        }));
    };
    let stop = || cancelled() || Instant::now() >= limit;
    let lanes: Vec<Lane> = resolved
        .lanes
        .iter()
        .map(|lane| Lane {
            worktree: Untrusted::new(lane.root_text.clone()),
            head: lane.head.clone(),
            state: lane_state(resolved, lane, &base_tip, cache, &stop),
        })
        .collect();
    let roots: Vec<&str> = resolved
        .lanes
        .iter()
        .map(|l| l.root_text.as_str())
        .collect();
    cache.prune(&resolved.repo_id, &roots);
    let mut graph = GraphLanes {
        computed_at_utc_ms: now_ms(),
        base: GraphBase::Ready {
            name: UntrustedName::new(name.as_str()),
        },
        lanes,
        lanes_total: resolved.lanes_total,
        cut: resolved.cut,
    };
    fit(&mut graph, MAX_GRAPH_RESPONSE_BYTES - ENVELOPE_MARGIN);
    Ok(graph)
}

fn lane_state(
    resolved: &Resolved,
    lane: &LaneRef,
    base_tip: &str,
    cache: &LaneCache,
    stop: &dyn Fn() -> bool,
) -> LaneState {
    let gap = |reason| LaneState::NotComputable { reason };
    if lane.unavailable {
        return gap(LaneGap::WorktreeUnavailable);
    }
    if stop() {
        return gap(LaneGap::TooSlow);
    }
    let reader = match crate::observe::open_worktree(
        &resolved.common_dir,
        &lane.root,
        lane.main,
        lane.admin_name.as_deref(),
    ) {
        Ok(reader) => reader,
        Err(ReadError::Unavailable(_) | ReadError::Untrusted(_) | ReadError::NotARepository(_)) => {
            return gap(LaneGap::WorktreeUnavailable);
        }
        Err(_) => return gap(LaneGap::Unreadable),
    };
    let head = match reader.head() {
        Ok(head) => head,
        Err(_) => return gap(LaneGap::Unreadable),
    };
    let Some(head) = head.commit.filter(|_| !head.unborn) else {
        return gap(LaneGap::NoCommits);
    };
    let limits = LaneLimits {
        max_commits: MAX_LANE_COMMITS,
        max_walk: MAX_WALK,
        max_summary_bytes: MAX_LANE_SUMMARY_BYTES,
    };
    let read_error = RefCell::new(false);
    let walked = cache.lane(
        &resolved.repo_id,
        &lane.root_text,
        &head,
        base_tip,
        || match reader.lane(&head, base_tip, &limits, stop) {
            Ok(walked) => walked,
            Err(_) => {
                *read_error.borrow_mut() = true;
                Err(GitLaneGap::TooSlow)
            }
        },
    );
    if *read_error.borrow() {
        return gap(LaneGap::Unreadable);
    }
    match walked {
        Ok(lane) => ready(lane),
        Err(GitLaneGap::NoMergeBase) => gap(LaneGap::NoMergeBase),
        Err(GitLaneGap::TooSlow) => gap(LaneGap::TooSlow),
    }
}

fn ready(lane: GitLane) -> LaneState {
    LaneState::Ready {
        merge_base: lane.merge_base,
        commits: lane
            .commits
            .into_iter()
            .map(|c| LaneCommit {
                oid: c.id,
                summary: Untrusted::new(c.summary),
                committed_utc_ms: c.committed_s.saturating_mul(1000),
                merge: c.merge,
            })
            .collect(),
        total: match lane.total {
            Count::Exact(count) => CommitCountView { count, exact: true },
            Count::AtLeast(count) => CommitCountView {
                count,
                exact: false,
            },
        },
    }
}

/// Cuts `graph` so its serialization is at most `limit` bytes: commits leave from the end of the
/// longest lane, one at a time, and the response says `bytes`. Totals are never touched.
fn fit(graph: &mut GraphLanes, limit: usize) {
    let Ok(whole) = serde_json::to_vec(graph) else {
        return;
    };
    if whole.len() <= limit {
        return;
    }
    let mut excess = whole.len() - limit;
    graph.cut = Some(GraphCut::Bytes);
    while excess > 0 {
        let longest = graph
            .lanes
            .iter_mut()
            .filter_map(|l| match &mut l.state {
                LaneState::Ready { commits, .. } if !commits.is_empty() => Some(commits),
                _ => None,
            })
            .max_by_key(|c| c.len());
        let Some(commits) = longest else {
            return;
        };
        let Some(last) = commits.pop() else {
            return;
        };
        // The commit and the comma that separated it.
        excess = excess.saturating_sub(serde_json::to_vec(&last).map_or(1, |b| b.len() + 1));
    }
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
