//! TS-GRP-010 (DS-TS-GRP-010 § 9.4, file 1): the profile the daemon fixes for any connection
//! outside the desktop class, by its requester and not by its client (S-01, D1). `decide` is
//! driven with a process-table double (`Tree`, the pattern of `ts_grp_009_desktop_profile.rs`)
//! that fabricates the ancestries.
//!
//! The `Checks` of every test have `terminal_proof` and `orphans_marked` (the Unix values):
//! without them a doubtful chain would not resolve and a test of "ambiguous ancestry" would pass
//! through the wrong rule (`requester-unverified`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitraptor_api::messages::{ClientKind, ConnectionProfile};
use gitraptor_api::methods;
use gitraptor_core::channel::AgentMatcher;
use gitraptor_core::channel::authz::{AcceptedPeer, Checks, check_reserved};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::{ProcError, ProcInfo, ProcSource};
use gitraptor_core::channel::requester::{Who, resolve};
use gitraptor_core::channel::requester_profile::{
    Ancestry, ProfileDecision, ProfileReason, ancestry, decide,
};

const UID: u32 = 501;
const DAEMON: (u32, u64) = (70, 700);
const CLAUDE: &str = "/opt/homebrew/Caskroom/claude-code@latest/2.1.284/claude";
const RAPTOR: &str = "/usr/local/bin/raptor";
const RAPTOR_MCP: &str = "/usr/local/bin/raptor-mcp";

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

/// A process table and the process of the client in it.
struct Scene {
    tree: Tree,
    pid: u32,
    start: u64,
}

/// The login chain every terminal scene hangs from: login (root, leader 10) → zsh (20).
fn base() -> Tree {
    let mut tree = Tree::default();
    tree.denied.insert(10, true);
    tree.add(20, 10, "/bin/zsh", 200, true, 10);
    tree
}

/// The developer at Terminal.app: login → zsh → raptor (30).
fn terminal() -> Scene {
    let mut tree = base();
    tree.add(30, 20, RAPTOR, 300, true, 10);
    Scene {
        tree,
        pid: 30,
        start: 300,
    }
}

/// `claude` → zsh → raptor.
fn under_agent() -> Scene {
    let mut tree = base();
    tree.add(25, 20, CLAUDE, 250, true, 10);
    tree.add(40, 25, "/bin/zsh", 400, false, 40);
    tree.add(41, 40, RAPTOR, 410, false, 40);
    Scene {
        tree,
        pid: 41,
        start: 410,
    }
}

/// Claude Code installed with npm runs as `node`: node → zsh → raptor, with a terminal.
fn under_interpreter() -> Scene {
    let mut tree = base();
    tree.add(26, 20, "/usr/local/bin/node", 260, true, 10);
    tree.add(27, 26, "/bin/zsh", 270, true, 10);
    tree.add(32, 27, RAPTOR, 320, true, 10);
    Scene {
        tree,
        pid: 32,
        start: 320,
    }
}

/// A tmux server under launchd with raptor's pane; with `agents` Claude Code sessions in other
/// panes of it.
fn under_multiplexer(agents: usize) -> Scene {
    let mut tree = Tree::default();
    tree.add(1, 0, "/sbin/launchd", 1, false, 1);
    tree.add(80, 1, "/opt/homebrew/bin/tmux", 800, false, 80);
    tree.add(90, 80, "/bin/zsh", 900, true, 90);
    tree.add(91, 90, RAPTOR, 910, true, 90);
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

/// Detached (`setsid`, double fork): its parent is launchd (root's), no terminal.
fn detached() -> Scene {
    let mut tree = Tree::default();
    tree.denied.insert(1, true);
    tree.add(30, 1, RAPTOR, 300, false, 30);
    Scene {
        tree,
        pid: 30,
        start: 300,
    }
}

/// raptor is a child of the daemon (`DAEMON`) and nothing marks it.
fn under_daemon() -> Scene {
    let mut tree = base();
    tree.add(70, 20, RAPTOR, 700, false, 70);
    tree.add(71, 70, RAPTOR, 710, false, 70);
    Scene {
        tree,
        pid: 71,
        start: 710,
    }
}

/// The client is the daemon's own process (an in-process client).
fn daemon_itself() -> Scene {
    let mut tree = base();
    tree.add(70, 20, RAPTOR, 700, true, 10);
    Scene {
        tree,
        pid: DAEMON.0,
        start: DAEMON.1,
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

/// Every knob of `decide` for one call.
struct Call<'a> {
    marks: Option<&'a ExecutorMarks>,
    declared: ClientKind,
    exe: &'a str,
    installed: bool,
    children_as_self: bool,
}

impl Default for Call<'_> {
    fn default() -> Self {
        Self {
            marks: None,
            declared: ClientKind::Cli,
            exe: RAPTOR,
            installed: false,
            children_as_self: false,
        }
    }
}

fn decide_call(scene: &Scene, call: Call<'_>) -> ProfileDecision {
    let matcher = AgentMatcher::default();
    decide(
        peer(scene.pid, scene.start),
        &checks(&scene.tree, &matcher, call.marks),
        call.marks,
        call.declared,
        Some(Path::new(call.exe)),
        call.installed,
        call.children_as_self,
    )
}

fn decide_scene(scene: &Scene) -> ProfileDecision {
    decide_call(scene, Call::default())
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
    got: ProfileDecision,
    profile: ConnectionProfile,
    reason: ProfileReason,
    why: &str,
) {
    assert_eq!(
        (got.profile, got.reason),
        (profile, reason),
        "{why} (decision: {got:?})"
    );
}

/// A running operation of the daemon whose step started the scene's process.
fn marked(scene: &Scene) -> Arc<ExecutorMarks> {
    let marks = Arc::new(ExecutorMarks::default());
    std::mem::forget(marks.open("op-ts-grp-010", &Who::unattributed(), 1, None));
    marks.add_child(
        "op-ts-grp-010",
        scene.pid,
        scene.start,
        scene.pid,
        Arc::from(Vec::new()),
    );
    marks
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
    assert_eq!(
        ancestry(
            peer(scene.pid, scene.start),
            &checks(&scene.tree, &matcher, None),
            None
        ),
        Ancestry::Agent,
        "the shared walk must report the agent"
    );
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Mcp,
        ProfileReason::AgentInAncestry,
        "claude → zsh → raptor declared cli: mcp with agent-in-ancestry",
    );
}

#[test]
fn agent_behind_multiplexer_gets_mcp() {
    let scene = under_multiplexer(1);
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Mcp,
        ProfileReason::AgentInAncestry,
        "tmux with a single agent under it: the caller is that agent, mcp",
    );
}

#[test]
fn npm_agent_as_interpreter_gets_mcp() {
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
        "fixture: node is not taken for an agent by name, only the doubt can decide"
    );
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Mcp,
        ProfileReason::AmbiguousAncestry,
        "a node in the chain (npm Claude Code) gives ambiguous-ancestry, not full",
    );
}

#[test]
fn developer_at_a_terminal_keeps_full() {
    let scene = terminal();
    assert!(
        passes_controls(&scene),
        "fixture: the terminal chain must pass controls 1 to 3"
    );
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Full,
        ProfileReason::Developer,
        "the developer at a clean terminal keeps the full profile",
    );
}

#[test]
fn multiplexer_with_a_live_agent_elsewhere_gets_mcp() {
    // The same tmux scene without any agent: the developer in tmux keeps full.
    let alone = under_multiplexer(0);
    assert!(
        passes_controls(&alone),
        "fixture: the tmux pane must pass controls 1 to 3"
    );
    assert_decision(
        decide_scene(&alone),
        ConnectionProfile::Full,
        ProfileReason::Developer,
        "tmux and no live agent anywhere: full",
    );
    // A live claude outside the server (another terminal): the pane cannot confirm.
    let mut scene = under_multiplexer(0);
    scene.tree.denied.insert(10, true);
    scene.tree.add(20, 10, "/bin/zsh", 200, true, 10);
    scene.tree.add(25, 20, CLAUDE, 250, true, 10);
    assert!(
        passes_controls(&scene),
        "fixture: controls 1 to 3 do not see an agent outside the chain"
    );
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Mcp,
        ProfileReason::AmbiguousAncestry,
        "tmux with a live agent elsewhere: not confirmable, mcp (controls 1 to 3 are not enough)",
    );
}

#[test]
fn detached_without_terminal_gets_mcp() {
    let scene = detached();
    assert!(
        !passes_controls(&scene),
        "fixture: a detached process has no terminal"
    );
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Mcp,
        ProfileReason::Unconfirmable,
        "a clean but detached chain without a terminal gives unconfirmable",
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
        Some(Path::new(RAPTOR)),
        false,
        false,
    );
    assert_decision(
        decision,
        ConnectionProfile::Mcp,
        ProfileReason::RequesterUnverified,
        "a peer that is no longer the process that connected gives requester-unverified",
    );
    assert_eq!(
        ancestry(
            peer(scene.pid, scene.start - 1),
            &checks(&scene.tree, &matcher, None),
            None
        ),
        Ancestry::Unverified
    );
}

#[test]
fn daemon_descendants_get_mcp() {
    let scene = under_daemon();
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Mcp,
        ProfileReason::DaemonDescendant,
        "an unmarked child of the daemon gives daemon-descendant",
    );
    let marks = marked(&terminal());
    assert_decision(
        decide_call(
            &terminal(),
            Call {
                marks: Some(&marks),
                ..Call::default()
            },
        ),
        ConnectionProfile::Mcp,
        ProfileReason::DaemonDescendant,
        "a peer marked by a running operation gives daemon-descendant",
    );
    // The debug-only test knob (D3): unmarked children count as the daemon itself.
    assert_decision(
        decide_call(
            &scene,
            Call {
                children_as_self: true,
                ..Call::default()
            },
        ),
        ConnectionProfile::Full,
        ProfileReason::DaemonSelf,
        "with the test knob an unmarked child of the daemon is the daemon itself",
    );
    assert_decision(
        decide_call(
            &terminal(),
            Call {
                marks: Some(&marks),
                children_as_self: true,
                ..Call::default()
            },
        ),
        ConnectionProfile::Mcp,
        ProfileReason::DaemonDescendant,
        "the test knob never widens a marked child",
    );
}

#[test]
fn the_daemon_itself_keeps_todays_rule() {
    let scene = daemon_itself();
    assert_decision(
        decide_scene(&scene),
        ConnectionProfile::Full,
        ProfileReason::DaemonSelf,
        "the daemon's own process is not a requester: full",
    );
    assert_decision(
        decide_call(
            &scene,
            Call {
                declared: ClientKind::Mcp,
                ..Call::default()
            },
        ),
        ConnectionProfile::Mcp,
        ProfileReason::DeclaredMcp,
        "the daemon's own process declaring mcp still gets mcp",
    );
}

#[test]
fn hook_client_keeps_full_under_an_agent() {
    let scene = under_agent();
    assert_decision(
        decide_call(
            &scene,
            Call {
                declared: ClientKind::Other,
                installed: true,
                ..Call::default()
            },
        ),
        ConnectionProfile::Full,
        ProfileReason::HookClient,
        "raptor hook (other + installed binary) under the agent's git keeps full",
    );
    assert_decision(
        decide_call(
            &scene,
            Call {
                declared: ClientKind::Other,
                installed: false,
                ..Call::default()
            },
        ),
        ConnectionProfile::Mcp,
        ProfileReason::AgentInAncestry,
        "other from a binary that is not the installed one is not the hook client",
    );
    assert_decision(
        decide_call(
            &scene,
            Call {
                declared: ClientKind::Cli,
                installed: true,
                ..Call::default()
            },
        ),
        ConnectionProfile::Mcp,
        ProfileReason::AgentInAncestry,
        "the installed binary declaring cli is the CLI, not the hook client",
    );
}

#[test]
fn declared_kind_never_widens() {
    let scenes = [
        ("terminal", terminal()),
        ("agent", under_agent()),
        ("interpreter", under_interpreter()),
        ("multiplexer", under_multiplexer(0)),
        ("multiplexer with agent", under_multiplexer(1)),
        ("detached", detached()),
        ("daemon descendant", under_daemon()),
        ("daemon itself", daemon_itself()),
    ];
    for (name, scene) in &scenes {
        let cli = decide_call(scene, Call::default());
        let other = decide_call(
            scene,
            Call {
                declared: ClientKind::Other,
                ..Call::default()
            },
        );
        assert_eq!(
            cli, other,
            "{name}: cli and other give the same decision without the installed binary"
        );
        for call in [
            Call {
                declared: ClientKind::Mcp,
                ..Call::default()
            },
            Call {
                exe: RAPTOR_MCP,
                ..Call::default()
            },
            Call {
                exe: RAPTOR_MCP,
                declared: ClientKind::Other,
                installed: true,
                ..Call::default()
            },
        ] {
            assert_decision(
                decide_call(scene, call),
                ConnectionProfile::Mcp,
                ProfileReason::DeclaredMcp,
                &format!("{name}: declaring mcp or being raptor-mcp always gives mcp"),
            );
        }
    }
}

#[test]
fn daemon_replace_stays_open_to_mcp() {
    let spec = methods::spec(methods::DAEMON_REPLACE).expect("daemon.replace is declared");
    assert!(
        spec.mcp,
        "daemon.replace must stay in the mcp allowlist (ADR-MCP-001 § 2, D5)"
    );
    assert!(!spec.reserved, "daemon.replace is not a reserved command");
    // A narrowed connection is mcp by the requester rule, not by declaration.
    let narrowed = ProfileDecision {
        profile: ConnectionProfile::Mcp,
        reason: ProfileReason::AgentInAncestry,
    };
    assert!(narrowed.narrowed());
    let declared = ProfileDecision {
        profile: ConnectionProfile::Mcp,
        reason: ProfileReason::DeclaredMcp,
    };
    assert!(!declared.narrowed());
}
