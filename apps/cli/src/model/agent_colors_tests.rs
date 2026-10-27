use gitraptor_api::AgentKind;
use gitraptor_api::messages::SessionStateView;

use super::*;

fn session(id: &str, started: i64, state: SessionStateView) -> SessionRow {
    SessionRow {
        session_id: id.into(),
        worktree: 1,
        kind: AgentKind::ClaudeCode,
        name: None,
        state,
        state_since_ms: started,
        started_ms: started,
        ended_ms: None,
        end_cause: None,
    }
}

fn present(n: usize) -> Vec<SessionRow> {
    (0..n)
        .map(|i| session(&format!("s{i}"), i as i64, SessionStateView::Active))
        .collect()
}

#[test]
fn the_ninth_present_session_takes_slot_eight() {
    let mut colors = AgentColors::default();
    colors.open("r");
    colors.sync(&present(9), true);
    assert_eq!(colors.index("s0"), Some(0));
    assert_eq!(colors.index("s8"), Some(8));
}

#[test]
fn an_ended_session_frees_its_slot_for_the_next_one() {
    let mut colors = AgentColors::default();
    colors.open("r");
    let mut sessions = present(2);
    colors.sync(&sessions, true);
    sessions[0].state = SessionStateView::Ended;
    sessions.push(session("s2", 9, SessionStateView::Active));
    colors.sync(&sessions, false);
    assert_eq!(colors.index("s0"), None);
    assert_eq!(colors.index("s1"), Some(1));
    assert_eq!(colors.index("s2"), Some(0));
}

#[test]
fn a_partial_update_never_frees_a_slot() {
    let mut colors = AgentColors::default();
    colors.open("r");
    colors.sync(&present(3), true);
    colors.sync(&present(3)[1..2], false);
    assert_eq!(colors.index("s0"), Some(0));
    assert_eq!(colors.index("s2"), Some(2));
}

#[test]
fn a_complete_list_frees_the_slots_it_lacks() {
    let mut colors = AgentColors::default();
    colors.open("r");
    colors.sync(&present(3), true);
    colors.sync(&present(3)[1..2], true);
    assert_eq!(colors.index("s0"), None);
    assert_eq!(colors.index("s1"), Some(1));
}

#[test]
fn another_repo_empties_the_register() {
    let mut colors = AgentColors::default();
    colors.open("r");
    colors.sync(&present(2), true);
    colors.open("r");
    assert_eq!(colors.index("s1"), Some(1));
    colors.open("other");
    assert_eq!(colors.index("s1"), None);
}
