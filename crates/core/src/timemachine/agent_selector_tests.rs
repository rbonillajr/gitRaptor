//! TMC011-SELECTOR (T003 of DS-US-TMC-011): which sessions `--agent` names. Child module of
//! `agent_selector` (`use super::*`), mounted there as `mod tests`.

use super::*;
use crate::timemachine::engine::SessionAttribution;
use crate::timemachine::oplog::{Requester, RequesterOrigin};
use std::collections::HashMap;

fn agent(name: &str, session: &str) -> Requester {
    Requester::Agent {
        name: name.into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn sessions() -> SessionAttribution {
    SessionAttribution(HashMap::from([
        ("100:1".to_owned(), agent("claude-code", "100:1")),
        ("200:1".to_owned(), agent("claude-code", "200:1")),
        ("300:1".to_owned(), agent("codex-1", "300:1")),
    ]))
}

#[test]
fn a_session_id_selects_that_session() {
    let map = sessions();
    let selector = AgentSelector::resolve("100:1", &map);
    assert_eq!(
        selector,
        AgentSelector::Session("100:1".into()),
        "TMC011-SELECTOR: a key of the sessions map is a session id"
    );
    assert!(
        selector.selects(&map.0["100:1"]),
        "TMC011-SELECTOR: the session selects its own work"
    );
    assert!(
        !selector.selects(&map.0["200:1"]),
        "TMC011-SELECTOR: another session of the same agent is not selected by id"
    );
    assert!(!selector.selects(&map.0["300:1"]));
}

#[test]
fn a_name_selects_by_current_attribution() {
    let map = sessions();
    let selector = AgentSelector::resolve("codex-1", &map);
    assert_eq!(
        selector,
        AgentSelector::Name("codex-1".into()),
        "TMC011-SELECTOR: text that is no session id is a name"
    );
    assert!(
        selector.selects(&map.0["300:1"]),
        "TMC011-SELECTOR: a name selects the sessions whose current attribution carries it"
    );
    assert!(
        !selector.selects(&map.0["100:1"]),
        "TMC011-SELECTOR: another agent is not selected"
    );
    // A corrected session is judged by what it is now: the map holds the current actor.
    let corrected = SessionAttribution(HashMap::from([(
        "100:1".to_owned(),
        agent("codex-1", "100:1"),
    )]));
    assert!(
        AgentSelector::resolve("codex-1", &corrected).selects(&corrected.0["100:1"]),
        "TMC011-SELECTOR: a session corrected to codex-1 is selected by that name"
    );
    assert!(
        !AgentSelector::resolve("claude-code", &corrected).selects(&corrected.0["100:1"]),
        "TMC011-SELECTOR: and no longer by claude-code"
    );
}

#[test]
fn claude_code_matches_every_spelling_the_timeline_matches() {
    let map = sessions();
    let selector = AgentSelector::resolve("claude-code", &map);
    for spelling in ["claude-code", "Claude Code", "claude code", "CLAUDE_CODE"] {
        assert!(
            selector.selects(&agent(spelling, "100:1")),
            "TMC011-SELECTOR: claude-code must match `{spelling}` as the timeline does"
        );
    }
    for other in ["claude", "claude-code-2", "codex-1", ""] {
        assert!(
            !selector.selects(&agent(other, "100:1")),
            "TMC011-SELECTOR: claude-code must not match `{other}`"
        );
    }
    // Any other name matches only exactly.
    let codex = AgentSelector::resolve("codex-1", &map);
    assert!(!codex.selects(&agent("Codex-1", "300:1")));
}

#[test]
fn unattributed_is_never_selected() {
    let map = sessions();
    for selector in [
        AgentSelector::resolve("100:1", &map),
        AgentSelector::resolve("claude-code", &map),
        AgentSelector::resolve("unattributed", &map),
        AgentSelector::Session("100:1".into()),
        AgentSelector::Name("unattributed".into()),
    ] {
        assert!(
            !selector.selects(&Requester::Unattributed),
            "TMC011-SELECTOR: {selector:?} must never select unattributed work"
        );
    }
    assert!(
        !AgentSelector::Name("unattributed".into()).selects(&agent("unattributed", "1:1")),
        "TMC011-SELECTOR: unattributed is no name of an agent"
    );
}

#[test]
fn names_requester_follows_the_same_rule() {
    let map = sessions();
    let by_session = AgentSelector::resolve("100:1", &map);
    assert!(by_session.names_requester(&map.0["100:1"]));
    assert!(
        !by_session.names_requester(&map.0["200:1"]),
        "TMC011-SELECTOR: another session is not named"
    );
    let by_name = AgentSelector::resolve("claude-code", &map);
    assert!(by_name.names_requester(&map.0["200:1"]));
    assert!(!by_name.names_requester(&map.0["300:1"]));
    assert!(!by_name.names_requester(&Requester::Unattributed));
}
