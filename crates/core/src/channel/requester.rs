//! Who asks for an operation, resolved in the daemon (ADR-TMC-005 § 1,
//! SEC-TMC-03, D-TMC-23).
//!
//! Only what the kernel says about the socket peer counts. The result is
//! "agent X" with its origin or "unattributed"; never "human". Rules, in
//! order:
//!
//! 1. The caller must still be the process that connected.
//! 2. Walking up from the caller, an ancestor counts only if it started
//!    before its child (a younger parent is a reused pid and breaks the
//!    walk). The walk stops at the daemon: whatever launched the daemon
//!    (`raptor-mcp` under an agent) says nothing about the caller.
//! 3. A process marked by a running operation acts for that operation's
//!    requester (DEP-MCP-3).
//! 4. The first agent process found is the requester.
//! 5. A terminal multiplexer server in the chain is shared: if exactly one
//!    live agent of the user descends from it, the caller is that agent;
//!    with any live agent around, the caller cannot confirm.
//! 6. Otherwise, unattributed.
//!
//! Where nothing proves the caller is the developer at a terminal (Windows,
//! TQ-14), "unattributed" is the most a caller can be, and it may still undo
//! unattributed work without a confirmation. So there it also has to be
//! verified: an ancestry that breaks (a gone or reused parent), crosses an
//! interpreter, reaches the daemon unmarked or meets a multiplexer with a
//! live agent is refused as unverified instead (security review of
//! 2026-10-05, C-01).

use gitraptor_api::messages::RefusalReason;
use gitraptor_api::timemachine::ResolvedVia;
use gitraptor_api::{Actor, AgentKind, AgentOrigin};

use super::authz::{AcceptedPeer, Checks, ExeClass, check_reserved};
use super::peer::{ProcError, ProcInfo};
use crate::timemachine::oplog::{Requester, RequesterOrigin};

/// Longest ancestry walked.
const MAX_DEPTH: usize = 64;

/// File names of terminal multiplexer servers.
const MULTIPLEXERS: &[&str] = &["tmux", "screen", "zellij", "abduco", "dtach"];

/// A requester in both shapes: the contract's actor and the oplog's frozen
/// record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub actor: Actor,
    pub requester: Requester,
}

impl Who {
    pub fn unattributed() -> Self {
        Self {
            actor: Actor::Unattributed,
            requester: Requester::Unattributed,
        }
    }

    /// A detected Claude Code session, identified by its process.
    fn claude(agent: &ProcInfo) -> Self {
        Self {
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            requester: Requester::Agent {
                name: "claude-code".into(),
                origin: RequesterOrigin::Detected,
                session_id: format!("{}:{}", agent.pid, agent.start_us),
            },
        }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self.actor, Actor::Agent { .. })
    }
}

/// The daemon's answer about a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub who: Who,
    pub via: ResolvedVia,
    /// Operation whose step started the caller (DEP-MCP-3).
    pub executor_operation: Option<String>,
    /// Whether a confirmation challenge may be issued (ADR-TMC-005 § 3).
    pub confirmable: bool,
}

/// The caller is no longer the process that connected, or cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unverified;

fn file_name(info: &ProcInfo) -> String {
    info.exe
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_multiplexer(info: &ProcInfo) -> bool {
    let name = file_name(info);
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    MULTIPLEXERS.contains(&name)
}

fn class(info: &ProcInfo, checks: &Checks<'_>) -> ExeClass {
    info.exe
        .as_deref()
        .map_or(ExeClass::Other, |exe| checks.matcher.classify(exe))
}

/// Reads the parent of `current` under the walk rules: `None` ends the walk
/// (root, another user's process, unreadable or a reused pid).
fn parent(current: &ProcInfo, checks: &Checks<'_>) -> Option<ProcInfo> {
    if current.pid <= 1 || current.ppid == 0 {
        return None;
    }
    match checks.procs.read(current.ppid) {
        Ok(p) if p.uid == checks.uid && p.start_us <= current.start_us => Some(p),
        _ => None,
    }
}

/// Whether the walk from `info` ended cleanly (root or another user), as
/// opposed to a reused pid or an unreadable process of this user.
fn ended_cleanly(current: &ProcInfo, checks: &Checks<'_>) -> bool {
    if current.pid <= 1 || current.ppid == 0 || checks.procs.is_session_root(current) {
        return true;
    }
    match checks.procs.read(current.ppid) {
        Ok(p) => p.uid != checks.uid,
        Err(ProcError::Denied) => checks.procs.foreign_to(current.ppid, checks.uid) == Some(true),
        Err(_) => false,
    }
}

/// Live Claude Code processes of the user that descend from `server`.
/// `None` when the process list cannot be read.
struct Presence {
    any_agent: bool,
    under_server: Vec<ProcInfo>,
}

fn presence(server: &ProcInfo, checks: &Checks<'_>) -> Option<Presence> {
    let pids = checks.procs.pids_of(checks.uid)?;
    let mut out = Presence {
        any_agent: false,
        under_server: Vec::new(),
    };
    for pid in pids {
        let Ok(info) = checks.procs.read(pid) else {
            continue;
        };
        if class(&info, checks) != ExeClass::ClaudeCode {
            continue;
        }
        out.any_agent = true;
        let mut current = info.clone();
        for _ in 0..MAX_DEPTH {
            if current.pid == server.pid && current.start_us == server.start_us {
                out.under_server.push(info.clone());
                break;
            }
            match parent(&current, checks) {
                Some(p) => current = p,
                None => break,
            }
        }
    }
    Some(out)
}

/// Resolves the requester of the peer of a connection, now.
pub fn resolve(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&super::marks::ExecutorMarks>,
) -> Result<Resolution, Unverified> {
    let caller = checks.procs.read(peer.pid).map_err(|_| Unverified)?;
    if caller.start_us != peer.start_us
        || caller.start_us > peer.accepted_us
        || caller.uid != checks.uid
    {
        return Err(Unverified);
    }
    let unattributed = |via, confirmable| Resolution {
        who: Who::unattributed(),
        via,
        executor_operation: None,
        confirmable,
    };
    // An unattributed caller the ancestry could not vouch for: where no
    // terminal proof exists, refused instead (C-01).
    let verified = |vouched: bool, r: Resolution| {
        if vouched || checks.terminal_proof {
            Ok(r)
        } else {
            Err(Unverified)
        }
    };

    let mut current = caller.clone();
    let mut server: Option<ProcInfo> = None;
    let mut interpreter = false;
    let mut clean = false;
    for _ in 0..MAX_DEPTH {
        if let Some(marked) = marks.and_then(|m| m.lookup(&current)) {
            return Ok(Resolution {
                who: marked.who,
                via: ResolvedVia::Executor,
                executor_operation: Some(marked.operation_id),
                confirmable: false,
            });
        }
        // A descendant of the daemon without a mark: nothing above the
        // daemon speaks for it. If a child is being launched, the caller may
        // be its hook before the mark: wait for the registration (I-02) and
        // resolve again; past the wait, unverified (fail-closed).
        if checks.daemon == Some((current.pid, current.start_us)) {
            if let Some(m) = marks
                && m.spawn_pending()
            {
                if !m.wait_registered(super::marks::REGISTRATION_WAIT) {
                    return Err(Unverified);
                }
                return resolve_after_barrier(peer, checks, m);
            }
            return verified(false, unattributed(ResolvedVia::None, false));
        }
        match class(&current, checks) {
            ExeClass::ClaudeCode => {
                return Ok(Resolution {
                    who: Who::claude(&current),
                    via: ResolvedVia::Ancestry,
                    executor_operation: None,
                    confirmable: false,
                });
            }
            // May host an agent: never attributed by name, never confirms.
            ExeClass::Interpreter => interpreter = true,
            ExeClass::Other => {}
        }
        if server.is_none() && is_multiplexer(&current) {
            server = Some(current.clone());
        }
        match parent(&current, checks) {
            Some(p) => current = p,
            None => {
                clean = ended_cleanly(&current, checks);
                break;
            }
        }
    }

    let mut confirmable = clean && !interpreter;
    if let Some(server) = server {
        match presence(&server, checks) {
            None => return verified(false, unattributed(ResolvedVia::Multiplexer, false)),
            Some(p) if p.under_server.len() == 1 => {
                return Ok(Resolution {
                    who: Who::claude(&p.under_server[0]),
                    via: ResolvedVia::Multiplexer,
                    executor_operation: None,
                    confirmable: false,
                });
            }
            Some(p) if p.under_server.len() > 1 => {
                return verified(false, unattributed(ResolvedVia::Multiplexer, false));
            }
            Some(p) => confirmable &= !p.any_agent,
        }
    }
    let resolution = |confirmable| unattributed(ResolvedVia::None, confirmable);
    if !confirmable {
        return verified(false, resolution(false));
    }
    // The rest of the reserved checks: terminal, session leader, identity.
    // Without a terminal proof they refuse as `Unsupported` (TQ-14).
    confirmable &= check_reserved(peer, checks).refused.is_none();
    Ok(resolution(confirmable))
}

/// Second walk, once the pending registration completed: a mark found now
/// wins; otherwise the caller stays an unmarked descendant of the daemon,
/// unverified where no terminal proof exists (C-01).
fn resolve_after_barrier(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: &super::marks::ExecutorMarks,
) -> Result<Resolution, Unverified> {
    let mut current = checks.procs.read(peer.pid).map_err(|_| Unverified)?;
    for _ in 0..MAX_DEPTH {
        if let Some(marked) = marks.lookup(&current) {
            return Ok(Resolution {
                who: marked.who,
                via: ResolvedVia::Executor,
                executor_operation: Some(marked.operation_id),
                confirmable: false,
            });
        }
        if checks.daemon == Some((current.pid, current.start_us)) {
            break;
        }
        match parent(&current, checks) {
            Some(p) => current = p,
            None => break,
        }
    }
    if !checks.terminal_proof {
        return Err(Unverified);
    }
    Ok(Resolution {
        who: Who::unattributed(),
        via: ResolvedVia::None,
        executor_operation: None,
        confirmable: false,
    })
}

/// Why a caller may not confirm, for the challenge (ADR-TMC-005 § 3).
pub fn confirmation_refusal(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&super::marks::ExecutorMarks>,
) -> Option<RefusalReason> {
    let reserved = check_reserved(peer, checks);
    if let Some(reason) = reserved.refused {
        return Some(reason);
    }
    match resolve(peer, checks, marks) {
        Err(Unverified) => Some(RefusalReason::IdentityUnverified),
        Ok(r) if r.via == ResolvedVia::Executor => Some(RefusalReason::DaemonDescendant),
        Ok(r) if r.who.is_agent() => Some(RefusalReason::AgentAncestry),
        Ok(r) if !r.confirmable => Some(RefusalReason::AgentAncestry),
        Ok(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::AgentMatcher;
    use crate::channel::marks::ExecutorMarks;
    use crate::channel::peer::ProcSource;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    const UID: u32 = 501;
    const DAEMON: (u32, u64) = (70, 700);
    const CLAUDE: &str = "/opt/homebrew/Caskroom/claude-code@latest/2.1.284/claude";

    #[derive(Default)]
    struct Tree {
        procs: HashMap<u32, ProcInfo>,
        denied: HashMap<u32, bool>,
        no_list: bool,
        /// Desktop roots (Windows `explorer.exe`).
        roots: Vec<u32>,
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
            (!self.no_list).then(|| self.procs.keys().copied().collect())
        }
        fn is_session_root(&self, info: &ProcInfo) -> bool {
            self.roots.contains(&info.pid)
        }
    }

    /// Terminal.app: launchd → login (root, leader 10) → zsh(20) → raptor(30).
    fn terminal() -> Tree {
        let mut t = Tree::default();
        t.denied.insert(10, true);
        t.add(20, 10, "/bin/zsh", 200, true, 10);
        t.add(30, 20, "/usr/local/bin/raptor", 300, true, 10);
        t
    }

    fn peer(pid: u32, start: u64) -> AcceptedPeer {
        AcceptedPeer {
            pid,
            start_us: start,
            accepted_us: 1_000_000,
        }
    }

    fn run(t: &Tree, pid: u32, start: u64, marks: Option<&ExecutorMarks>) -> Resolution {
        resolve_on(t, pid, start, marks, true).unwrap()
    }

    /// Resolves with or without a terminal proof (Unix or Windows).
    fn resolve_on(
        t: &Tree,
        pid: u32,
        start: u64,
        marks: Option<&ExecutorMarks>,
        terminal_proof: bool,
    ) -> Result<Resolution, Unverified> {
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: UID,
            procs: t,
            matcher: &matcher,
            daemon: Some(DAEMON),
            marks,
            terminal_proof,
        };
        resolve(peer(pid, start), &checks, marks)
    }

    fn refusal(t: &Tree, pid: u32, start: u64) -> Option<RefusalReason> {
        refusal_on(t, pid, start, true)
    }

    fn refusal_on(t: &Tree, pid: u32, start: u64, terminal_proof: bool) -> Option<RefusalReason> {
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: UID,
            procs: t,
            matcher: &matcher,
            daemon: Some(DAEMON),
            marks: None,
            terminal_proof,
        };
        confirmation_refusal(peer(pid, start), &checks, None)
    }

    #[test]
    fn a_terminal_without_agents_is_unattributed_and_may_confirm() {
        let r = run(&terminal(), 30, 300, None);
        assert_eq!(r.who, Who::unattributed());
        assert_eq!(r.via, ResolvedVia::None);
        assert!(r.confirmable);
        assert_eq!(refusal(&terminal(), 30, 300), None);
    }

    /// CLI from the agent's shell, or `raptor-mcp` launched by the agent.
    #[test]
    fn a_client_under_claude_code_is_that_agent() {
        let mut t = terminal();
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(40, 25, "/bin/zsh", 400, false, 40);
        t.add(41, 40, "/usr/local/bin/raptor", 410, false, 40);
        t.add(42, 25, "/usr/local/bin/raptor-mcp", 420, false, 10);
        for (pid, start) in [(41, 410), (42, 420)] {
            let r = run(&t, pid, start, None);
            assert!(r.who.is_agent());
            assert_eq!(r.via, ResolvedVia::Ancestry);
            assert_eq!(
                r.who.requester.session_id(),
                Some("25:250"),
                "the session is the agent process"
            );
            assert!(!r.confirmable);
        }
        assert_eq!(refusal(&t, 41, 410), Some(RefusalReason::AgentAncestry));
    }

    /// `setsid` without a pty: same process tree, no terminal.
    #[test]
    fn setsid_without_a_pty_stays_the_agent_and_cannot_confirm() {
        let mut t = terminal();
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(43, 25, "/usr/local/bin/raptor", 430, false, 43);
        assert!(run(&t, 43, 430, None).who.is_agent());
        assert_eq!(refusal(&t, 43, 430), Some(RefusalReason::AgentAncestry));
        // Double fork + setsid detaches it: unattributed, but without a
        // terminal it still cannot confirm (accepted residual risk otherwise).
        t.add(1, 0, "/sbin/launchd", 1, false, 1);
        t.add(44, 1, "/usr/local/bin/raptor", 440, false, 44);
        let r = run(&t, 44, 440, None);
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
        assert_eq!(
            refusal(&t, 44, 440),
            Some(RefusalReason::NoControllingTerminal)
        );
    }

    /// `tmux new-window` + `send-keys` from an agent in another pane of the
    /// same server: the new pane's client is the agent.
    #[test]
    fn a_multiplexer_shared_with_an_agent_makes_the_client_that_agent() {
        let mut t = terminal();
        t.add(1, 0, "/sbin/launchd", 1, false, 1);
        t.add(80, 1, "/opt/homebrew/bin/tmux", 800, false, 80);
        t.add(81, 80, "/bin/zsh", 810, true, 81);
        t.add(82, 81, CLAUDE, 820, true, 81);
        t.add(90, 80, "/bin/zsh", 900, true, 90);
        t.add(91, 90, "/usr/local/bin/raptor", 910, true, 90);
        let r = run(&t, 91, 910, None);
        assert!(r.who.is_agent());
        assert_eq!(r.via, ResolvedVia::Multiplexer);
        assert_eq!(r.who.requester.session_id(), Some("82:820"));
        assert!(refusal(&t, 91, 910).is_some());
        // Two agents in the server: ambiguous, unattributed, no confirmation.
        t.add(83, 81, CLAUDE, 830, true, 81);
        let r = run(&t, 91, 910, None);
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
    }

    /// An agent outside the server that drives it as a client: no agent
    /// descends from the server, but one is alive, so no confirmation.
    #[test]
    fn a_multiplexer_with_any_live_agent_cannot_confirm() {
        let mut t = terminal();
        t.add(1, 0, "/sbin/launchd", 1, false, 1);
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(80, 1, "/opt/homebrew/bin/tmux", 800, false, 80);
        t.add(90, 80, "/bin/zsh", 900, true, 90);
        t.add(91, 90, "/usr/local/bin/raptor", 910, true, 90);
        let r = run(&t, 91, 910, None);
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
        assert_eq!(refusal(&t, 91, 910), Some(RefusalReason::AgentAncestry));
        // Without any agent alive the developer's tmux may confirm.
        t.procs.remove(&25);
        assert!(run(&t, 91, 910, None).confirmable);
        // An unreadable process list fails closed.
        t.no_list = true;
        let r = run(&t, 91, 910, None);
        assert_eq!(r.via, ResolvedVia::Multiplexer);
        assert!(!r.confirmable);
    }

    /// A parent younger than its child is a reused pid: the walk breaks and
    /// the caller cannot confirm.
    #[test]
    fn a_reused_pid_breaks_the_walk() {
        let mut t = terminal();
        t.add(20, 10, "/bin/zsh", 900, true, 10);
        let r = run(&t, 30, 300, None);
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
        // The caller itself replaced by another process with its pid.
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: UID,
            procs: &t,
            matcher: &matcher,
            daemon: Some(DAEMON),
            marks: None,
            terminal_proof: true,
        };
        assert_eq!(resolve(peer(30, 299), &checks, None), Err(Unverified));
    }

    /// The daemon was launched by `raptor-mcp` under an agent: an unmarked
    /// descendant of the daemon is not that agent's (the walk stops).
    #[test]
    fn the_walk_stops_at_the_daemon() {
        let mut t = terminal();
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(60, 25, "/usr/local/bin/raptor-mcp", 600, false, 10);
        t.add(70, 60, "/usr/local/bin/raptor", 700, false, 70);
        t.add(71, 70, "/usr/bin/git", 710, false, 70);
        let r = run(&t, 71, 710, None);
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
    }

    /// DEP-MCP-3: a child of an operation acts for its requester, even
    /// reparented away from the daemon.
    #[test]
    fn a_marked_process_acts_for_the_operation_requester() {
        let mut t = terminal();
        t.add(1, 0, "/sbin/launchd", 1, false, 1);
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(73, 1, "/repo/.git/hooks/post-checkout", 730, true, 10);
        t.add(74, 73, "/usr/local/bin/raptor", 740, true, 10);
        let marks = Arc::new(ExecutorMarks::default());
        let requester = Who::claude(&t.procs[&25]);
        let _guard = marks.open("op-9", &requester, 720);
        marks.add_child("op-9", 73, 730, 73);
        let r = run(&t, 74, 740, Some(&marks));
        assert_eq!(r.who, requester);
        assert_eq!(r.via, ResolvedVia::Executor);
        assert_eq!(r.executor_operation.as_deref(), Some("op-9"));
        assert!(!r.confirmable);
        // And the reserved checks refuse it as a descendant of the executor.
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: UID,
            procs: &t,
            matcher: &matcher,
            daemon: Some(DAEMON),
            marks: Some(&marks),
            terminal_proof: true,
        };
        assert_eq!(
            check_reserved(peer(74, 740), &checks).refused,
            Some(RefusalReason::DaemonDescendant)
        );
        assert_eq!(
            confirmation_refusal(peer(74, 740), &checks, Some(&marks)),
            Some(RefusalReason::DaemonDescendant)
        );
    }

    /// Without a terminal proof (Windows, TQ-14) a verified caller is
    /// unattributed but never confirms, and the refusal says why.
    #[test]
    fn without_a_terminal_proof_nobody_confirms() {
        let r = resolve_on(&terminal(), 30, 300, None, false).unwrap();
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
        assert_eq!(
            refusal_on(&terminal(), 30, 300, false),
            Some(RefusalReason::Unsupported)
        );
        // An agent is still that agent, and the refusal names the ancestry.
        let mut t = terminal();
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(43, 25, "/usr/local/bin/raptor", 430, false, 43);
        let r = resolve_on(&t, 43, 430, None, false).unwrap();
        assert!(r.who.is_agent());
        assert_eq!(r.via, ResolvedVia::Ancestry);
        assert_eq!(
            refusal_on(&t, 43, 430, false),
            Some(RefusalReason::AgentAncestry)
        );
    }

    /// C-01: without a terminal proof an ancestry that cannot vouch for the
    /// caller is refused, never a plain "unattributed" that could undo the
    /// developer's work.
    #[test]
    fn without_a_terminal_proof_an_unverifiable_caller_is_refused() {
        // A reused pid (a parent younger than its child).
        let mut t = terminal();
        t.add(20, 10, "/bin/zsh", 900, true, 10);
        assert_eq!(resolve_on(&t, 30, 300, None, false), Err(Unverified));
        // A gone parent: an orphan, such as a hook whose marked parent ended.
        let mut t = terminal();
        t.add(31, 29, "/usr/local/bin/raptor", 310, false, 10);
        assert_eq!(resolve_on(&t, 31, 310, None, false), Err(Unverified));
        assert!(!run(&t, 31, 310, None).confirmable, "Unix: unconfirmable");
        // An interpreter that may host an agent (npm Claude Code is `node`).
        let mut t = terminal();
        t.add(26, 20, "C:/Program Files/nodejs/node.exe", 260, true, 10);
        t.add(32, 26, "/usr/local/bin/raptor", 320, true, 10);
        assert_eq!(resolve_on(&t, 32, 320, None, false), Err(Unverified));
        // An unmarked descendant of the daemon.
        let mut t = terminal();
        t.add(70, 20, "/usr/local/bin/raptor", 700, false, 70);
        t.add(71, 70, "/usr/bin/git", 710, false, 70);
        assert_eq!(resolve_on(&t, 71, 710, None, false), Err(Unverified));
        // A multiplexer while an agent is alive.
        let mut t = terminal();
        t.add(1, 0, "/sbin/launchd", 1, false, 1);
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(80, 1, "/opt/homebrew/bin/tmux", 800, false, 80);
        t.add(90, 80, "/bin/zsh", 900, true, 90);
        t.add(91, 90, "/usr/local/bin/raptor", 910, true, 90);
        assert_eq!(resolve_on(&t, 91, 910, None, false), Err(Unverified));
    }

    /// Windows desktop: `explorer.exe`'s parent (`userinit`) has exited, so
    /// the walk ends cleanly only at the desktop root.
    #[test]
    fn the_desktop_root_ends_the_walk_cleanly() {
        let mut t = Tree::default();
        t.add(20, 8, "C:/Windows/explorer.exe", 200, false, 0);
        t.add(
            30,
            20,
            "C:/Program Files/PowerShell/7/pwsh.exe",
            300,
            false,
            0,
        );
        t.add(31, 30, "C:/Users/u/.cargo/bin/raptor.exe", 310, false, 0);
        assert_eq!(resolve_on(&t, 31, 310, None, false), Err(Unverified));
        t.roots.push(20);
        let r = resolve_on(&t, 31, 310, None, false).unwrap();
        assert_eq!(r.who, Who::unattributed());
        assert!(!r.confirmable);
        // `claude.exe` under the desktop is the agent.
        t.add(40, 30, "C:/Users/u/.local/bin/claude.exe", 400, false, 0);
        t.add(41, 40, "C:/Program Files/Git/bin/bash.exe", 410, false, 0);
        t.add(42, 41, "C:/Users/u/.cargo/bin/raptor.exe", 420, false, 0);
        let r = resolve_on(&t, 42, 420, None, false).unwrap();
        assert!(r.who.is_agent());
        assert_eq!(r.who.requester.session_id(), Some("40:400"));
    }

    /// DEP-MCP-3 without process groups: the marked child itself acts for
    /// the requester even without a terminal proof.
    #[test]
    fn without_a_terminal_proof_a_marked_child_acts_for_the_requester() {
        let mut t = terminal();
        t.add(25, 20, CLAUDE, 250, true, 10);
        t.add(73, 20, "C:/Program Files/Git/cmd/git.exe", 730, false, 0);
        let marks = Arc::new(ExecutorMarks::default());
        let requester = Who::claude(&t.procs[&25]);
        let _guard = marks.open("op-9", &requester, 720);
        marks.add_child("op-9", 73, 730, 0);
        let r = resolve_on(&t, 73, 730, Some(&marks), false).unwrap();
        assert_eq!(r.who, requester);
        assert_eq!(r.via, ResolvedVia::Executor);
        assert!(!r.confirmable);
    }

    /// Q34: no path of the resolution yields a "human".
    #[test]
    fn never_human() {
        let schema = serde_json::to_string(&schemars::schema_for!(
            gitraptor_api::timemachine::RequesterView
        ))
        .unwrap();
        assert!(!schema.to_lowercase().contains("human"));
    }
}

/// Real processes on Windows: a copy of this test binary named `claude.exe`
/// starts a helper and keeps it alive; the helper resolves to that agent.
#[cfg(all(test, windows))]
mod real_processes {
    use super::*;
    use crate::channel::AgentMatcher;
    use crate::channel::authz::TERMINAL_PROOF;
    use crate::channel::peer::{ProcSource, SystemProcs, current_uid};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    const MODE: &str = "GITRAPTOR_TEST_PROCESS_MODE";
    const ENTRY: &str = "channel::requester::real_processes::entry";

    /// Not a test: the body of the copies. `spawn` starts a `helper.exe`
    /// child, prints its pid and waits for stdin to close; `wait` only waits.
    #[test]
    fn entry() {
        match std::env::var(MODE).as_deref() {
            Ok("spawn") => {
                let helper = std::env::current_exe()
                    .unwrap()
                    .with_file_name("helper.exe");
                let mut child = Command::new(helper)
                    .args(["--exact", ENTRY, "--nocapture"])
                    .env(MODE, "wait")
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                println!("CHILD={}", child.id());
                std::io::stdout().flush().unwrap();
                let _ = std::io::stdin().read_to_end(&mut Vec::new());
                let _ = child.wait();
            }
            Ok("wait") => {
                let _ = std::io::stdin().read_to_end(&mut Vec::new());
            }
            _ => {}
        }
    }

    /// Copies of this binary under `names` in a temp folder.
    fn copies(dir: &Path, names: &[&str]) {
        let me = std::env::current_exe().unwrap();
        for name in names {
            std::fs::copy(&me, dir.join(name)).unwrap();
        }
    }

    /// Starts `parent` in `spawn` mode and returns it with its child's pid.
    fn launch(parent: &Path) -> (Child, u32) {
        let mut proc = Command::new(parent)
            .args(["--exact", ENTRY, "--nocapture"])
            .env(MODE, "spawn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = proc.stdout.take().unwrap();
        let pid = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.strip_prefix("CHILD=").and_then(|p| p.trim().parse().ok()))
            .expect("the child's pid");
        (proc, pid)
    }

    fn stop(mut proc: Child) {
        drop(proc.stdin.take());
        let _ = proc.wait();
    }

    fn resolve_pid(pid: u32) -> (AcceptedPeer, Result<Resolution, Unverified>) {
        let info = SystemProcs.read(pid).unwrap();
        let peer = AcceptedPeer {
            pid,
            start_us: info.start_us,
            accepted_us: info.start_us + 1,
        };
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: current_uid(),
            procs: &SystemProcs,
            matcher: &matcher,
            daemon: None,
            marks: None,
            terminal_proof: TERMINAL_PROOF,
        };
        (peer, resolve(peer, &checks, None))
    }

    #[test]
    fn a_client_under_a_claude_process_is_that_agent() {
        let dir = tempfile::tempdir().unwrap();
        copies(dir.path(), &["claude.exe", "helper.exe"]);
        let (claude, child) = launch(&dir.path().join("claude.exe"));
        let agent = SystemProcs.read(claude.id()).unwrap();
        let (peer, r) = resolve_pid(child);
        let r = r.unwrap();
        assert!(r.who.is_agent(), "{r:?}");
        assert_eq!(r.via, ResolvedVia::Ancestry);
        let session = format!("{}:{}", agent.pid, agent.start_us);
        assert_eq!(r.who.requester.session_id(), Some(session.as_str()));
        assert!(!r.confirmable);
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: current_uid(),
            procs: &SystemProcs,
            matcher: &matcher,
            daemon: None,
            marks: None,
            terminal_proof: TERMINAL_PROOF,
        };
        assert_eq!(
            confirmation_refusal(peer, &checks, None),
            Some(RefusalReason::AgentAncestry)
        );
        stop(claude);
        // Gone: the identity can no longer be verified.
        assert_eq!(resolve(peer, &checks, None), Err(Unverified));
    }

    /// The same tree without `claude.exe` is never an agent, and without a
    /// terminal proof it never confirms.
    #[test]
    fn a_client_without_an_agent_is_not_one() {
        let dir = tempfile::tempdir().unwrap();
        copies(dir.path(), &["helper.exe"]);
        let (parent, child) = launch(&dir.path().join("helper.exe"));
        // Unverified is fine too: the walk may not reach a clean end here.
        if let Ok(r) = resolve_pid(child).1 {
            assert_eq!(r.who, Who::unattributed());
            assert!(!r.confirmable);
        }
        stop(parent);
    }
}
