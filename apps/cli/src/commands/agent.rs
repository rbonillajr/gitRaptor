//! `raptor agent`: explicit registration of agents and the correction of a detected
//! attribution.

use std::path::PathBuf;
use std::process::ExitCode;

use super::Global;
use crate::agent;

/// Register the agent that works in a worktree, withdraw its registration, or correct a detected
/// attribution.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    #[command(subcommand)]
    action: AgentAction,
}

#[derive(clap::Subcommand)]
enum AgentAction {
    /// Register an agent in a worktree: "Claude Code", or any other agent by its name
    /// (Codex, Cursor...). An agent registers itself in the worktree it works in.
    Register {
        /// The agent: "Claude Code" or the name of another agent.
        agent: String,
        /// The worktree (default: the current folder). Only the developer names another one.
        #[arg(long)]
        worktree: Option<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Withdraw the registration of an agent: its session ends (reserved to the developer).
    Withdraw {
        /// The agent: "Claude Code" or the name of another agent.
        agent: String,
        /// The worktree (default: the current folder).
        #[arg(long)]
        worktree: Option<PathBuf>,
    },
    /// Correct the agent of a detected session, from its start (reserved to the developer).
    Correct {
        /// The agent the session really is: "Claude Code" or the name of another agent.
        agent: String,
        /// The worktree of the session (default: the current folder).
        #[arg(long)]
        worktree: Option<PathBuf>,
        /// The session to correct, when several are present (`raptor sessions --json`).
        #[arg(long)]
        session: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Withdraw the correction of a session: it returns to the detected attribution
    /// (reserved to the developer).
    Uncorrect {
        /// The worktree of the session (default: the current folder).
        #[arg(long)]
        worktree: Option<PathBuf>,
        /// The corrected session, when several are present.
        #[arg(long)]
        session: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            AgentAction::Register {
                agent,
                worktree,
                json,
            } => agent::register(&agent, worktree, json),
            AgentAction::Withdraw { agent, worktree } => agent::withdraw(&agent, worktree),
            AgentAction::Correct {
                agent,
                worktree,
                session,
                json,
            } => agent::correct(&agent, worktree, session, json),
            AgentAction::Uncorrect {
                worktree,
                session,
                json,
            } => agent::uncorrect(worktree, session, json),
        }
    }
}
