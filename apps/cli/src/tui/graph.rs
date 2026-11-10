//! The graph view's state (US-CKP-022): opened with `g`, asked from the engine and kept up to
//! date while it is open (D9), closed with `Esc` or `g`. Pure: the asking and the answer travel
//! as [`Cmd`] and [`EngineMsg::Graph`]; the only clock is the instant `tick` is given.

use std::time::{Duration, Instant};

use gitraptor_api::graph::GraphLanes;

use crate::model::{Cmd, GraphFailure, Model, Notice, WorktreeState};
pub use crate::present::graph::Shown;
use crate::present::graph::{fleet_keys, ingest};

/// The least time between two requests of an open view.
pub const REFRESH_MIN: Duration = Duration::from_secs(1);

/// Where the open view starts painting, by what is in it and not by a line number: new commits
/// on top of a lane do not move what the developer is reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    Top,
    /// The first visible row is this commit of the lane with this key.
    Commit {
        lane: u64,
        oid: String,
    },
    /// The first visible row is the header of the lane with this key.
    Lane(u64),
}

/// What the full-screen view holds while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphState {
    /// The ticket of the request in flight, if any.
    pub in_flight: Option<u64>,
    pub last_request: Option<Instant>,
    /// How long the last answer took: the refresh waits twice that (D9).
    pub last_duration: Duration,
    /// Something changed since the last request.
    pub stale: bool,
    /// The last answer, as sanitized text.
    pub shown: Option<Shown>,
    /// Why the last request did not arrive; the last answer stays.
    pub failure: Option<GraphFailure>,
    pub anchor: Anchor,
}

fn repo_id(model: &Model) -> Option<String> {
    model
        .engine
        .repo
        .as_ref()
        .and_then(|r| r.data.as_ref())
        .map(|r| r.repo_id.clone())
}

fn next_ticket(model: &mut Model) -> u64 {
    let ticket = model.ui.next_ticket;
    model.ui.next_ticket += 1;
    ticket
}

/// `g`: opens the view and asks for the graph of the repo on screen, or says why it does not.
/// With the diff open, nothing happens (D20).
pub fn open(model: &mut Model) -> Vec<Cmd> {
    if model.ui.diff.is_some() {
        return Vec::new();
    }
    let Some(repo_id) = repo_id(model) else {
        model.ui.notice = Some(Notice::UnknownKey);
        return Vec::new();
    };
    if !model.engine.graph {
        model.ui.notice = Some(Notice::GraphUnsupported);
        return Vec::new();
    }
    let mut cmds = close(model);
    let ticket = next_ticket(model);
    model.ui.graph = Some(GraphState {
        in_flight: Some(ticket),
        last_request: Some(Instant::now()),
        last_duration: Duration::ZERO,
        stale: false,
        shown: None,
        failure: None,
        anchor: Anchor::Top,
    });
    cmds.push(Cmd::Graph { ticket, repo_id });
    cmds
}

/// Closes the view; a query still running is cancelled.
pub fn close(model: &mut Model) -> Vec<Cmd> {
    match model.ui.graph.take().and_then(|s| s.in_flight) {
        Some(ticket) => vec![Cmd::CancelQuery { ticket }],
        None => Vec::new(),
    }
}

/// What the graph depends on in the stream: the base, and each worktree's branch and
/// divergence. Counting files changed or sessions is no reason to ask again.
pub fn signature(model: &Model) -> Vec<String> {
    let Some(repo) = model.engine.repo.as_ref().and_then(|r| r.data.as_ref()) else {
        return Vec::new();
    };
    let mut parts = vec![format!("{:?}/{}", repo.base, repo.base_confirmed)];
    parts.extend(repo.worktrees.iter().map(|w| {
        let state = match &w.state {
            WorktreeState::Ready {
                head, divergence, ..
            } => format!("{head:?}/{divergence:?}"),
            WorktreeState::Unavailable(reason) => format!("{reason:?}"),
        };
        format!("{}/{state}", w.key)
    }));
    parts
}

/// Something the open view shows may have changed: the next `tick` asks again.
pub fn mark_stale(model: &mut Model) {
    if let Some(state) = &mut model.ui.graph {
        state.stale = true;
    }
}

/// Asks again when the view is stale, nothing is in flight and enough time has passed since the
/// last request: `max(REFRESH_MIN, 2 × last_duration)`, so the view never takes more than about
/// half of the engine's query pool (D9).
pub fn tick(model: &mut Model, now: Instant) -> Vec<Cmd> {
    let Some(repo_id) = repo_id(model) else {
        return Vec::new();
    };
    let Some(state) = &model.ui.graph else {
        return Vec::new();
    };
    if !state.stale || state.in_flight.is_some() {
        return Vec::new();
    }
    let wait = REFRESH_MIN.max(state.last_duration.saturating_mul(2));
    if state
        .last_request
        .is_some_and(|last| now.saturating_duration_since(last) < wait)
    {
        return Vec::new();
    }
    let ticket = next_ticket(model);
    if let Some(state) = &mut model.ui.graph {
        state.in_flight = Some(ticket);
        state.last_request = Some(now);
        state.stale = false;
    }
    model.dirty = true;
    vec![Cmd::Graph { ticket, repo_id }]
}

/// The answer of ticket `ticket`. One that is not the request in flight (an older one, or one
/// for a view that was closed) is dropped.
pub fn apply_reply(model: &mut Model, ticket: u64, result: Result<Box<GraphLanes>, GraphFailure>) {
    let keys = fleet_keys(model);
    let Some(state) = &mut model.ui.graph else {
        return;
    };
    if state.in_flight != Some(ticket) {
        return;
    }
    state.in_flight = None;
    state.last_duration = state.last_request.map_or(Duration::ZERO, |at| {
        Instant::now().saturating_duration_since(at)
    });
    match result {
        Ok(lanes) => {
            let mut shown = ingest(&lanes);
            shown.order_by(&keys);
            state.anchor = reanchor(&state.anchor, &shown);
            state.shown = Some(shown);
            state.failure = None;
        }
        // Another query holds the pool, or a cancel that is not ours: try again later.
        Err(GraphFailure::Busy | GraphFailure::Cancelled) => state.stale = true,
        Err(failure @ GraphFailure::Lost) => {
            state.failure = Some(failure);
            state.stale = true;
        }
        Err(failure @ GraphFailure::Refused) => state.failure = Some(failure),
    }
    model.dirty = true;
}

/// The anchor in a new answer: the same commit if it is still there, else the header of its
/// lane, else the top.
fn reanchor(anchor: &Anchor, shown: &Shown) -> Anchor {
    let stops = shown.stops();
    match anchor {
        Anchor::Top => Anchor::Top,
        Anchor::Commit { .. } if stops.contains(anchor) => anchor.clone(),
        Anchor::Commit { lane, .. } | Anchor::Lane(lane) => {
            let header = Anchor::Lane(*lane);
            if stops.contains(&header) {
                header
            } else {
                Anchor::Top
            }
        }
    }
}

/// Scrolls the open view to the previous or the next stop (a lane header or a commit).
pub fn scroll(model: &mut Model, down: bool) {
    let Some(state) = &mut model.ui.graph else {
        return;
    };
    let Some(shown) = &state.shown else {
        return;
    };
    let stops = shown.stops();
    let at = match &state.anchor {
        Anchor::Top => 0,
        anchor => stops.iter().position(|s| s == anchor).unwrap_or(0),
    };
    let to = if down {
        (at + 1).min(stops.len().saturating_sub(1))
    } else {
        at.saturating_sub(1)
    };
    state.anchor = match stops.get(to) {
        Some(_) if to == 0 => Anchor::Top,
        Some(stop) => stop.clone(),
        None => Anchor::Top,
    };
    model.dirty = true;
}
