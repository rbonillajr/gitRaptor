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
//! Accepted residual risk (ADR-GRP-005 § 6): an agent that detaches from its
//! process tree (double fork with `setsid`, `launchctl submit`) evades the
//! ancestry check; the audit still records the attempt.

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
/// after its version.
fn in_native_versions(exe: &Path) -> bool {
    let mut parents = exe.ancestors().skip(1);
    let versions = parents.next().and_then(Path::file_name);
    let claude = parents.next().and_then(Path::file_name);
    versions.is_some_and(|v| v == "versions") && claude.is_some_and(|c| c == "claude")
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
    parts
        .windows(2)
        .any(|w| w[0] == "@anthropic-ai" && w[1].starts_with("claude-code"))
}

/// One process of the ancestry, as stored in the audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainLink {
    pub pid: u32,
    pub start_us: u64,
    pub exe: Option<String>,
    pub agent: bool,
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
    /// Wall clock of the accept, microseconds since the epoch.
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
        out.daemon |= daemon == Some((current.pid, current.start_us));
        if current.pid <= 1 || current.ppid == 0 {
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
}

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

    // The session leader, unless it is the caller (already walked).
    if caller.session == 0 {
        return refuse(client, chain, RefusalReason::IdentityUnverified);
    }
    if caller.session != caller.pid && !chain.iter().any(|l| l.pid == caller.session) {
        match procs.read(caller.session) {
            // Another user's leader (`login` in Terminal.app): not an agent
            // of this user, and its ancestry is not this user's either.
            Ok(leader) if leader.uid != uid => client.chain_truncated = true,
            Ok(leader) => {
                let lw = walk(&leader, checks);
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
    // Still the same process after the walk.
    match procs.read(peer.pid) {
        Ok(again) if again.start_us == caller.start_us && again.ppid == caller.ppid => {}
        _ => return refuse(client, chain, RefusalReason::IdentityUnverified),
    }
    Verdict {
        client,
        chain,
        refused: None,
    }
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

    fn verdict(t: &Tree, pid: u32, start: u64) -> Verdict {
        let matcher = AgentMatcher::default();
        let checks = Checks {
            uid: UID,
            procs: t,
            matcher: &matcher,
            daemon: Some(DAEMON),
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
}
