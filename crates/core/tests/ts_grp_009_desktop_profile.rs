//! TS-GRP-009 (DS-TS-GRP-009 § 9.4, file 1): the profile the daemon fixes for a connection of the
//! desktop class (D3, first rule that applies). `decide` is driven with a process-table double
//! (`Tree`, the pattern of the tests of `requester.rs`) that fabricates the ancestries.
//!
//! The `Checks` of every test have `terminal_proof` and `orphans_marked` (the Unix values): without
//! them a doubtful chain would not resolve and a test of "ambiguous ancestry" would pass through
//! the wrong rule (`requester-unverified`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitraptor_api::desktop::{DesktopIdentity, DesktopReason};
use gitraptor_api::messages::{ClientKind, ConnectionProfile};
use gitraptor_core::channel::AgentMatcher;
use gitraptor_core::channel::authz::{AcceptedPeer, Checks, check_reserved};
use gitraptor_core::channel::desktop::{DesktopDecision, PathIdentity, decide};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::{ProcError, ProcInfo, ProcSource};
use gitraptor_core::channel::requester::{Who, resolve};

const UID: u32 = 501;
const DAEMON: (u32, u64) = (70, 700);
const CLAUDE: &str = "/opt/homebrew/Caskroom/claude-code@latest/2.1.284/claude";
const APP: &str = "/Applications/GitRaptor.app/Contents/MacOS/GitRaptor";

#[derive(Default)]
struct Tree {
    procs: HashMap<u32, ProcInfo>,
    denied: HashMap<u32, bool>,
}

impl Tree {
    fn add(&mut self, pid: u32, ppid: u32, exe: &str, start: u64, tty: bool, session: u32) {
        self.procs.insert(
            pid,
            ProcInfo {
                pid,
                ppid,
                uid: UID,
                start_us: start,
                exe: Some(PathBuf::from(exe)),
                controlling_terminal: tty,
                session,
                pgid: pid,
                desktop_session: None,
            },
        );
    }
}

impl ProcSource for Tree {
    fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
        if self.denied.contains_key(&pid) {
            return Err(ProcError::Denied);
        }
        self.procs.get(&pid).cloned().ok_or(ProcError::Gone)
    }
    fn foreign_to(&self, pid: u32, _uid: u32) -> Option<bool> {
        self.denied.get(&pid).copied()
    }
    fn pids_of(&self, _uid: u32) -> Option<Vec<u32>> {
        Some(self.procs.keys().copied().collect())
    }
}

/// A process table and the process of the app in it.
struct Scene {
    tree: Tree,
    pid: u32,
    start: u64,
}

/// Terminal.app: launchd → login (root, leader 10) → zsh (20) → the app (30).
fn terminal() -> Scene {
    let mut tree = base();
    tree.add(30, 20, APP, 300, true, 10);
    Scene {
        tree,
        pid: 30,
        start: 300,
    }
}

/// The login chain every terminal scene hangs from.
fn base() -> Tree {
    let mut tree = Tree::default();
    tree.denied.insert(10, true);
    tree.add(20, 10, "/bin/zsh", 200, true, 10);
    tree
}

/// Opened from the Finder: its parent is launchd (root's), no terminal.
fn finder() -> Scene {
    let mut tree = Tree::default();
    tree.denied.insert(1, true);
    tree.add(30, 1, APP, 300, false, 30);
    Scene {
        tree,
        pid: 30,
        start: 300,
    }
}

/// `claude` → zsh → the app.
fn under_agent() -> Scene {
    let mut tree = base();
    tree.add(25, 20, CLAUDE, 250, true, 10);
    tree.add(40, 25, "/bin/zsh", 400, false, 40);
    tree.add(41, 40, APP, 410, false, 40);
    Scene {
        tree,
        pid: 41,
        start: 410,
    }
}

/// A `node` (an interpreter that may host an agent) between the shell and the app.
fn under_interpreter() -> Scene {
    let mut tree = base();
    tree.add(26, 20, "/usr/local/bin/node", 260, true, 10);
    tree.add(32, 26, APP, 320, true, 10);
    Scene {
        tree,
        pid: 32,
        start: 320,
    }
}

/// A tmux server with the app's pane; with `agents` Claude Code sessions in other panes of it.
fn under_multiplexer(agents: usize) -> Scene {
    let mut tree = Tree::default();
    tree.add(1, 0, "/sbin/launchd", 1, false, 1);
    tree.add(80, 1, "/opt/homebrew/bin/tmux", 800, false, 80);
    tree.add(90, 80, "/bin/zsh", 900, true, 90);
    tree.add(91, 90, APP, 910, true, 90);
    for i in 0..agents as u32 {
        tree.add(
            81 + 2 * i,
            80,
            "/bin/zsh",
            810 + 20 * i as u64,
            true,
            81 + 2 * i,
        );
        tree.add(
            82 + 2 * i,
            81 + 2 * i,
            CLAUDE,
            820 + 20 * i as u64,
            true,
            81 + 2 * i,
        );
    }
    Scene {
        tree,
        pid: 91,
        start: 910,
    }
}

/// The shell is younger than the app: a reused pid breaks the walk.
fn broken() -> Scene {
    let mut scene = terminal();
    scene.tree.add(20, 10, "/bin/zsh", 900, true, 10);
    scene
}

/// The app is a child of the daemon (`DAEMON`) and nothing marks it.
fn under_daemon() -> Scene {
    let mut tree = base();
    tree.add(70, 20, "/usr/local/bin/raptor", 700, false, 70);
    tree.add(71, 70, APP, 710, false, 70);
    Scene {
        tree,
        pid: 71,
        start: 710,
    }
}

fn peer(pid: u32, start: u64) -> AcceptedPeer {
    AcceptedPeer {
        pid,
        start_us: start,
        accepted_us: 1_000_000,
    }
}

fn checks<'a>(
    tree: &'a Tree,
    matcher: &'a AgentMatcher,
    marks: Option<&'a ExecutorMarks>,
) -> Checks<'a> {
    Checks {
        uid: UID,
        procs: tree,
        matcher,
        daemon: Some(DAEMON),
        marks,
        terminal_proof: true,
        orphans_marked: true,
    }
}

fn decide_with(
    scene: &Scene,
    marks: Option<&ExecutorMarks>,
    declared: ClientKind,
    exe: &str,
    identity: PathIdentity,
) -> DesktopDecision {
    let matcher = AgentMatcher::default();
    decide(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, marks),
        marks,
        declared,
        Some(Path::new(exe)),
        identity,
    )
}

fn decide_scene(scene: &Scene, identity: PathIdentity) -> DesktopDecision {
    decide_with(scene, None, ClientKind::Cli, APP, identity)
}

/// What `check_reserved` (controls 1 to 3) says about the scene's process.
fn passes_controls(scene: &Scene) -> bool {
    let matcher = AgentMatcher::default();
    check_reserved(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, None),
    )
    .refused
    .is_none()
}

fn assert_decision(
    got: DesktopDecision,
    profile: ConnectionProfile,
    reason: DesktopReason,
    why: &str,
) {
    assert_eq!(
        (got.profile, got.reason),
        (profile, reason),
        "{why} (decision: {got:?})"
    );
}

#[test]
fn agent_in_ancestry_gets_mcp() {
    let scene = under_agent();
    let matcher = AgentMatcher::default();
    let seen = resolve(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, None),
        None,
    )
    .unwrap();
    assert!(
        seen.who.is_agent(),
        "fixture: claude must be in the ancestry"
    );
    assert_decision(
        decide_scene(&scene, PathIdentity::Absent),
        ConnectionProfile::Mcp,
        DesktopReason::AgentInAncestry,
        "claude → zsh → app: row 1 gives mcp with agent-in-ancestry",
    );
}

#[test]
fn agent_behind_multiplexer_gets_mcp() {
    let scene = under_multiplexer(1);
    let matcher = AgentMatcher::default();
    let seen = resolve(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, None),
        None,
    )
    .unwrap();
    assert!(
        seen.who.is_agent(),
        "fixture: the one agent of the multiplexer must be the requester"
    );
    for identity in [PathIdentity::Absent, PathIdentity::Verified] {
        assert_decision(
            decide_scene(&scene, identity),
            ConnectionProfile::Mcp,
            DesktopReason::AgentInAncestry,
            "tmux with a single agent: row 1 gives mcp with agent-in-ancestry",
        );
    }
}

#[test]
fn clean_ancestry_with_path_identity_gets_full() {
    let scene = finder();
    let decision = decide_scene(&scene, PathIdentity::Verified);
    assert_decision(
        decision,
        ConnectionProfile::Full,
        DesktopReason::PathIdentity,
        "launchd parent + verified path identity: row 3 gives full with path-identity",
    );
    assert_eq!(
        decision.identity,
        DesktopIdentity::Path,
        "a verified path identity is reported as `path`"
    );
}

#[test]
fn clean_ancestry_without_identity_at_a_terminal_gets_full() {
    let scene = terminal();
    assert!(
        passes_controls(&scene),
        "fixture: the terminal chain must pass controls 1 to 3"
    );
    let decision = decide_scene(&scene, PathIdentity::Absent);
    assert_decision(
        decision,
        ConnectionProfile::Full,
        DesktopReason::NoIdentity,
        "clean ancestry, no identity, controls 1 to 3 pass: row 4 gives full with no-identity",
    );
    assert_eq!(
        decision.identity,
        DesktopIdentity::None,
        "an absent path identity is reported as `none`"
    );
}

#[test]
fn clean_ancestry_without_identity_or_terminal_gets_mcp() {
    let scene = finder();
    assert!(
        !passes_controls(&scene),
        "fixture: an app opened from the Finder has no terminal"
    );
    let decision = decide_scene(&scene, PathIdentity::Absent);
    assert_decision(
        decision,
        ConnectionProfile::Mcp,
        DesktopReason::NoIdentity,
        "clean ancestry, no identity, no terminal: row 4 gives mcp with no-identity",
    );
    assert_eq!(decision.identity, DesktopIdentity::None);
}

#[test]
fn unverifiable_identity_gets_mcp() {
    let decision = decide_scene(&finder(), PathIdentity::Unverifiable);
    assert_decision(
        decision,
        ConnectionProfile::Mcp,
        DesktopReason::IdentityUnverifiable,
        "an identity that cannot be verified gives mcp with identity-unverifiable",
    );
    assert_eq!(
        decision.identity,
        DesktopIdentity::Unverifiable,
        "the identity is reported as `unverifiable`"
    );
}

#[test]
fn unverified_requester_gets_mcp() {
    let scene = terminal();
    let matcher = AgentMatcher::default();
    // The peer started at 299; the process with that pid started at 300: another process.
    let decision = decide(
        peer(scene.pid, scene.start - 1),
        &checks(&scene.tree, &matcher, None),
        None,
        ClientKind::Cli,
        Some(Path::new(APP)),
        PathIdentity::Verified,
    );
    assert_decision(
        decision,
        ConnectionProfile::Mcp,
        DesktopReason::RequesterUnverified,
        "a peer that is no longer the process that connected gives requester-unverified",
    );
    // Control: the same chain with the right start is verified and decided by the later rules.
    let verified = decide_scene(&scene, PathIdentity::Verified);
    assert_ne!(
        verified.reason,
        DesktopReason::RequesterUnverified,
        "control: the peer that is still the process that connected must be resolved ({verified:?})"
    );
}

#[test]
fn ambiguous_ancestry_gets_mcp_even_with_path_identity() {
    let scene = under_interpreter();
    let matcher = AgentMatcher::default();
    let seen = resolve(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, None),
        None,
    )
    .expect("fixture: with terminal proof an interpreter chain resolves");
    assert_eq!(
        seen.who,
        Who::unattributed(),
        "fixture: the interpreter chain has no agent, so only the doubt can decide"
    );
    assert_decision(
        decide_scene(&scene, PathIdentity::Verified),
        ConnectionProfile::Mcp,
        DesktopReason::AmbiguousAncestry,
        "a node in the chain gives ambiguous-ancestry (not requester-unverified, not full) even \
         with a verified path identity",
    );
}

#[test]
fn multiplexer_without_agent_gets_mcp_even_with_path_identity() {
    let scene = under_multiplexer(0);
    assert_decision(
        decide_scene(&scene, PathIdentity::Verified),
        ConnectionProfile::Mcp,
        DesktopReason::AmbiguousAncestry,
        "a terminal multiplexer without an agent gives ambiguous-ancestry even with a verified \
         path identity",
    );
}

#[test]
fn broken_ancestry_gets_mcp() {
    let scene = broken();
    let matcher = AgentMatcher::default();
    let seen = resolve(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, None),
        None,
    )
    .expect("fixture: with terminal proof a broken chain still resolves");
    assert!(
        !seen.confirmable,
        "fixture: a parent younger than its child must break the chain"
    );
    assert_decision(
        decide_scene(&scene, PathIdentity::Verified),
        ConnectionProfile::Mcp,
        DesktopReason::AmbiguousAncestry,
        "a parent younger than its child (reused pid) gives ambiguous-ancestry",
    );
}

#[test]
fn daemon_descendant_gets_mcp() {
    assert_decision(
        decide_scene(&under_daemon(), PathIdentity::Verified),
        ConnectionProfile::Mcp,
        DesktopReason::DaemonDescendant,
        "the daemon in the chain gives daemon-descendant",
    );
}

#[test]
fn executor_mark_gets_mcp_as_daemon_descendant() {
    let scene = terminal();
    let marks = Arc::new(ExecutorMarks::default());
    let _guard = marks.open("op-ts-grp-009", &Who::unattributed(), 1, None);
    marks.add_child(
        "op-ts-grp-009",
        scene.pid,
        scene.start,
        scene.pid,
        Arc::from(Vec::new()),
    );
    assert_decision(
        decide_with(
            &scene,
            Some(&marks),
            ClientKind::Cli,
            APP,
            PathIdentity::Verified,
        ),
        ConnectionProfile::Mcp,
        DesktopReason::DaemonDescendant,
        "a peer marked by a running operation gives daemon-descendant",
    );
}

#[test]
fn declared_mcp_gets_mcp_even_with_path_identity() {
    assert_decision(
        decide_with(
            &finder(),
            None,
            ClientKind::Mcp,
            APP,
            PathIdentity::Verified,
        ),
        ConnectionProfile::Mcp,
        DesktopReason::DeclaredMcp,
        "a client that declares mcp gives declared-mcp even with a verified path identity",
    );
    assert_decision(
        decide_with(
            &finder(),
            None,
            ClientKind::Cli,
            "/usr/local/bin/raptor-mcp",
            PathIdentity::Verified,
        ),
        ConnectionProfile::Mcp,
        DesktopReason::DeclaredMcp,
        "a raptor-mcp executable gives declared-mcp whatever it declares",
    );
}

#[test]
fn identity_is_reported_even_when_an_earlier_rule_decides() {
    let cases = [
        (under_agent(), PathIdentity::Verified, DesktopIdentity::Path),
        (under_agent(), PathIdentity::Absent, DesktopIdentity::None),
        (
            under_interpreter(),
            PathIdentity::Verified,
            DesktopIdentity::Path,
        ),
        (
            under_daemon(),
            PathIdentity::Unverifiable,
            DesktopIdentity::Unverifiable,
        ),
    ];
    for (scene, identity, want) in cases {
        let decision = decide_scene(&scene, identity);
        assert_eq!(
            decision.profile,
            ConnectionProfile::Mcp,
            "fixture: an earlier rule must decide mcp ({decision:?})"
        );
        assert_eq!(
            decision.identity, want,
            "the identity {identity:?} must be reported as {want:?} even if an earlier rule \
             decides ({decision:?})"
        );
    }
}

/// I-01: a peer that is not at the app's path never gets path-identity, and gets `full` only
/// where the rule of a client without a terminal gives it (the controls 1 to 3 pass).
#[test]
fn impostor_never_gets_more_than_cli() {
    let scenes = [
        ("terminal", terminal()),
        ("finder", finder()),
        ("agent", under_agent()),
        ("interpreter", under_interpreter()),
        ("multiplexer", under_multiplexer(0)),
        ("multiplexer with agent", under_multiplexer(1)),
        ("broken", broken()),
        ("daemon descendant", under_daemon()),
    ];
    for (name, scene) in &scenes {
        let decision = decide_scene(scene, PathIdentity::Absent);
        assert_ne!(
            decision.reason,
            DesktopReason::PathIdentity,
            "{name}: an impostor must never get path-identity ({decision:?})"
        );
        if decision.profile == ConnectionProfile::Full {
            assert!(
                passes_controls(scene),
                "{name}: full only if controls 1 to 3 pass ({decision:?})"
            );
        }
    }
    // The rule of a client without a terminal, exactly: the terminal is `full`, the Finder is not.
    assert_decision(
        decide_scene(&terminal(), PathIdentity::Absent),
        ConnectionProfile::Full,
        DesktopReason::NoIdentity,
        "an impostor at a terminal gets exactly what a cli there gets: full (no-identity)",
    );
    assert_decision(
        decide_scene(&finder(), PathIdentity::Absent),
        ConnectionProfile::Mcp,
        DesktopReason::NoIdentity,
        "an impostor without a terminal gets mcp (no-identity)",
    );
}

#[test]
fn declared_kind_does_not_change_the_decision() {
    let cases = [
        (
            terminal(),
            PathIdentity::Absent,
            ConnectionProfile::Full,
            DesktopReason::NoIdentity,
        ),
        (
            finder(),
            PathIdentity::Verified,
            ConnectionProfile::Full,
            DesktopReason::PathIdentity,
        ),
        (
            under_agent(),
            PathIdentity::Verified,
            ConnectionProfile::Mcp,
            DesktopReason::AgentInAncestry,
        ),
    ];
    for (scene, identity, profile, reason) in cases {
        let as_cli = decide_with(&scene, None, ClientKind::Cli, APP, identity);
        let as_other = decide_with(&scene, None, ClientKind::Other, APP, identity);
        assert_eq!(
            as_cli, as_other,
            "declaring cli or other must not change the decision"
        );
        assert_decision(
            as_cli,
            profile,
            reason,
            "the decision of the class comes from the chain and the identity, not the declared kind",
        );
    }
}
