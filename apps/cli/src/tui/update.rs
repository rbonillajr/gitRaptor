//! `update(&mut Model, Msg) -> Vec<Cmd>` (ADR-CKP-003 § 2): pure, no I/O,
//! no clock. The engine replica only changes through snapshots and events
//! of the engine; a gap or a `resync` stops applying and asks for a new
//! snapshot.

use gitraptor_api::discovery::RepoDiscoveredData;
use gitraptor_api::event::{
    ENGINE_STATE, GIT_EVENT, REPO_DISCOVERED, SESSION_STATE, WORKTREE_STATE,
};
use gitraptor_api::messages::{
    EngineView, GitEventView, ResyncReason, SessionView, SessionsListResult, WorktreeStateData,
};
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use ratatui::crossterm::event::KeyEventKind;

use crate::client::sequence::Verdict;
use gitraptor_api::catalog::Layer;

use crate::model::{
    Candidate, Cmd, ConnEvent, ConnState, EngineMsg, Found, Model, Msg, Notice, ObserveFailure,
    Pick, Requester, ScopeReplica,
};
use crate::present::{SafeText, ingest};
use crate::tui::keymap::{self, Action};

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Key(key) if key.kind == KeyEventKind::Release => Vec::new(),
        Msg::Key(key) => on_action(model, keymap::action(&key)),
        // Paste has no target yet: it still answers.
        Msg::Paste(_) => on_action(model, None),
        Msg::Resize(size) => {
            model.ui.size = size;
            model.dirty = true;
            Vec::new()
        }
        Msg::Engine(stamped) => on_engine(model, stamped.msg),
        Msg::Conn(event) => on_conn(model, event),
        // The ages of the fleet move with the clock.
        Msg::Tick { now_ms } => {
            model.now_ms = now_ms;
            if model.engine.activity && model.engine.repo.as_ref().is_some_and(|r| r.data.is_some())
            {
                model.dirty = true;
            }
            Vec::new()
        }
    }
}

/// Every key changes something visible (feedback < 100 ms, § 3).
fn on_action(model: &mut Model, action: Option<Action>) -> Vec<Cmd> {
    model.dirty = true;
    if model.discovery_prompt().is_some() {
        match action {
            Some(Action::Yes) => return answer_discovered(model, true),
            Some(Action::No) => return answer_discovered(model, false),
            // Later: still pending in the engine, not asked again in this run.
            Some(Action::Later) => {
                model.ui.discovered.pop_front();
                return Vec::new();
            }
            _ => {}
        }
    }
    if model.ui.pick == Pick::Asking {
        match action {
            Some(Action::Yes) => return observe(model),
            // Enter takes the default, which is no.
            Some(Action::No | Action::Open | Action::Later) => {
                model.ui.asked = true;
                model.ui.here = None;
                return on_unlocated(model);
            }
            Some(Action::Up | Action::Down) => {
                model.ui.notice = Some(Notice::UnknownKey);
                return Vec::new();
            }
            _ => {}
        }
    }
    match action {
        Some(Action::Quit) => {
            model.ui.quit = true;
            vec![Cmd::Quit]
        }
        Some(Action::Retry) if model.conn == ConnState::Live => {
            model.ui.notice = Some(Notice::AlreadyLive);
            Vec::new()
        }
        Some(Action::Retry) if model.conn == ConnState::Starting => {
            model.ui.notice = Some(Notice::Starting);
            Vec::new()
        }
        Some(Action::Retry) => {
            model.ui.notice = Some(Notice::Retrying);
            vec![Cmd::Reconnect]
        }
        // Job control exists only on Unix; elsewhere the key says so and nothing else happens.
        Some(Action::Suspend) if cfg!(unix) => vec![Cmd::Suspend],
        Some(Action::Suspend) => {
            model.ui.notice = Some(Notice::SuspendUnsupported);
            Vec::new()
        }
        Some(action @ (Action::Up | Action::Down | Action::Open)) => on_pick(model, action),
        // No question on screen.
        Some(Action::Yes | Action::No | Action::Later) | None => {
            model.ui.notice = Some(Notice::UnknownKey);
            Vec::new()
        }
    }
}

/// The repo picker: ↑↓ move, Enter opens. Outside it these keys do nothing (yet).
fn on_pick(model: &mut Model, action: Action) -> Vec<Cmd> {
    let repos = model
        .engine
        .global
        .data
        .as_ref()
        .map(|g| g.repos.as_slice())
        .unwrap_or_default();
    let Pick::Choosing { selected } = model.ui.pick else {
        model.ui.notice = Some(Notice::UnknownKey);
        return Vec::new();
    };
    let last = repos.len().saturating_sub(1);
    match action {
        Action::Up => {
            model.ui.pick = Pick::Choosing {
                selected: selected.saturating_sub(1),
            }
        }
        Action::Down => {
            model.ui.pick = Pick::Choosing {
                selected: (selected + 1).min(last),
            }
        }
        _ => {
            if let Some(repo) = repos.get(selected.min(last)) {
                let repo_id = repo.repo_id.clone();
                model.ui.pick = Pick::Opening;
                return vec![Cmd::Open { repo_id }];
            }
        }
    }
    Vec::new()
}

/// The developer said yes: observe the repo of the folder (`repo.add`; the engine authorizes
/// it again) and show it when its snapshot arrives.
fn observe(model: &mut Model) -> Vec<Cmd> {
    model.ui.asked = true;
    let Some(here) = &model.ui.here else {
        return on_unlocated(model);
    };
    let root = here.root.clone();
    model.ui.pick = Pick::Observing;
    vec![Cmd::Observe { root }]
}

/// The folder is in a repo the engine does not observe (US-CKP-025): ask the developer once
/// per run, and never anyone else. Only a connection the engine resolved as the developer
/// (unattributed, Cockpit layer) is asked: an agent, an unverified caller or an unknown one
/// goes on as outside any repo. The engine authorizes `repo.add` again either way.
fn on_unobserved(model: &mut Model, candidate: Candidate) -> Vec<Cmd> {
    let developer = matches!(
        model.engine.requester,
        Some(Requester::Unattributed {
            layer: Layer::Cockpit
        })
    );
    if !developer || model.ui.asked {
        return on_unlocated(model);
    }
    // A reconnection while the question is on screen keeps it.
    if model.ui.pick != Pick::Asking {
        model.ui.pick = Pick::Asking;
        model.ui.here = Some(candidate);
    }
    Vec::new()
}

/// The developer answered the discovered repo on screen: `s` observes it (`repo.add`, without
/// leaving the fleet) and `n` dismisses it for good (`discovery.dismiss`).
fn answer_discovered(model: &mut Model, observe: bool) -> Vec<Cmd> {
    let Some(found) = model.ui.discovered.pop_front() else {
        return Vec::new();
    };
    let path = found.path;
    vec![if observe {
        Cmd::AcceptDiscovered { path }
    } else {
        Cmd::DismissDiscovered { path }
    }]
}

/// Queue discovered repos: once per path in a run, and never behind another being asked.
fn enqueue_discovered(model: &mut Model, found: impl IntoIterator<Item = Found>) {
    for repo in found {
        if model.ui.seen.insert(repo.path.clone()) {
            model.ui.discovered.push_back(repo);
        }
    }
}

/// `repo.add` failed: say what happened, why and how to retry, and go on as outside any
/// repo. Not asked again in this run.
fn on_observe_failed(model: &mut Model, failure: ObserveFailure) -> Vec<Cmd> {
    model.ui.asked = true;
    model.ui.notice = Some(Notice::ObserveFailed(failure));
    on_unlocated(model)
}

/// The folder is in no observed repo: the only one opens by itself; with several, the
/// developer chooses; with none, the fleet says how to add one.
fn on_unlocated(model: &mut Model) -> Vec<Cmd> {
    let repos = model
        .engine
        .global
        .data
        .as_ref()
        .map(|g| g.repos.as_slice())
        .unwrap_or_default();
    match repos {
        [] => {
            model.ui.pick = Pick::None;
            Vec::new()
        }
        [only] => {
            let repo_id = only.repo_id.clone();
            model.ui.pick = Pick::Opening;
            vec![Cmd::Open { repo_id }]
        }
        _ => {
            if !matches!(model.ui.pick, Pick::Choosing { .. }) {
                model.ui.pick = Pick::Choosing { selected: 0 };
            }
            Vec::new()
        }
    }
}

fn on_conn(model: &mut Model, event: ConnEvent) -> Vec<Cmd> {
    model.dirty = true;
    match event {
        ConnEvent::Unlocated => return on_unlocated(model),
        ConnEvent::Unobserved(candidate) => return on_unobserved(model, candidate),
        ConnEvent::ObserveFailed(failure) => return on_observe_failed(model, failure),
        ConnEvent::Discovered(found) => enqueue_discovered(model, found),
        ConnEvent::DiscoveryFailed(failure) => {
            model.ui.notice = Some(Notice::DiscoveryFailed(failure));
        }
        ConnEvent::Activity(activity) => model.engine.activity = activity,
        ConnEvent::Requester(requester) => model.engine.requester = requester,
        ConnEvent::State(state) => {
            if matches!(
                state,
                ConnState::Connecting
                    | ConnState::Starting
                    | ConnState::Reconnecting { .. }
                    | ConnState::Syncing
            ) {
                model.engine.mark_all_stale();
            }
            // A failed observation keeps saying why and how to retry until the next key.
            if state == ConnState::Live
                && !matches!(model.ui.notice, Some(Notice::ObserveFailed(_)))
            {
                model.ui.notice = None;
            }
            model.conn = state;
        }
    }
    Vec::new()
}

fn on_engine(model: &mut Model, msg: EngineMsg) -> Vec<Cmd> {
    match msg {
        EngineMsg::Snapshot(snapshot) => {
            on_snapshot(model, *snapshot);
            Vec::new()
        }
        EngineMsg::Event {
            scope,
            scope_seq,
            event,
        } => on_event(model, &scope, scope_seq, &event),
        EngineMsg::Resync { scope, reason } => on_resync(model, scope, reason),
        EngineMsg::Sessions { repo_id, result } => {
            on_sessions(model, &repo_id, &result);
            Vec::new()
        }
        EngineMsg::History { repo_id, events } => {
            on_history(model, &repo_id, &events);
            Vec::new()
        }
    }
}

/// The latest Git events, asked right after the repo's sessions: the last commit of each
/// worktree (US-CKP-026), merged with what the stream already applied (the later one wins).
fn on_history(model: &mut Model, repo_id: &str, events: &[GitEventView]) {
    let Some(data) = selected(model, repo_id).and_then(|r| r.data.as_mut()) else {
        return;
    };
    for commit in events.iter().filter_map(ingest::last_commit) {
        data.record(commit);
    }
    model.dirty = true;
}

/// The sessions listed right after the repo's snapshot, merged with what the stream already
/// applied (the same upsert: neither order regresses a session).
fn on_sessions(model: &mut Model, repo_id: &str, result: &SessionsListResult) {
    let Some(data) = selected(model, repo_id).and_then(|r| r.data.as_mut()) else {
        return;
    };
    data.detection = Some(result.detection_available);
    for view in &result.sessions {
        data.upsert(ingest::session(view));
    }
    model.dirty = true;
}

fn on_snapshot(model: &mut Model, snapshot: ScopeSnapshot) {
    model.dirty = true;
    match snapshot {
        ScopeSnapshot::Global(global) => {
            let replica = &mut model.engine.global;
            replica.data = Some(ingest::global(&global));
            synced(replica, &global.run_id, global.scope_seq);
        }
        ScopeSnapshot::Repo(repo) => {
            let replica = model.engine.repo.get_or_insert_with(ScopeReplica::default);
            replica.data = Some(ingest::repo(&repo));
            // The repo that was being observed is shown: the question is over.
            if matches!(model.ui.pick, Pick::Asking | Pick::Observing) {
                model.ui.asked = true;
                model.ui.here = None;
            }
            model.ui.pick = Pick::None;
            synced(replica, &repo.run_id, repo.scope_seq);
        }
    }
    if model.conn == ConnState::Resyncing && model.engine.all_synced() {
        model.conn = ConnState::Live;
    }
}

fn synced<T>(replica: &mut ScopeReplica<T>, run_id: &str, seq: u64) {
    replica.track.reset(run_id, seq);
    replica.stale = false;
    replica.applied = 0;
}

fn on_event(
    model: &mut Model,
    scope: &Scope,
    scope_seq: u64,
    event: &gitraptor_api::Event,
) -> Vec<Cmd> {
    let verdict = match scope {
        Scope::Global => model.engine.global.track.accept(scope_seq),
        Scope::Repo { repo_id } => match selected(model, repo_id) {
            Some(replica) => replica.track.accept(scope_seq),
            // Not the selected repo: not ours to apply.
            None => return Vec::new(),
        },
    };
    match verdict {
        Verdict::Duplicate | Verdict::Drop => Vec::new(),
        Verdict::Gap => start_resync(model, scope.clone(), false),
        Verdict::Apply => {
            apply(model, scope, event);
            Vec::new()
        }
    }
}

fn selected<'m>(
    model: &'m mut Model,
    repo_id: &str,
) -> Option<&'m mut ScopeReplica<crate::model::RepoView>> {
    model
        .engine
        .repo
        .as_mut()
        .filter(|r| r.data.as_ref().is_some_and(|d| d.repo_id == repo_id))
}

/// Applies one event in order. Kinds the skeleton does not show only move
/// the sequence; their views belong to each story.
fn apply(model: &mut Model, scope: &Scope, event: &gitraptor_api::Event) {
    model.dirty = true;
    match scope {
        Scope::Global => {
            if event.kind == REPO_DISCOVERED {
                model.engine.global.applied += 1;
                if let Ok(found) = serde_json::from_value::<RepoDiscoveredData>(event.data.clone())
                {
                    let found: Vec<Found> = found.candidates.iter().map(ingest::found).collect();
                    enqueue_discovered(model, found);
                }
                return;
            }
            let replica = &mut model.engine.global;
            replica.applied += 1;
            if event.kind == ENGINE_STATE
                && let (Some(data), Ok(view)) = (
                    replica.data.as_mut(),
                    serde_json::from_value::<EngineView>(event.data.clone()),
                )
            {
                data.engine = view.state;
                data.git_version = view.git_version.as_deref().map(SafeText::name);
            }
        }
        Scope::Repo { .. } => {
            let Some(replica) = model.engine.repo.as_mut() else {
                return;
            };
            replica.applied += 1;
            let Some(data) = replica.data.as_mut() else {
                return;
            };
            if event.kind == WORKTREE_STATE
                && let Ok(state) = serde_json::from_value::<WorktreeStateData>(event.data.clone())
            {
                data.worktrees = ingest::worktrees(&state.worktrees);
                data.fetched_ms = state.fetched_utc_ms;
            } else if event.kind == SESSION_STATE
                && let Ok(view) = serde_json::from_value::<SessionView>(event.data.clone())
                && view.repo_id == data.repo_id
            {
                data.upsert(ingest::session(&view));
            } else if event.kind == GIT_EVENT
                && let Ok(view) = serde_json::from_value::<GitEventView>(event.data.clone())
                && view.repo_id == data.repo_id
                && let Some(commit) = ingest::last_commit(&view)
            {
                data.record(commit);
            }
        }
    }
}

fn on_resync(model: &mut Model, scope: Scope, reason: ResyncReason) -> Vec<Cmd> {
    model.dirty = true;
    if reason == ResyncReason::ScopeClosed
        && let Scope::Repo { repo_id } = &scope
    {
        // The repo is no longer observed: its subscription ended.
        if selected(model, repo_id).is_some() {
            model.engine.repo = None;
        }
        return Vec::new();
    }
    start_resync(model, scope, true)
}

fn start_resync(model: &mut Model, scope: Scope, resubscribe: bool) -> Vec<Cmd> {
    model.dirty = true;
    match &scope {
        Scope::Global => model.engine.global.mark_stale(),
        Scope::Repo { .. } => {
            if let Some(repo) = model.engine.repo.as_mut() {
                repo.mark_stale();
            }
        }
    }
    if model.conn == ConnState::Live {
        model.conn = ConnState::Resyncing;
    }
    vec![Cmd::Resync { scope, resubscribe }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Size, Stamped};
    use crate::present::i18n::Lang;
    use gitraptor_api::Untrusted;
    use gitraptor_api::event::Event;
    use gitraptor_api::messages::{
        BaseBranchView, BaseStatusView, DaemonView, EngineStateView, RepoStateView, RepoView,
        SessionStateView,
    };
    use gitraptor_api::scope::{AutostartView, GlobalSnapshot, RepoSnapshot};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    fn model() -> Model {
        let mut model = Model::new(
            Lang::En,
            Size {
                width: 80,
                height: 24,
            },
        );
        model.conn = ConnState::Live;
        model
    }

    fn engine(msg: EngineMsg) -> Msg {
        Msg::Engine(Stamped {
            recv_ns: 0,
            decoded_ns: 0,
            msg,
        })
    }

    fn global_snapshot(seq: u64, state: EngineStateView) -> Msg {
        engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Global(
            GlobalSnapshot {
                run_id: "run".into(),
                scope_seq: seq,
                engine: EngineView {
                    state,
                    git_version: Some("2.50.0".into()),
                },
                daemon: DaemonView {
                    pid: 1,
                    protocol: 6,
                    binary_version: "0.0.0".into(),
                    started_wall_ms: 0,
                },
                autostart: AutostartView::Unknown,
                repos: Vec::new(),
            },
        ))))
    }

    fn repo_snapshot(seq: u64) -> Msg {
        engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
            RepoSnapshot {
                run_id: "run".into(),
                scope_seq: seq,
                repo: RepoView {
                    fetched_utc_ms: None,
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/\u{1b}]0;x\u{7}/.git"),
                    base: BaseBranchView {
                        name: None,
                        status: BaseStatusView::Invalid,
                    },
                    worktrees: Vec::new(),
                    tier: None,
                    checked_utc_ms: None,
                    kept_temps: None,
                },
            },
        ))))
    }

    fn engine_state(seq: u64, state: EngineStateView) -> Msg {
        engine(EngineMsg::Event {
            scope: Scope::Global,
            scope_seq: seq,
            event: Box::new(Event {
                seq: 100 + seq,
                kind: ENGINE_STATE.into(),
                version: 1,
                wall_ms: 0,
                timings: None,
                data: json!({"state": state, "git_version": null}),
            }),
        })
    }

    fn state(model: &Model) -> EngineStateView {
        model.engine.global.data.as_ref().unwrap().engine
    }

    #[test]
    fn a_duplicate_is_dropped_and_the_next_applies() {
        let mut m = model();
        update(&mut m, global_snapshot(3, EngineStateView::NoRepos));
        assert!(update(&mut m, engine_state(3, EngineStateView::Observing)).is_empty());
        assert_eq!(state(&m), EngineStateView::NoRepos);
        update(&mut m, engine_state(4, EngineStateView::Observing));
        assert_eq!(state(&m), EngineStateView::Observing);
        assert_eq!(m.engine.global.applied, 1);
    }

    #[test]
    fn a_gap_resyncs_and_applies_nothing_after_it() {
        let mut m = model();
        update(&mut m, global_snapshot(3, EngineStateView::NoRepos));
        let cmds = update(&mut m, engine_state(5, EngineStateView::Observing));
        assert_eq!(
            cmds,
            vec![Cmd::Resync {
                scope: Scope::Global,
                resubscribe: false
            }]
        );
        assert_eq!(m.conn, ConnState::Resyncing);
        assert!(m.engine.global.stale);
        // Neither the missing event nor later ones apply.
        for seq in [4, 6, 7] {
            assert!(update(&mut m, engine_state(seq, EngineStateView::Observing)).is_empty());
        }
        assert_eq!(state(&m), EngineStateView::NoRepos);
        // The new snapshot brings everything back.
        update(&mut m, global_snapshot(7, EngineStateView::WaitingForGit));
        assert_eq!(m.conn, ConnState::Live);
        assert!(!m.engine.global.stale);
        update(&mut m, engine_state(8, EngineStateView::Observing));
        assert_eq!(state(&m), EngineStateView::Observing);
    }

    #[test]
    fn a_daemon_resync_redoes_the_snapshot_and_resubscribes() {
        let mut m = model();
        update(&mut m, global_snapshot(3, EngineStateView::NoRepos));
        let cmds = update(
            &mut m,
            engine(EngineMsg::Resync {
                scope: Scope::Global,
                reason: ResyncReason::ReplayUnavailable,
            }),
        );
        assert_eq!(
            cmds,
            vec![Cmd::Resync {
                scope: Scope::Global,
                resubscribe: true
            }]
        );
        assert!(update(&mut m, engine_state(4, EngineStateView::Observing)).is_empty());
        assert_eq!(state(&m), EngineStateView::NoRepos);
    }

    #[test]
    fn a_closed_repo_scope_is_dropped() {
        let mut m = model();
        update(&mut m, repo_snapshot(2));
        assert!(m.engine.repo.is_some());
        let cmds = update(
            &mut m,
            engine(EngineMsg::Resync {
                scope: Scope::Repo {
                    repo_id: "r1".into(),
                },
                reason: ResyncReason::ScopeClosed,
            }),
        );
        assert!(cmds.is_empty());
        assert!(m.engine.repo.is_none());
    }

    #[test]
    fn a_reconnection_keeps_the_data_marked_stale_until_the_new_snapshot() {
        let mut m = model();
        update(&mut m, global_snapshot(3, EngineStateView::Observing));
        update(
            &mut m,
            Msg::Conn(ConnEvent::State(ConnState::Reconnecting { attempt: 1 })),
        );
        assert!(m.engine.global.stale);
        assert_eq!(state(&m), EngineStateView::Observing);
        assert!(!m.conn.writes_allowed());
        // A new daemon run starts its sequence again.
        update(&mut m, Msg::Conn(ConnEvent::State(ConnState::Syncing)));
        update(&mut m, global_snapshot(0, EngineStateView::NoRepos));
        update(&mut m, Msg::Conn(ConnEvent::State(ConnState::Live)));
        assert!(!m.engine.global.stale);
        update(&mut m, engine_state(1, EngineStateView::Observing));
        assert_eq!(state(&m), EngineStateView::Observing);
    }

    #[test]
    fn untrusted_text_enters_the_model_sanitized() {
        let mut m = model();
        update(&mut m, repo_snapshot(2));
        let path = &m.engine.repo.as_ref().unwrap().data.as_ref().unwrap().path;
        assert!(!path.as_str().contains('\u{1b}'));
        assert!(path.as_str().contains("\\x1b]0;x\\x07"));
    }

    #[test]
    fn events_of_another_repo_are_not_applied() {
        let mut m = model();
        update(&mut m, repo_snapshot(2));
        let other = engine(EngineMsg::Event {
            scope: Scope::Repo {
                repo_id: "r2".into(),
            },
            scope_seq: 9,
            event: Box::new(Event {
                seq: 1,
                kind: WORKTREE_STATE.into(),
                version: 1,
                wall_ms: 0,
                timings: None,
                data: json!({"repo_id": "r2", "worktrees": []}),
            }),
        });
        assert!(update(&mut m, other).is_empty());
        assert_eq!(m.engine.repo.as_ref().unwrap().applied, 0);
    }

    #[test]
    fn every_key_answers_and_quit_quits() {
        let mut m = model();
        m.dirty = false;
        update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        assert!(m.dirty);
        assert_eq!(m.ui.notice, Some(Notice::UnknownKey));
        update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        );
        assert_eq!(m.ui.notice, Some(Notice::AlreadyLive));
        m.conn = ConnState::EngineUnavailable;
        let cmds = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        );
        assert_eq!(cmds, vec![Cmd::Reconnect]);
        // Starting the engine is already the attempt: `r` says so and asks for nothing.
        m.conn = ConnState::Starting;
        let cmds = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        );
        assert!(cmds.is_empty());
        assert_eq!(m.ui.notice, Some(Notice::Starting));
        // `Ctrl-Z`: a suspension where there is job control, a typed hint elsewhere.
        let cmds = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)),
        );
        if cfg!(unix) {
            assert_eq!(cmds, vec![Cmd::Suspend]);
        } else {
            assert!(cmds.is_empty());
            assert_eq!(m.ui.notice, Some(Notice::SuspendUnsupported));
        }
        assert!(!m.ui.quit);
        let cmds = update(
            &mut m,
            Msg::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        );
        assert_eq!(cmds, vec![Cmd::Quit]);
        assert!(m.ui.quit);
    }

    #[test]
    fn a_resize_is_kept_and_repaints() {
        let mut m = model();
        m.dirty = false;
        let size = Size {
            width: 100,
            height: 30,
        };
        update(&mut m, Msg::Resize(size));
        assert_eq!(m.ui.size, size);
        assert!(m.dirty);
    }

    /// No request changes the replica: only engine messages do.
    #[test]
    fn writes_never_change_the_replica() {
        let mut m = model();
        update(&mut m, global_snapshot(3, EngineStateView::Observing));
        let before = m.engine.global.data.clone();
        for c in ['r', 'x', 'q'] {
            update(
                &mut m,
                Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
            );
        }
        assert_eq!(m.engine.global.data, before);
    }

    fn session_view(id: &str, state: SessionStateView, since: i64) -> SessionView {
        SessionView {
            repo_id: "r1".into(),
            session_id: id.into(),
            worktree: Untrusted::new("/w/a"),
            actor: gitraptor_api::Actor::Agent {
                kind: gitraptor_api::AgentKind::ClaudeCode,
                name: None,
                origin: gitraptor_api::AgentOrigin::Detected,
            },
            state,
            started_utc_ms: 0,
            state_since_utc_ms: since,
            utc_offset_s: 0,
            ended_utc_ms: None,
            end_cause: None,
        }
    }

    fn sessions_list(views: Vec<SessionView>) -> Msg {
        engine(EngineMsg::Sessions {
            repo_id: "r1".into(),
            result: Box::new(SessionsListResult {
                detection_available: true,
                sessions: views,
            }),
        })
    }

    fn session_event(seq: u64, view: &SessionView) -> Msg {
        engine(EngineMsg::Event {
            scope: Scope::Repo {
                repo_id: "r1".into(),
            },
            scope_seq: seq,
            event: Box::new(Event {
                seq,
                kind: SESSION_STATE.into(),
                version: 1,
                wall_ms: 0,
                timings: None,
                data: serde_json::to_value(view).unwrap(),
            }),
        })
    }

    fn sessions(m: &Model) -> Vec<(String, SessionStateView)> {
        m.engine
            .repo
            .as_ref()
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.state))
            .collect()
    }

    /// D1: an event read before the list answer but applied after it does not regress the
    /// session; a later one applies.
    #[test]
    fn the_session_list_and_the_stream_converge_in_any_order() {
        let mut m = model();
        update(&mut m, repo_snapshot(2));
        update(
            &mut m,
            sessions_list(vec![session_view("s1", SessionStateView::Inactive, 20)]),
        );
        update(
            &mut m,
            session_event(3, &session_view("s1", SessionStateView::Active, 10)),
        );
        assert_eq!(
            sessions(&m),
            vec![("s1".into(), SessionStateView::Inactive)]
        );
        update(
            &mut m,
            session_event(4, &session_view("s1", SessionStateView::Active, 30)),
        );
        assert_eq!(sessions(&m), vec![("s1".into(), SessionStateView::Active)]);
        // At the same instant, ended wins: an ended session is never reopened.
        update(
            &mut m,
            session_event(5, &session_view("s1", SessionStateView::Ended, 30)),
        );
        update(
            &mut m,
            session_event(6, &session_view("s1", SessionStateView::Active, 30)),
        );
        assert_eq!(sessions(&m), vec![("s1".into(), SessionStateView::Ended)]);
    }

    /// A new snapshot (gap, resync, reconnection) drops the sessions and their detection until
    /// the channel lists them again.
    #[test]
    fn a_new_repo_snapshot_waits_for_the_session_list_again() {
        let mut m = model();
        update(&mut m, repo_snapshot(2));
        update(
            &mut m,
            sessions_list(vec![session_view("s1", SessionStateView::Active, 1)]),
        );
        update(&mut m, repo_snapshot(9));
        let data = m.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
        assert!(data.sessions.is_empty());
        assert_eq!(data.detection, None);
    }

    #[test]
    fn a_worktree_state_event_replaces_the_rows() {
        let mut m = model();
        update(&mut m, repo_snapshot(2));
        let event = engine(EngineMsg::Event {
            scope: Scope::Repo {
                repo_id: "r1".into(),
            },
            scope_seq: 3,
            event: Box::new(Event {
                seq: 3,
                kind: WORKTREE_STATE.into(),
                version: 1,
                wall_ms: 0,
                timings: None,
                data: json!({"repo_id": "r1", "worktrees": [{
                    "path": {"untrusted": "/w/a\u{1b}]0;x\u{7}"},
                    "main": true,
                    "admin_name": null,
                    "status": {"state": "unavailable", "reason": "missing"}
                }]}),
            }),
        });
        update(&mut m, event);
        let data = m.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
        assert_eq!(data.worktrees.len(), 1);
        assert!(!data.worktrees[0].path.as_str().contains('\u{1b}'));
    }

    fn git_commit(seq: i64, worktree: &str, author: &str) -> gitraptor_api::messages::GitEventView {
        use gitraptor_api::messages::{
            DeclaredAuthorship, GitEventDetails, GitEventKind, GitEventView, GitIdentity,
        };
        let who = || GitIdentity {
            name: Untrusted::new(author),
            email: Untrusted::new("x@example.com"),
        };
        GitEventView {
            repo_id: "r1".into(),
            seq,
            worktree: Untrusted::new(worktree),
            kind: GitEventKind::Commit,
            actor: gitraptor_api::Actor::Unattributed,
            observed_utc_ms: 0,
            utc_offset_s: 0,
            details: GitEventDetails::default(),
            gap_id: None,
            inferred: None,
            authorship: Some(DeclaredAuthorship {
                author: who(),
                committer: who(),
                coauthors: Vec::new(),
            }),
        }
    }

    fn git_event(scope_seq: u64, view: &gitraptor_api::messages::GitEventView) -> Msg {
        engine(EngineMsg::Event {
            scope: Scope::Repo {
                repo_id: "r1".into(),
            },
            scope_seq,
            event: Box::new(Event {
                seq: 200 + scope_seq,
                kind: GIT_EVENT.into(),
                version: 1,
                wall_ms: 0,
                timings: None,
                data: serde_json::to_value(view).unwrap(),
            }),
        })
    }

    fn authors(m: &Model) -> Vec<String> {
        let data = m.engine.repo.as_ref().unwrap().data.as_ref().unwrap();
        data.commits
            .iter()
            .map(|c| c.author.as_str().to_owned())
            .collect()
    }

    /// US-CKP-026: the last commit of each worktree comes from the history and then the stream;
    /// the most recent wins, an older one never replaces it, and a new snapshot (a resync or a
    /// reconnection) drops them until the history arrives again, so none is left stale.
    #[test]
    fn the_last_commit_comes_from_history_and_stream() {
        let mut m = model();
        update(&mut m, repo_snapshot(3));
        update(
            &mut m,
            engine(EngineMsg::History {
                repo_id: "r1".into(),
                events: vec![git_commit(4, "/w/a", "Ana"), git_commit(5, "/w/b", "Bea")],
            }),
        );
        assert_eq!(authors(&m), ["Ana", "Bea"]);
        update(&mut m, git_event(4, &git_commit(7, "/w/a", "Carla")));
        assert_eq!(authors(&m), ["Carla", "Bea"]);
        // An older one arriving late does not replace it.
        update(
            &mut m,
            engine(EngineMsg::History {
                repo_id: "r1".into(),
                events: vec![git_commit(4, "/w/a", "Ana")],
            }),
        );
        assert_eq!(authors(&m), ["Carla", "Bea"]);
        // Another repo's history is not ours.
        update(
            &mut m,
            engine(EngineMsg::History {
                repo_id: "r2".into(),
                events: vec![git_commit(9, "/w/c", "Dora")],
            }),
        );
        assert_eq!(authors(&m), ["Carla", "Bea"]);
        // A resync: the new snapshot drops them, the history rebuilds them.
        update(&mut m, repo_snapshot(10));
        assert!(authors(&m).is_empty());
        update(
            &mut m,
            engine(EngineMsg::History {
                repo_id: "r1".into(),
                events: vec![git_commit(8, "/w/b", "Eva")],
            }),
        );
        assert_eq!(authors(&m), ["Eva"]);
    }

    fn notes() -> Candidate {
        Candidate {
            root: "/w/notes".into(),
            name: SafeText::name("notes"),
            path: SafeText::text("/w/notes"),
        }
    }

    fn developer() -> Option<Requester> {
        Some(Requester::Unattributed {
            layer: Layer::Cockpit,
        })
    }

    fn press(m: &mut Model, code: KeyCode) -> Vec<Cmd> {
        update(m, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn found(name: &str) -> Found {
        Found {
            path: format!("/code/{name}").into(),
            name: SafeText::name(name),
            shown: SafeText::text(&format!("/code/{name}")),
            root: SafeText::text("/code"),
        }
    }

    fn discovering(names: &[&str]) -> Model {
        let mut m = model();
        m.engine.requester = developer();
        let found = names.iter().map(|n| found(n)).collect();
        update(&mut m, Msg::Conn(ConnEvent::Discovered(found)));
        m
    }

    fn front(m: &Model) -> Option<String> {
        m.discovery_prompt().map(|f| f.name.as_str().to_owned())
    }

    /// US-GRP-020: one prompt at a time, in order; `s` observes without leaving, `n` dismisses.
    #[test]
    fn discovered_repos_are_asked_one_at_a_time() {
        let mut m = discovering(&["a", "b"]);
        assert_eq!(front(&m).as_deref(), Some("a"));
        assert_eq!(
            press(&mut m, KeyCode::Char('s')),
            vec![Cmd::AcceptDiscovered {
                path: "/code/a".into()
            }]
        );
        assert_eq!(front(&m).as_deref(), Some("b"));
        assert_eq!(
            press(&mut m, KeyCode::Char('n')),
            vec![Cmd::DismissDiscovered {
                path: "/code/b".into()
            }]
        );
        assert_eq!(front(&m), None);
        // Nothing left: the keys are the plain ones again.
        assert!(press(&mut m, KeyCode::Char('s')).is_empty());
    }

    /// `Esc` is later: nothing is sent, and the same repo is not asked again in this run, not
    /// even when the engine lists it again after a reconnection.
    #[test]
    fn discovered_later_decides_nothing_and_does_not_nag() {
        let mut m = discovering(&["a", "b"]);
        assert!(press(&mut m, KeyCode::Esc).is_empty());
        assert_eq!(front(&m).as_deref(), Some("b"));
        update(
            &mut m,
            Msg::Conn(ConnEvent::Discovered(vec![found("a"), found("b")])),
        );
        assert_eq!(m.ui.discovered.len(), 1);
        assert!(press(&mut m, KeyCode::Esc).is_empty());
        assert_eq!(front(&m), None);
    }

    /// Only the developer is asked, never while a repo is being picked or opened, and the
    /// answer keys keep their old meaning (none) when nothing is asked.
    #[test]
    fn discovered_never_takes_focus_and_never_asks_others() {
        let mut m = model();
        update(&mut m, Msg::Conn(ConnEvent::Discovered(vec![found("a")])));
        assert_eq!(front(&m), None, "requester unknown");
        assert!(press(&mut m, KeyCode::Char('s')).is_empty());
        m.engine.requester = Some(Requester::Unverified);
        assert_eq!(front(&m), None);
        m.engine.requester = developer();
        assert_eq!(front(&m).as_deref(), Some("a"));
        for pick in [
            Pick::Choosing { selected: 0 },
            Pick::Opening,
            Pick::Asking,
            Pick::Observing,
        ] {
            m.ui.pick = pick;
            assert_eq!(front(&m), None, "{pick:?}");
        }
        // A pick of the folder is answered by its own question.
        m.ui.pick = Pick::Asking;
        m.ui.here = Some(notes());
        assert_eq!(
            press(&mut m, KeyCode::Char('s')),
            vec![Cmd::Observe {
                root: "/w/notes".into()
            }]
        );
    }

    /// The same path from the event and from the list is queued once.
    #[test]
    fn discovered_event_and_list_are_deduplicated() {
        use gitraptor_api::discovery::{CandidateView, RepoDiscoveredData};
        let mut m = model();
        update(&mut m, global_snapshot(1, EngineStateView::Observing));
        let data = RepoDiscoveredData {
            root: "/code".into(),
            candidates: vec![CandidateView {
                path: "/code/a".into(),
                name: "a".into(),
                root: "/code".into(),
                found_utc_ms: 0,
            }],
            count: 1,
        };
        let event = Event {
            seq: 2,
            kind: REPO_DISCOVERED.into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: serde_json::to_value(data).unwrap(),
        };
        update(
            &mut m,
            engine(EngineMsg::Event {
                scope: Scope::Global,
                scope_seq: 2,
                event: Box::new(event),
            }),
        );
        update(&mut m, Msg::Conn(ConnEvent::Discovered(vec![found("a")])));
        assert_eq!(m.ui.discovered.len(), 1);
        assert_eq!(m.ui.discovered[0].root.as_str(), "/code");
    }

    /// A failed answer says so and goes on.
    #[test]
    fn discovered_failure_shows_a_notice() {
        let mut m = model();
        update(
            &mut m,
            Msg::Conn(ConnEvent::DiscoveryFailed(ObserveFailure::Refused)),
        );
        assert_eq!(
            m.ui.notice,
            Some(Notice::DiscoveryFailed(ObserveFailure::Refused))
        );
    }

    /// US-CKP-025: only a connection the engine resolved as the developer is asked; an agent,
    /// an unverified or unknown caller, or the MCP layer go on as outside any repo.
    #[test]
    fn observe_asks_only_the_developer() {
        let others = [
            None,
            Some(Requester::Unverified),
            Some(Requester::Agent {
                name: None,
                layer: Layer::Mcp,
            }),
            Some(Requester::Unattributed { layer: Layer::Mcp }),
        ];
        for requester in others {
            let mut m = model();
            m.engine.requester = requester.clone();
            let cmds = update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
            assert!(cmds.is_empty(), "{requester:?}");
            assert_eq!(m.ui.pick, Pick::None, "{requester:?}");
            assert_eq!(m.ui.here, None, "{requester:?}");
            // A yes key does nothing but say so.
            assert!(press(&mut m, KeyCode::Char('y')).is_empty());
            assert_eq!(m.ui.notice, Some(Notice::UnknownKey));
        }
        let mut m = model();
        m.engine.requester = developer();
        assert!(update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes()))).is_empty());
        assert_eq!(m.ui.pick, Pick::Asking);
        assert_eq!(m.ui.here, Some(notes()));
    }

    /// `y` and `s` observe the repo of the folder; `n`, Esc and Enter (the default) do not.
    #[test]
    fn observe_yes_observes_and_no_goes_on() {
        for key in [KeyCode::Char('y'), KeyCode::Char('s')] {
            let mut m = model();
            m.engine.requester = developer();
            update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
            assert_eq!(
                press(&mut m, key),
                vec![Cmd::Observe {
                    root: "/w/notes".into()
                }]
            );
            assert_eq!(m.ui.pick, Pick::Observing);
            assert!(m.ui.asked);
        }
        for key in [KeyCode::Char('n'), KeyCode::Esc, KeyCode::Enter] {
            let mut m = model();
            m.engine.requester = developer();
            update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
            // The arrows do not answer.
            assert!(press(&mut m, KeyCode::Down).is_empty());
            assert_eq!(m.ui.pick, Pick::Asking);
            assert!(press(&mut m, key).is_empty(), "{key:?}");
            assert_eq!(m.ui.pick, Pick::None, "{key:?}");
            assert!(m.ui.asked);
        }
    }

    /// Asked once per run: after a no, a reconnection does not ask again; while the question
    /// is on screen, a reconnection keeps it.
    #[test]
    fn observe_is_asked_once_per_run() {
        let mut m = model();
        m.engine.requester = developer();
        update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
        update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
        assert_eq!(m.ui.pick, Pick::Asking);
        press(&mut m, KeyCode::Char('n'));
        update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
        assert_eq!(m.ui.pick, Pick::None);
    }

    /// A failed `repo.add` says why, is not asked again and goes on as outside any repo.
    #[test]
    fn observe_failure_says_why_and_goes_on() {
        let mut m = model();
        m.engine.requester = developer();
        update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
        press(&mut m, KeyCode::Char('y'));
        let cmds = update(
            &mut m,
            Msg::Conn(ConnEvent::ObserveFailed(ObserveFailure::NotTrusted)),
        );
        assert!(cmds.is_empty());
        assert_eq!(
            m.ui.notice,
            Some(Notice::ObserveFailed(ObserveFailure::NotTrusted))
        );
        assert_eq!(m.ui.pick, Pick::None);
        update(&mut m, Msg::Conn(ConnEvent::Unobserved(notes())));
        assert_eq!(m.ui.pick, Pick::None);
        // The reconnection after a dropped `repo.add` does not erase why.
        update(
            &mut m,
            Msg::Conn(ConnEvent::ObserveFailed(ObserveFailure::Disconnected)),
        );
        update(&mut m, Msg::Conn(ConnEvent::State(ConnState::Live)));
        assert_eq!(
            m.ui.notice,
            Some(Notice::ObserveFailed(ObserveFailure::Disconnected))
        );
    }
}
