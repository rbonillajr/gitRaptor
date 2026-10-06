//! `raptor agent`: explicit registration of agents (US-GRP-009).

use std::path::PathBuf;
use std::process::ExitCode;

use super::Global;
use crate::agent;

/// Register the agent that works in a worktree, or withdraw its registration.
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
        }
    }
}
