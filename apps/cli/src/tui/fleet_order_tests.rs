use gitraptor_api::AgentKind;

use super::*;

const H: i64 = 3_600_000;
const NOW: i64 = 100 * H;

fn session(id: &str, state: SessionStateView, started: i64, ended: Option<i64>) -> SessionRow {
    SessionRow {
        session_id: id.into(),
        worktree: 1,
        kind: AgentKind::ClaudeCode,
        name: None,
        state,
        state_since_ms: ended.unwrap_or(started),
        started_ms: started,
        ended_ms: ended,
        end_cause: None,
    }
}

fn ids<'a>(shown: &Shown<'a>) -> Vec<&'a str> {
    shown
        .sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect()
}

#[test]
fn groups_go_main_attention_active_inactive_ended_no_agent() {
    use SessionStateView::*;
    assert!(Group::Main < Group::Attention && Group::Attention < Group::Active);
    assert!(Group::Active < Group::Inactive && Group::Inactive < Group::Ended);
    assert!(Group::Ended < Group::NoAgent);
    assert_eq!(group(true, true, Some(Active)), Group::Main);
    assert_eq!(group(false, true, Some(Inactive)), Group::Attention);
    assert_eq!(group(false, false, Some(Ended)), Group::Ended);
    assert_eq!(group(false, false, None), Group::NoAgent);
}

#[test]
fn an_ended_session_is_hidden_at_24_hours() {
    let s = [session(
        "e",
        SessionStateView::Ended,
        0,
        Some(NOW - 24 * H + 1),
    )];
    assert_eq!(ids(&shown(&s, 1, NOW, false)), ["e"]);
    let s = [session("e", SessionStateView::Ended, 0, Some(NOW - 24 * H))];
    let hidden = shown(&s, 1, NOW, false);
    assert!(hidden.sessions.is_empty() && hidden.hidden);
    assert_eq!(hidden.last_agent.map(|a| a.session_id.as_str()), Some("e"));
    assert_eq!(ids(&shown(&s, 1, NOW, true)), ["e"]);
}

#[test]
fn a_later_session_hides_the_ended_one_and_an_overlapping_one_does_not() {
    let ended = session("e", SessionStateView::Ended, 0, Some(10 * H));
    let after = session("a", SessionStateView::Active, 11 * H, None);
    let overlap = session("o", SessionStateView::Active, 5 * H, None);
    assert_eq!(
        ids(&shown(&[ended.clone(), after], 1, 12 * H, false)),
        ["a"]
    );
    assert_eq!(ids(&shown(&[ended, overlap], 1, 12 * H, false)), ["o", "e"]);
}

#[test]
fn two_present_sessions_are_shared() {
    let s = [
        session("a", SessionStateView::Active, 0, None),
        session("b", SessionStateView::Inactive, 1, None),
    ];
    assert!(is_shared(&shown(&s, 1, NOW, false)));
}

#[test]
fn an_ended_and_an_inactive_session_are_not_shared() {
    let s = [
        session("a", SessionStateView::Inactive, 0, None),
        session("e", SessionStateView::Ended, 1, Some(2 * H)),
    ];
    for filter in [false, true] {
        assert!(!is_shared(&shown(&s, 1, 3 * H, filter)));
    }
}

#[test]
fn only_the_latest_ended_session_of_a_worktree_counts() {
    let s = [
        session("old", SessionStateView::Ended, 0, Some(NOW - H)),
        session("new", SessionStateView::Ended, 5, Some(NOW - 2 * H)),
    ];
    assert_eq!(ids(&shown(&s, 1, NOW, false)), ["new"]);
}

#[test]
fn an_end_without_time_counts_from_when_the_engine_published_it() {
    let mut s = session("e", SessionStateView::Ended, 0, None);
    s.state_since_ms = 7;
    assert_eq!(
        ended_at(&s).map(|e| (e.at_ms, e.kind)),
        Some((7, EndKind::NotAvailable))
    );
    s.end_cause = Some(SessionEndCauseView::EndedDuringGap);
    assert_eq!(ended_at(&s).map(|e| e.kind), Some(EndKind::Unobserved));
    assert_eq!(
        ended_at(&session("a", SessionStateView::Active, 0, None)),
        None
    );
}
