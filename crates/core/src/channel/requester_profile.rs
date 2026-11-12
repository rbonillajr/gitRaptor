//! The profile of a connection by its requester, not by its client (S-01,
//! SEC-MCP-01, ADR-MCP-001 § 2, TS-GRP-010, DS-TS-GRP-010).
//!
//! The daemon deduces it from the kernel's peer and its ancestry, never from
//! the cwd nor from what the client declares (which may only ask for less),
//! and fixes it once, at `hello`. In doubt, the most restrictive profile.

use std::path::Path;

use gitraptor_api::messages::{ClientKind, ConnectionProfile};

use gitraptor_api::timemachine::ResolvedVia;

use super::authz::{AcceptedPeer, Checks};
use super::marks::ExecutorMarks;
use super::requester::{AncestryTrace, Resolution, resolve_traced};

/// What the shared ancestry walk says about a peer (D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ancestry {
    /// `resolve_traced` failed: gone, reused or unreadable.
    Unverified,
    /// Below the daemon; `marked` when an executor mark resolved it.
    DaemonDescendant { marked: bool },
    /// An agent, directly or as the only one under a multiplexer.
    Agent,
    /// No agent found: the resolution and what the walk saw.
    Unattributed {
        resolution: Resolution,
        trace: AncestryTrace,
    },
}

/// Why the daemon fixed this profile (log only, never on the wire: D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileReason {
    DeclaredMcp,
    HookClient,
    DaemonSelf,
    RequesterUnverified,
    DaemonDescendant,
    AgentInAncestry,
    AmbiguousAncestry,
    Unconfirmable,
    Developer,
    /// Debug builds only (D3-bis): rule (i) under a test's agent matcher.
    TestTtyless,
}

impl ProfileReason {
    /// The kebab-case word of the log (closed vocabulary).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeclaredMcp => "declared-mcp",
            Self::HookClient => "hook-client",
            Self::DaemonSelf => "daemon-self",
            Self::RequesterUnverified => "requester-unverified",
            Self::DaemonDescendant => "daemon-descendant",
            Self::AgentInAncestry => "agent-in-ancestry",
            Self::AmbiguousAncestry => "ambiguous-ancestry",
            Self::Unconfirmable => "unconfirmable",
            Self::Developer => "developer",
            Self::TestTtyless => "test-ttyless",
        }
    }
}

/// The profile of a connection outside the desktop class, fixed at `hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileDecision {
    pub profile: ConnectionProfile,
    pub reason: ProfileReason,
}

impl ProfileDecision {
    /// `mcp` by the requester rule, not by declaration: the exceptions of D8
    /// apply (declared channel, own reserved verdict, non-MCP read budgets).
    pub fn narrowed(&self) -> bool {
        self.profile == ConnectionProfile::Mcp && self.reason != ProfileReason::DeclaredMcp
    }
}

/// The MCP server's executable (`raptor-mcp`, `raptor-mcp.exe`).
pub fn is_mcp_exe(exe: Option<&Path>) -> bool {
    exe.and_then(Path::file_name)
        .is_some_and(|n| n == "raptor-mcp" || n == "raptor-mcp.exe")
}

/// Steps shared with `desktop::decide` (D2). Order: resolve → executor mark →
/// daemon descendant → agent → unattributed.
pub fn ancestry(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
) -> Ancestry {
    let Ok((resolution, trace)) = resolve_traced(peer, checks, marks) else {
        return Ancestry::Unverified;
    };
    // The mark first: a marked child of an agent's plan is the daemon's.
    if resolution.via == ResolvedVia::Executor {
        return Ancestry::DaemonDescendant { marked: true };
    }
    if trace.daemon_descendant {
        return Ancestry::DaemonDescendant { marked: false };
    }
    if resolution.who.is_agent() {
        return Ancestry::Agent;
    }
    Ancestry::Unattributed { resolution, trace }
}

/// Profile of any connection outside the desktop class (D1), first rule that
/// applies. `daemon_children_as_self` is the test knob of D3: always `false`
/// in release.
pub fn decide(
    peer: AcceptedPeer,
    checks: &Checks<'_>,
    marks: Option<&ExecutorMarks>,
    declared: ClientKind,
    exe: Option<&Path>,
    installed_exe: bool,
    daemon_children_as_self: bool,
) -> ProfileDecision {
    let with = |profile, reason| ProfileDecision { profile, reason };
    let mcp = |reason| with(ConnectionProfile::Mcp, reason);

    // (a) A client may always ask for less.
    if declared == ClientKind::Mcp || is_mcp_exe(exe) {
        return mcp(ProfileReason::DeclaredMcp);
    }
    // (b) The Guardrails hook client runs under the agent's `git` and keeps
    // its decision methods (ADR-MCP-001 § 2): classified by the installed
    // binary, the same predicate as `limits::is_hook_client`.
    if declared == ClientKind::Other && installed_exe {
        return with(ConnectionProfile::Full, ProfileReason::HookClient);
    }
    // (c) The daemon's own process is not a requester: only the kernel's
    // peer has its `(pid, start)`.
    if checks.daemon == Some((peer.pid, peer.start_us)) {
        return with(ConnectionProfile::Full, ProfileReason::DaemonSelf);
    }
    match ancestry(peer, checks, marks) {
        // (d) Gone, reused or unreadable: in doubt, the most restrictive.
        Ancestry::Unverified => mcp(ProfileReason::RequesterUnverified),
        // (e) Test knob (D3, debug builds only): an unmarked child of an
        // in-process daemon counts as the daemon itself.
        Ancestry::DaemonDescendant { marked: false } if daemon_children_as_self => {
            with(ConnectionProfile::Full, ProfileReason::DaemonSelf)
        }
        Ancestry::DaemonDescendant { .. } => mcp(ProfileReason::DaemonDescendant),
        // (f) An agent, directly or as the only one under a multiplexer.
        Ancestry::Agent => mcp(ProfileReason::AgentInAncestry),
        // (g)-(i) Full only for a caller that could confirm: controls 1 to 3
        // alone miss a multiplexer server an agent detached (D6).
        Ancestry::Unattributed { resolution, trace } => {
            if resolution.confirmable {
                with(ConnectionProfile::Full, ProfileReason::Developer)
            } else if trace.interpreter || trace.multiplexer || !trace.ended_cleanly {
                mcp(ProfileReason::AmbiguousAncestry)
            } else {
                mcp(ProfileReason::Unconfirmable)
            }
        }
    }
}

/// Test mode of rule (i) (D3-bis, DS-TS-GRP-010): a clean ancestry without a
/// controlling terminal counts as the developer. `cargo test` has no terminal,
/// locally nor in CI, so a test's client of a daemon process would otherwise
/// get `mcp`. The caller applies it only in debug builds and only when the
/// agent matcher is a test's override, which already takes the real Claude
/// Code for a plain process; every other rule stays as it is.
#[cfg(debug_assertions)]
pub fn test_ttyless(decision: ProfileDecision, on: bool) -> ProfileDecision {
    if on && decision.reason == ProfileReason::Unconfirmable {
        ProfileDecision {
            profile: ConnectionProfile::Full,
            reason: ProfileReason::TestTtyless,
        }
    } else {
        decision
    }
}
