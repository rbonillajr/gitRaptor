//! `update(&mut Model, Msg) -> Vec<Cmd>` (ADR-CKP-003 § 2): pure, no I/O,
//! no clock. The engine replica only changes through snapshots and events
//! of the engine; a gap or a `resync` stops applying and asks for a new
//! snapshot.

use gitraptor_api::event::{ENGINE_STATE, WORKTREE_STATE};
use gitraptor_api::messages::{EngineView, ResyncReason, WorktreeStateData};
use gitraptor_api::scope::{Scope, ScopeSnapshot};
use ratatui::crossterm::event::KeyEventKind;

use crate::client::sequence::Verdict;
use crate::model::{Cmd, ConnEvent, ConnState, EngineMsg, Model, Msg, Notice, ScopeReplica};
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
        Msg::Conn(event) => {
            on_conn(model, event);
            Vec::new()
        }
        // Nothing visible depends on the clock yet.
        Msg::Tick { now_ms } => {
            model.now_ms = now_ms;
            Vec::new()
        }
    }
}

/// Every key changes something visible (feedback < 100 ms, § 3).
fn on_action(model: &mut Model, action: Option<Action>) -> Vec<Cmd> {
    model.dirty = true;
    match action {
        Some(Action::Quit) => {
            model.ui.quit = true;
            vec![Cmd::Quit]
        }
        Some(Action::Retry) if model.conn == ConnState::Live => {
            model.ui.notice = Some(Notice::AlreadyLive);
            Vec::new()
        }
        Some(Action::Retry) => {
            model.ui.notice = Some(Notice::Retrying);
            vec![Cmd::Reconnect]
        }
        None => {
            model.ui.notice = Some(Notice::UnknownKey);
            Vec::new()
        }
    }
}

fn on_conn(model: &mut Model, event: ConnEvent) {
    model.dirty = true;
    match event {
        ConnEvent::Requester(requester) => model.engine.requester = requester,
        ConnEvent::State(state) => {
            if matches!(
                state,
                ConnState::Connecting | ConnState::Reconnecting { .. } | ConnState::Syncing
            ) {
                model.engine.mark_all_stale();
            }
            if state == ConnState::Live {
                model.ui.notice = None;
            }
            model.conn = state;
        }
    }
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
    }
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
            if event.kind == WORKTREE_STATE
                && let (Some(data), Ok(state)) = (
                    replica.data.as_mut(),
                    serde_json::from_value::<WorktreeStateData>(event.data.clone()),
                )
            {
                data.worktree_count = state.worktrees.len();
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
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/\u{1b}]0;x\u{7}/.git"),
                    base: BaseBranchView {
                        name: None,
                        status: BaseStatusView::Invalid,
                    },
                    worktrees: Vec::new(),
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
}
