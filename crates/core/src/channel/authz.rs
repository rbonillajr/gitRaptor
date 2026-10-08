//! Authorization of reserved commands, decided only in the daemon
//! (ADR-GRP-005 § 6, SEC-03, SEC-13).
//!
//! The client is identified by what the kernel says about the socket peer;
//! nothing the client declares counts. A reserved command is accepted only
//! if the caller has a controlling terminal, neither the caller, nor its
//! ancestors, nor its session leader and the leader's ancestors are agent
//! processes, and the caller's identity is the same before and after the
//! checks. Any read error refuses (fail-closed).
//!
//! A descendant of the daemon itself (a hook or a `git` that the daemon's
//! operation executor runs) is refused too: it would act with the authority
//! of whoever requested the operation (confused deputy, DEP-MCP-3).
//!
//! On Windows the terminal is the console (TQ-14, DS-TS-GRP-004 § 9): the
//! process hosting it stands for the session leader, it must be in an
//! interactive Windows session, and its creator must be the caller or one of
//! the caller's ancestors.
//!
//! Accepted residual risk (ADR-GRP-005 § 6): an agent that detaches from its
//! process tree (double fork with `setsid`, `launchctl submit`, a scheduled
//! task) evades the ancestry check, and on Windows a process may choose its
//! parent (`PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`); the audit still records
//! the attempt.

use std::path::{Component, Path};

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{ClientIdentity, RefusalReason};
use serde::Serialize;

use super::peer::{ProcError, ProcInfo, ProcSource};

/// Longest ancestry walked before giving up (fail-closed).
const MAX_DEPTH: usize = 64;

/// What an executable is, for the purpose of these checks. Shared with the
/// S1 signal of ADR-GRP-012 when the detector lands (US-GRP-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExeClass {
    ClaudeCode,
    /// A script interpreter that may be hosting an agent (`node`, `bun`,
    /// `deno`). Refused until SPIKE-GRP-001 settles how to read argv[1].
    Interpreter,
    Other,
}

/// Classifies executables as agents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentMatcher {
    /// Debug-build test override: only these file names are agents, and
    /// interpreters are not refused.
    override_names: Option<Vec<String>>,
}

impl AgentMatcher {
    /// Only `names` are agents. For tests (and debug builds through
    /// `GITRAPTOR_AGENT_EXECUTABLES`), so the Claude Code session that runs
    /// the tests is not taken for the simulated agent.
    pub fn only(names: Vec<String>) -> Self {
        Self {
            override_names: Some(names),
        }
    }

    pub fn is_override(&self) -> bool {
        self.override_names.is_some()
    }

    pub fn classify(&self, exe: &Path) -> ExeClass {
        let name = exe
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        if let Some(names) = &self.override_names {
            return if names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                ExeClass::ClaudeCode
            } else {
                ExeClass::Other
            };
        }
        if name == "claude" || in_native_versions(exe) || in_npm_package(exe) {
            return ExeClass::ClaudeCode;
        }
        if matches!(name, "node" | "nodejs" | "bun" | "deno") {
            return ExeClass::Interpreter;
        }
        ExeClass::Other
    }
}

/// `…/claude/versions/<version>`: the native installer names the binary
/// after its version. Compared without case, as Windows and macOS paths are.
fn in_native_versions(exe: &Path) -> bool {
    let mut parents = exe.ancestors().skip(1);
    let versions = parents.next().and_then(Path::file_name);
    let claude = parents.next().and_then(Path::file_name);
    let is = |name: Option<&std::ffi::OsStr>, want: &str| {
        name.is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(want))
    };
    is(versions, "versions") && is(claude, "claude")
}

/// Any path through `@anthropic-ai/claude-code`.
fn in_npm_package(exe: &Path) -> bool {
    let parts: Vec<_> = exe
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect();
    parts.windows(2).any(|w| {
        w[0].eq_ignore_ascii_case("@anthropic-ai")
            && w[1].to_ascii_lowercase().starts_with("claude-code")
    })
}

/// One process of the ancestry, as stored in the audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainLink {
    pub pid: u32,
    pub start_us: u64,
    pub exe: Option<String>,
    pub agent: bool,
    /// Windows session of the process (absent on Unix).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop_session: Option<u32>,
    /// This link is the process hosting the caller's console (Windows).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub console_host: bool,
}

/// The daemon's verdict on one reserved command attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub client: ClientIdentity,
    /// Caller first, then its ancestors, then the session leader's chain.
    pub chain: Vec<ChainLink>,
    pub refused: Option<RefusalReason>,
}

/// The identity recorded when the connection was accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedPeer {
    pub pid: u32,
    pub start_us: u64,
    /// The accept, on the clock of the start times ([`super::peer::proc_clock_us`]),
    /// microseconds since the epoch.
    pub accepted_us: u64,
}

struct Walk {
    chain: Vec<ChainLink>,
    agent: bool,
    /// The daemon itself is in the chain.
    daemon: bool,
    truncated: bool,
    broken: bool,
}

fn link(info: &ProcInfo, matcher: &AgentMatcher) -> (ChainLink, bool) {
    let agent = info
        .exe
        .as_deref()
        .map(|exe| matcher.classify(exe) != ExeClass::Other)
        .unwrap_or(false);
    (
        ChainLink {
            pid: info.pid,
            start_us: info.start_us,
            exe: info.exe.as_ref().map(|p| p.to_string_lossy().into_owned()),
            agent,
            desktop_session: info.desktop_session,
            console_host: false,
        },
        agent,
    )
}

/// Walks from `first` up to the root. Stops cleanly only at pid 1 or at a
/// process known to belong to another user; anything else unreadable breaks
/// the walk.
fn walk(first: &ProcInfo, checks: &Checks<'_>) -> Walk {
    let Checks {
        uid,
        procs,
        matcher,
        daemon,
        marks,
        ..
    } = *checks;
    let mut out = Walk {
        chain: Vec::new(),
        agent: false,
        daemon: false,
        truncated: false,
        broken: false,
    };
    let mut current = first.clone();
    for _ in 0..MAX_DEPTH {
        let (l, agent) = link(&current, matcher);
        out.chain.push(l);
        out.agent |= agent;
        // The daemon itself, or a process a running operation started
        // (DEP-MCP-3), even if it was reparented away from the daemon.
        out.daemon |= daemon == Some((current.pid, current.start_us))
            || marks.is_some_and(|m| m.lookup(&current).is_some());
        if current.pid <= 1 || current.ppid == 0 {
            return out;
        }
        // The desktop root (Windows `explorer.exe`), whose parent is gone.
        if procs.is_session_root(&current) {
            out.truncated = true;
            return out;
        }
        match procs.read(current.ppid) {
            Ok(parent) if parent.uid != uid => {
                out.truncated = true;
                return out;
            }
            // A parent younger than its child is a reused pid.
            Ok(parent) if parent.start_us > current.start_us => {
                out.broken = true;
                return out;
            }
            Ok(parent) => current = parent,
            Err(ProcError::Denied) if procs.foreign_to(current.ppid, uid) == Some(true) => {
                out.truncated = true;
                return out;
            }
            Err(_) => {
                out.broken = true;
                return out;
            }
        }
    }
    out.broken = true;
    out
}

/// What the checks need.
#[derive(Clone, Copy)]
pub struct Checks<'a> {
    pub uid: u32,
    pub procs: &'a dyn ProcSource,
    pub matcher: &'a AgentMatcher,
    /// `(pid, start)` of the daemon: its descendants are refused.
    pub daemon: Option<(u32, u64)>,
    /// Processes started by running operations: refused like the daemon's
    /// descendants.
    pub marks: Option<&'a super::marks::ExecutorMarks>,
    /// Whether this platform can prove a caller is the developer at a
    /// terminal (controlling terminal and session leader). Without it every
    /// reserved command and every confirmation is refused as `Unsupported`
    /// after the ancestry is walked (TQ-14). [`TERMINAL_PROOF`] in
    /// production; injected by tests so both branches run on every OS.
    pub terminal_proof: bool,
    /// Whether the marks of a running operation also cover the descendants
    /// of a marked child whose parent ended (Unix: its process group).
    /// Without it, an unattributed caller whose ancestry cannot vouch for it
    /// is refused even with a terminal proof (C-01, DS-TS-GRP-004 § 9 C4).
    /// [`ORPHANS_MARKED`] in production; injected by tests.
    pub orphans_marked: bool,
}

/// Unix has a controlling terminal and session leaders; Windows has the
/// console of an interactive session (TQ-14 → A, DS-TS-GRP-004 § 9). An
/// OS-verified presence for high-risk commands is TS-GRP-007.
pub const TERMINAL_PROOF: bool = cfg!(any(unix, windows));

/// Process groups mark a whole operation on Unix; Windows has no Job
/// Objects for it yet (W6).
pub const ORPHANS_MARKED: bool = cfg!(unix);

/// Runs the checks of ADR-GRP-005 § 6 (points 1 to 3) on the peer of a
/// connection, now.
pub fn check_reserved(peer: AcceptedPeer, checks: &Checks<'_>) -> Verdict {
    let Checks { uid, procs, .. } = *checks;
    let mut client = ClientIdentity {
        pid: peer.pid,
        start_us: peer.start_us,
        exe: None,
        agent_ancestor: false,
        daemon_descendant: false,
        controlling_terminal: false,
        chain_truncated: false,
    };
    let refuse = |client, chain, reason| Verdict {
        client,
        chain,
        refused: Some(reason),
    };
    let caller = match procs.read(peer.pid) {
        Ok(info) => info,
        Err(ProcError::Unsupported) => {
            return refuse(client, Vec::new(), RefusalReason::Unsupported);
        }
        Err(_) => return refuse(client, Vec::new(), RefusalReason::IdentityUnverified),
    };
    client.exe = caller
        .exe
        .as_deref()
        .map(|p| Untrusted::from_os(p.as_os_str()));
    client.controlling_terminal = caller.controlling_terminal;
    // The same process that connected: same start, started before the accept.
    if caller.start_us != peer.start_us || caller.start_us > peer.accepted_us || caller.uid != uid {
        return refuse(client, Vec::new(), RefusalReason::IdentityUnverified);
    }

    let own = walk(&caller, checks);
    let mut chain = own.chain;
    client.agent_ancestor = own.agent;
    client.daemon_descendant = own.daemon;
    client.chain_truncated = own.truncated;
    if own.agent {
        return refuse(client, chain, RefusalReason::AgentAncestry);
    }
    if own.daemon {
        return refuse(client, chain, RefusalReason::DaemonDescendant);
    }
    if own.broken {
        return refuse(client, chain, RefusalReason::IdentityUnverified);
    }
    // Nothing else can prove the caller is the developer (Windows, TQ-14).
    if !checks.terminal_proof {
        return refuse(client, chain, RefusalReason::Unsupported);
    }

    // The session leader, unless it is the caller (already walked). On
    // Windows, the process hosting the console.
    if caller.session == 0 {
        return refuse(client, chain, RefusalReason::IdentityUnverified);
    }
    let consoles = procs.console_hosts();
    let own_len = chain.len();
    // The leader read here, pinned by `(pid, start)` for the last check.
    let mut leader_pinned = None;
    // A console host already in the caller's chain (`conhost.exe raptor.exe`)
    // was walked with it, and so was its creator: only mark it for the audit.
    if consoles && let Some(host) = chain.iter_mut().find(|l| l.pid == caller.session) {
        host.console_host = true;
    }
    if caller.session != caller.pid && !chain.iter().any(|l| l.pid == caller.session) {
        match procs.read(caller.session) {
            // A console hosted by another user, or that cannot be read, is
            // not the developer's (fail-closed).
            Ok(leader) if consoles && leader.uid != uid => {
                return refuse(client, chain, RefusalReason::NoControllingTerminal);
            }
            // Another user's leader (`login` in Terminal.app): not an agent
            // of this user, and its ancestry is not this user's either.
            Ok(leader) if leader.uid != uid => client.chain_truncated = true,
            Ok(leader) => {
                leader_pinned = Some((leader.pid, leader.start_us));
                let mut lw = walk(&leader, checks);
                if consoles && let Some(host) = lw.chain.first_mut() {
                    host.console_host = true;
                }
                // C8: the console's creator (the host's parent, if the walk
                // could read it) is the caller or one of its ancestors;
                // otherwise the caller attached to another's console.
                let creator_ours = lw.chain.get(1).is_some_and(|creator| {
                    chain[..own_len]
                        .iter()
                        .any(|l| (l.pid, l.start_us) == (creator.pid, creator.start_us))
                });
                chain.extend(lw.chain);
                if lw.agent {
                    client.agent_ancestor = true;
                    return refuse(client, chain, RefusalReason::SessionLeaderAgent);
                }
                if lw.daemon {
                    client.daemon_descendant = true;
                    return refuse(client, chain, RefusalReason::DaemonDescendant);
                }
                if lw.broken {
                    return refuse(client, chain, RefusalReason::IdentityUnverified);
                }
                if consoles && !creator_ours {
                    return refuse(client, chain, RefusalReason::NoControllingTerminal);
                }
            }
            Err(_) if consoles => {
                return refuse(client, chain, RefusalReason::NoControllingTerminal);
            }
            Err(ProcError::Denied) if procs.foreign_to(caller.session, uid) == Some(true) => {
                client.chain_truncated = true;
            }
            Err(_) => return refuse(client, chain, RefusalReason::IdentityUnverified),
        }
    }

    if !caller.controlling_terminal {
        return refuse(client, chain, RefusalReason::NoControllingTerminal);
    }
    // Still the same process after the walk, with the same terminal (C5).
    match procs.read(peer.pid) {
        Ok(again)
            if again.start_us == caller.start_us
                && again.ppid == caller.ppid
                && again.session == caller.session
                && again.controlling_terminal == caller.controlling_terminal => {}
        _ => return refuse(client, chain, RefusalReason::IdentityUnverified),
    }
    // And the leader (the console host) walked is still the same process.
    if let Some((pid, start)) = leader_pinned {
        match procs.read(pid) {
            Ok(again) if again.start_us == start => {}
            _ => return refuse(client, chain, RefusalReason::IdentityUnverified),
        }
    }
    Verdict {
        client,
        chain,
        refused: None,
    }
}

/// Why a process's console does not count as the developer's terminal
/// (Windows, DS-TS-GRP-004 § 9). Only for explaining a refusal: the daemon
/// decides with [`check_reserved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleIssue {
    /// Not attached to a console.
    NoConsole,
    /// A console in session 0 (a service, OpenSSH) or a session with no
    /// user connected.
    NotInteractive,
    /// Another process's console: its creator is not in the ancestry (C8).
    NotYours,
}

/// The [`ConsoleIssue`] of `pid`, read with `procs`; `None` where consoles
/// are not the terminal (Unix), when the console counts, or when `pid`
/// cannot be read.
pub fn console_issue(procs: &dyn ProcSource, uid: u32, pid: u32) -> Option<ConsoleIssue> {
    if !procs.console_hosts() {
        return None;
    }
    let me = procs.read(pid).ok()?;
    if me.session == me.pid {
        return Some(ConsoleIssue::NoConsole);
    }
    if !me.controlling_terminal {
        return Some(ConsoleIssue::NotInteractive);
    }
    let host = procs.read(me.session).ok().filter(|h| h.uid == uid);
    let creator = host.and_then(|host| {
        procs
            .read(host.ppid)
            .ok()
            .filter(|c| c.start_us <= host.start_us)
    });
    let Some(creator) = creator else {
        return Some(ConsoleIssue::NotYours);
    };
    let mut current = me;
    for _ in 0..MAX_DEPTH {
        if (current.pid, current.start_us) == (creator.pid, creator.start_us) {
            return None;
        }
        match procs.read(current.ppid) {
            Ok(parent) if parent.uid == uid && parent.start_us <= current.start_us => {
                current = parent;
            }
            _ => break,
        }
    }
    Some(ConsoleIssue::NotYours)
}

/// The [`ConsoleIssue`] of this process: a client explains the daemon's
/// `no-controlling-terminal` with it (C10).
pub fn own_console_issue() -> Option<ConsoleIssue> {
    console_issue(
        &super::peer::SystemProcs,
        super::peer::current_uid(),
        std::process::id(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    const UID: u32 = 501;

    #[derive(Default)]
    struct Tree {
        procs: HashMap<u32, ProcInfo>,
        denied: HashMap<u32, bool>,
        /// The desktop root (Windows `explorer.exe`).
        root: Option<u32>,
        /// `session` names a console host (Windows).
        consoles: bool,
    }

    /// `tree`, except that `pid` reads as `after` from its second read on.
    struct Flip {
        tree: Tree,
        pid: u32,
        after: ProcInfo,
        reads: std::cell::Cell<u32>,
    }

    impl ProcSource for Flip {
        fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            if pid == self.pid {
                self.reads.set(self.reads.get() + 1);
                if self.reads.get() > 1 {
                    return Ok(self.after.clone());
                }
            }
            self.tree.read(pid)
        }
        fn foreign_to(&self, pid: u32, uid: u32) -> Option<bool> {
            self.tree.foreign_to(pid, uid)
        }
        fn is_session_root(&self, info: &ProcInfo) -> bool {
            self.tree.is_session_root(info)
        }
        fn console_hosts(&self) -> bool {
            self.tree.console_hosts()
        }
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
        /// An unreadable process; `foreign` says whether it is another user's.
        fn deny(&mut self, pid: u32, foreign: bool) {
            self.denied.insert(pid, foreign);
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
        fn is_session_root(&self, info: &ProcInfo) -> bool {
            self.root == Some(info.pid)
        }
        fn console_hosts(&self) -> bool {
            self.consoles
        }
    }

    fn peer(pid: u32, start: u64) -> AcceptedPeer {
        AcceptedPeer {
            pid,
            start_us: start,
            accepted_us: 1_000_000,
        }
    }

    /// Terminal.app: launchd(1) → login (root, unreadable, leader) → zsh → raptor.
    fn developer_terminal() -> Tree {
        let mut t = Tree::default();
        t.deny(10, true);
        t.add(20, 10, "/bin/zsh", 200, true, 10);
        t.add(30, 20, "/usr/local/bin/raptor", 300, true, 10);
        t
    }

    /// The daemon in the synthetic trees.
    const DAEMON: (u32, u64) = (70, 700);

    fn verdict(t: &dyn ProcSource, pid: u32, start: u64) -> Verdict {
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: UID,
            procs: t,
            matcher: &matcher,
            daemon: Some(DAEMON),
            marks: None,
            terminal_proof: true,
            orphans_marked: true,
        };
        check_reserved(peer(pid, start), &checks)
    }

    #[test]
    fn developer_terminal_is_accepted() {
        let v = verdict(&developer_terminal(), 30, 300);
        assert_eq!(v.refused, None);
        assert!(v.client.chain_truncated);
        assert!(v.client.controlling_terminal);
        assert_eq!(v.chain.len(), 2);
    }

    #[test]
    fn agent_ancestor_is_refused() {
        let mut t = developer_terminal();
        t.add(
            25,
            20,
            "/opt/homebrew/Caskroom/claude-code@latest/2.1.284/claude",
            250,
            true,
            10,
        );
        t.add(40, 25, "/bin/zsh", 400, false, 40);
        t.add(41, 40, "/usr/local/bin/raptor", 410, false, 40);
        let v = verdict(&t, 41, 410);
        assert_eq!(v.refused, Some(RefusalReason::AgentAncestry));
        assert!(v.client.agent_ancestor);
    }

    /// `script` under the agent: the caller has a terminal and leads its own
    /// session, but the walk still meets the agent.
    #[test]
    fn pty_under_the_agent_is_refused() {
        let mut t = developer_terminal();
        t.add(
            25,
            20,
            "/Users/u/.local/share/claude/versions/2.1.3",
            250,
            true,
            10,
        );
        t.add(50, 25, "/usr/bin/script", 500, false, 50);
        t.add(51, 50, "/usr/local/bin/raptor", 510, true, 51);
        assert_eq!(
            verdict(&t, 51, 510).refused,
            Some(RefusalReason::AgentAncestry)
        );
    }

    /// The caller was reparented away from the agent (accepted residual risk
    /// does not cover this): its session leader still descends from it.
    #[test]
    fn session_leader_under_an_agent_is_refused() {
        let mut t = developer_terminal();
        t.add(
            25,
            20,
            "/x/node_modules/@anthropic-ai/claude-code/cli",
            250,
            true,
            10,
        );
        t.add(60, 25, "/bin/zsh", 600, false, 60);
        t.add(61, 1, "/usr/local/bin/raptor", 610, true, 60);
        t.add(1, 0, "/sbin/launchd", 1, false, 1);
        let v = verdict(&t, 61, 610);
        assert_eq!(v.refused, Some(RefusalReason::SessionLeaderAgent));
    }

    /// DEP-MCP-3: a hook or `git` run by the daemon's executor, even from
    /// the developer's terminal session, is refused.
    #[test]
    fn a_descendant_of_the_daemon_is_refused() {
        let mut t = developer_terminal();
        t.add(70, 20, "/usr/local/bin/raptor", 700, true, 10);
        t.add(71, 70, "/usr/bin/git", 710, true, 10);
        t.add(72, 71, "/repo/.git/hooks/post-merge", 720, true, 10);
        let v = verdict(&t, 72, 720);
        assert_eq!(v.refused, Some(RefusalReason::DaemonDescendant));
        assert!(v.client.daemon_descendant);
        // The same pid with another start is not the daemon.
        let mut t = developer_terminal();
        t.add(70, 20, "/usr/local/bin/raptor", 699, true, 10);
        t.add(71, 70, "/usr/bin/git", 710, true, 10);
        assert_eq!(verdict(&t, 71, 710).refused, None);
    }

    #[test]
    fn no_controlling_terminal_is_refused() {
        let mut t = developer_terminal();
        t.add(31, 20, "/usr/local/bin/raptor", 310, false, 10);
        assert_eq!(
            verdict(&t, 31, 310).refused,
            Some(RefusalReason::NoControllingTerminal)
        );
    }

    #[test]
    fn identity_changes_and_unreadable_ancestors_are_refused() {
        let t = developer_terminal();
        // A different process now holds the pid.
        assert_eq!(
            verdict(&t, 30, 299).refused,
            Some(RefusalReason::IdentityUnverified)
        );
        // A process of the same user the kernel refuses to describe.
        let mut t = developer_terminal();
        t.procs.remove(&10);
        t.deny(10, false);
        assert_eq!(
            verdict(&t, 30, 300).refused,
            Some(RefusalReason::IdentityUnverified)
        );
        // A parent younger than its child (pid reuse).
        let mut t = developer_terminal();
        t.add(20, 10, "/bin/zsh", 900, true, 10);
        assert_eq!(
            verdict(&t, 30, 300).refused,
            Some(RefusalReason::IdentityUnverified)
        );
        // Gone.
        assert_eq!(
            verdict(&t, 99, 1).refused,
            Some(RefusalReason::IdentityUnverified)
        );
    }

    /// Windows (TQ-14, DS-TS-GRP-004 § 9): explorer (the desktop root, its
    /// parent gone) → powershell → raptor, the console hosted by a
    /// `conhost.exe` that powershell started. `session` is the console host.
    fn windows_console() -> Tree {
        let mut t = Tree::default();
        t.add(10, 5, "C:/Windows/explorer.exe", 100, false, 10);
        t.add(20, 10, "C:/Windows/System32/powershell.exe", 200, true, 21);
        t.add(21, 20, "C:/Windows/System32/conhost.exe", 210, false, 21);
        t.add(30, 20, "C:/Users/u/.cargo/bin/raptor.exe", 300, true, 21);
        t.root = Some(10);
        t.consoles = true;
        t
    }

    #[test]
    fn a_windows_console_is_accepted() {
        let v = verdict(&windows_console(), 30, 300);
        assert_eq!(v.refused, None);
        assert!(v.client.controlling_terminal);
        assert!(v.client.chain_truncated);
        // raptor, powershell, explorer, then the console host (its parent
        // already walked ends the second walk at the desktop root).
        assert!(v.chain.iter().any(|l| l.pid == 21));
    }

    /// The agent opened a pseudoconsole (ConPTY) and a process that
    /// reached the developer's desktop by a clean path attached to it.
    #[test]
    fn a_console_hosted_under_an_agent_is_refused() {
        let mut t = windows_console();
        t.add(40, 20, "C:/Users/u/.local/bin/claude.exe", 400, true, 21);
        t.add(41, 40, "C:/Windows/System32/conhost.exe", 410, false, 41);
        t.add(42, 10, "C:/Users/u/.cargo/bin/raptor.exe", 420, true, 41);
        let v = verdict(&t, 42, 420);
        assert_eq!(v.refused, Some(RefusalReason::SessionLeaderAgent));
        assert!(v.client.agent_ancestor);
    }

    /// No console (`DETACHED_PROCESS`, a GUI process): `session` is the
    /// caller itself. A console in session 0 (a service, OpenSSH) reads as
    /// no controlling terminal too.
    #[test]
    fn without_a_console_or_an_interactive_session_it_is_refused() {
        let mut t = windows_console();
        t.add(31, 10, "C:/Users/u/.cargo/bin/raptor.exe", 310, false, 31);
        assert_eq!(
            verdict(&t, 31, 310).refused,
            Some(RefusalReason::NoControllingTerminal)
        );
        t.add(32, 20, "C:/Users/u/.cargo/bin/raptor.exe", 320, false, 21);
        assert_eq!(
            verdict(&t, 32, 320).refused,
            Some(RefusalReason::NoControllingTerminal)
        );
    }

    /// C5: the caller detached from its console and attached to another
    /// (`FreeConsole` + `AttachConsole`) while the daemon walked.
    #[test]
    fn a_caller_that_changes_console_during_the_checks_is_refused() {
        let t = windows_console();
        let mut after = t.procs[&30].clone();
        after.session = 99;
        let flip = Flip {
            tree: t,
            pid: 30,
            after: after.clone(),
            reads: std::cell::Cell::new(0),
        };
        assert_eq!(
            verdict(&flip, 30, 300).refused,
            Some(RefusalReason::IdentityUnverified)
        );
        // Losing the terminal is a change too.
        let t = developer_terminal();
        let mut after = t.procs[&30].clone();
        after.controlling_terminal = false;
        let flip = Flip {
            tree: t,
            pid: 30,
            after,
            reads: std::cell::Cell::new(0),
        };
        assert_eq!(
            verdict(&flip, 30, 300).refused,
            Some(RefusalReason::IdentityUnverified)
        );
        // The console host is pinned by (pid, start) too.
        let t = windows_console();
        let mut after = t.procs[&21].clone();
        after.start_us = 999;
        let flip = Flip {
            tree: t,
            pid: 21,
            after,
            reads: std::cell::Cell::new(0),
        };
        assert_eq!(
            verdict(&flip, 30, 300).refused,
            Some(RefusalReason::IdentityUnverified)
        );
    }

    /// The caller reached the desktop through a broker (its chain ends
    /// cleanly at another user's process, WMI say) and attached to the
    /// developer's console (`AttachConsole`): the console's creator, the
    /// host's parent, is not in its chain.
    #[test]
    fn attaching_to_someone_elses_console_is_refused() {
        let mut t = windows_console();
        t.deny(50, true);
        t.add(51, 50, "C:/Users/u/.cargo/bin/raptor.exe", 510, true, 21);
        let v = verdict(&t, 51, 510);
        assert_eq!(v.refused, Some(RefusalReason::NoControllingTerminal));
        // Where `session` is a session leader (Unix) nothing changes.
        t.consoles = false;
        assert_eq!(verdict(&t, 51, 510).refused, None);
    }

    /// C9: the audit records the Windows session and marks the console
    /// host; a Unix chain serializes as before.
    #[test]
    fn the_audit_chain_marks_the_console_host() {
        let mut t = windows_console();
        for p in t.procs.values_mut() {
            p.desktop_session = Some(2);
        }
        let v = verdict(&t, 30, 300);
        assert_eq!(v.refused, None);
        let host = v.chain.iter().find(|l| l.console_host).unwrap();
        assert_eq!((host.pid, host.start_us), (21, 210));
        assert!(v.chain.iter().all(|l| l.desktop_session == Some(2)));
        let json = serde_json::to_string(&v.chain).unwrap();
        assert!(json.contains("\"console_host\":true") && json.contains("\"desktop_session\":2"));
        // `conhost.exe raptor.exe`: the host is in the caller's own chain.
        let mut t = windows_console();
        t.add(22, 20, "C:/Windows/System32/conhost.exe", 220, false, 22);
        t.add(33, 22, "C:/Users/u/.cargo/bin/raptor.exe", 330, true, 22);
        let v = verdict(&t, 33, 330);
        assert_eq!(v.refused, None);
        assert!(v.chain.iter().any(|l| l.pid == 22 && l.console_host));
        let unix = serde_json::to_string(&verdict(&developer_terminal(), 30, 300).chain).unwrap();
        assert!(!unix.contains("console_host") && !unix.contains("desktop_session"));
    }

    /// C10: what the CLI tells the developer, from the same reading.
    #[test]
    fn the_console_issue_explains_the_refusal() {
        let t = windows_console();
        assert_eq!(console_issue(&t, UID, 30), None);
        let mut t = windows_console();
        t.add(31, 10, "C:/Users/u/.cargo/bin/raptor.exe", 310, false, 31);
        t.add(32, 20, "C:/Users/u/.cargo/bin/raptor.exe", 320, false, 21);
        t.deny(50, true);
        t.add(51, 50, "C:/Users/u/.cargo/bin/raptor.exe", 510, true, 21);
        assert_eq!(console_issue(&t, UID, 31), Some(ConsoleIssue::NoConsole));
        assert_eq!(
            console_issue(&t, UID, 32),
            Some(ConsoleIssue::NotInteractive)
        );
        assert_eq!(console_issue(&t, UID, 51), Some(ConsoleIssue::NotYours));
        // Unix: never a console issue.
        assert_eq!(console_issue(&developer_terminal(), UID, 30), None);
    }

    /// Windows has a terminal proof (the console) but no group marks
    /// (DS-TS-GRP-004 § 9, C4): checked when the tests build.
    #[cfg(windows)]
    const _: () = assert!(TERMINAL_PROOF && !ORPHANS_MARKED);

    #[test]
    fn interpreters_are_refused_until_argv_can_be_read() {
        let mut t = developer_terminal();
        t.add(26, 20, "/usr/local/bin/node", 260, true, 10);
        t.add(32, 26, "/usr/local/bin/raptor", 320, true, 10);
        assert_eq!(
            verdict(&t, 32, 320).refused,
            Some(RefusalReason::AgentAncestry)
        );
    }

    #[test]
    fn classifier_table() {
        let m = AgentMatcher::default();
        for (path, class) in [
            (
                "/opt/homebrew/Caskroom/claude-code@latest/2.1.284/claude",
                ExeClass::ClaudeCode,
            ),
            ("/Users/u/.local/bin/claude", ExeClass::ClaudeCode),
            (
                "/Users/u/.local/share/claude/versions/2.1.3",
                ExeClass::ClaudeCode,
            ),
            (
                "/x/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                ExeClass::ClaudeCode,
            ),
            (
                "/x/node_modules/@anthropic-ai/claude-code-darwin-arm64/claude",
                ExeClass::ClaudeCode,
            ),
            ("/usr/local/bin/node", ExeClass::Interpreter),
            ("/Users/u/.bun/bin/bun", ExeClass::Interpreter),
            ("/bin/zsh", ExeClass::Other),
            ("/Users/u/versions/2.1.3", ExeClass::Other),
            ("/usr/local/bin/raptor", ExeClass::Other),
            // Case-insensitive file systems (Windows, macOS).
            (
                "/Users/u/.local/share/Claude/Versions/2.1.3",
                ExeClass::ClaudeCode,
            ),
            (
                "/x/node_modules/@Anthropic-AI/Claude-Code/bin/CLAUDE.EXE",
                ExeClass::ClaudeCode,
            ),
        ] {
            assert_eq!(m.classify(Path::new(path)), class, "{path}");
        }
        let only = AgentMatcher::only(vec!["fake-agent".into()]);
        assert_eq!(
            only.classify(Path::new("/tmp/fake-agent")),
            ExeClass::ClaudeCode
        );
        assert_eq!(only.classify(Path::new("/x/claude")), ExeClass::Other);
        assert_eq!(only.classify(Path::new("/usr/bin/node")), ExeClass::Other);
    }

    /// The same rule on Windows paths (S1 of ADR-GRP-012, DS-US-GRP-007 Enmienda 2026-10-08): the
    /// resolved image path of the process, never its command line; `claude.exe` anywhere, a
    /// version folder or the npm package, as on the other platforms.
    #[cfg(windows)]
    #[test]
    fn classifier_table_on_windows_paths() {
        let m = AgentMatcher::default();
        for (path, class) in [
            (r"C:\Users\u\.local\bin\claude.exe", ExeClass::ClaudeCode),
            (r"C:\Users\u\CLAUDE.EXE", ExeClass::ClaudeCode),
            (
                r"C:\Users\u\.local\share\claude\versions\2.1.3",
                ExeClass::ClaudeCode,
            ),
            (
                r"C:\Users\u\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe",
                ExeClass::ClaudeCode,
            ),
            (r"C:\Program Files\nodejs\node.exe", ExeClass::Interpreter),
            (r"C:\Windows\System32\cmd.exe", ExeClass::Other),
            (r"C:\Users\u\claude-helper.exe", ExeClass::Other),
            (r"C:\Users\u\notclaude.exe", ExeClass::Other),
            (r"C:\Users\u\claude.exe.bak", ExeClass::Other),
            (r"C:\Users\u\versions\2.1.3", ExeClass::Other),
        ] {
            assert_eq!(m.classify(Path::new(path)), class, "{path}");
        }
    }
}
