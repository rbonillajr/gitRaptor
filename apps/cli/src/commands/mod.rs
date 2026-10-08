//! The subcommands of `raptor` (ADR-GRP-016 § 5). Each one lives in its own
//! file with its arguments and its handler: a story that adds an action to
//! a subcommand edits only that file. A new top-level subcommand is a file
//! here and one line in [`commands!`], in the order `--help` lists them.

use std::process::ExitCode;

use crate::Display;

pub(crate) use tui::tui;

/// What every subcommand may read from the top level of the command line.
pub(crate) struct Global {
    pub(crate) display: Display,
}

macro_rules! commands {
    ($($(#[$meta:meta])* $variant:ident($module:ident)),* $(,)?) => {
        $(mod $module;)*

        #[derive(clap::Subcommand)]
        pub(crate) enum Command {
            $($(#[$meta])* $variant($module::Cmd),)*
        }

        impl Command {
            pub(crate) fn run(self, global: &Global) -> ExitCode {
                match self {
                    $(Self::$variant(cmd) => cmd.run(global),)*
                }
            }
        }
    };
}

commands! {
    Tui(tui),
    Daemon(daemon),
    Repo(repo),
    Status(status),
    Events(events),
    Sessions(sessions),
    Agent(agent),
    Undo(undo),
    Timeline(timeline),
    Mcp(mcp),
    #[command(hide = true)]
    Ui(ui),
    Guard(guard),
}
