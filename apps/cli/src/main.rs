mod agent;
mod autostart;
mod codes;
mod commands;
mod discovery;
mod events;
mod guard;
mod i18n;
mod mcp;
mod resources;
mod sessions;
mod status;
mod support;
mod term;
mod undo;

use std::process::ExitCode;

use clap::Parser;

// The helpers every subcommand shares, at the crate root where they were.
pub(crate) use support::{
    command_path, confirm, engine, error_text, offers, profile_dirs, refusal_text, repo_error,
    shown, snapshot,
};

use commands::Command;

/// The Git copilot for teams that code with AI agents.
#[derive(Parser)]
#[command(name = "raptor", version, about)]
struct Cli {
    #[command(flatten)]
    display: Display,
    /// Language of the messages: en or es [default: GITRAPTOR_LANG, then LC_ALL, LC_MESSAGES,
    /// LANG].
    #[arg(long, global = true, value_parser = parse_lang)]
    lang: Option<gitraptor_cli::present::i18n::Lang>,
    #[command(subcommand)]
    command: Option<Command>,
}

/// How the cockpit paints (DSYS-GRP-001 § 6; TS-CKP-004, 2026-10-05 amendment).
#[derive(clap::Args, Clone, Copy)]
struct Display {
    /// Colors for a dark or light terminal, or high contrast [default: detect the background;
    /// also GITRAPTOR_THEME].
    #[arg(long, value_parser = parse_theme)]
    theme: Option<gitraptor_theme::ThemeChoice>,
    /// Paint without colors (also NO_COLOR).
    #[arg(long)]
    no_color: bool,
    /// ASCII symbols instead of Unicode ones.
    #[arg(long)]
    ascii: bool,
}

fn parse_theme(value: &str) -> Result<gitraptor_theme::ThemeChoice, String> {
    value
        .parse()
        .map_err(|err: gitraptor_theme::ParseThemeChoiceError| err.to_string())
}

fn parse_lang(value: &str) -> Result<gitraptor_cli::present::i18n::Lang, String> {
    match value {
        "en" => Ok(gitraptor_cli::present::i18n::Lang::En),
        "es" => Ok(gitraptor_cli::present::i18n::Lang::Es),
        _ => Err("expected `en` or `es`".into()),
    }
}

fn main() -> ExitCode {
    // What a Guardrails dispatcher starts: positional constants, no clap, no profile, no
    // daemon start (ADR-GRD-001 § 2).
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "hook") {
        return guard::hook(&args[2..]);
    }
    let cli = Cli::parse();
    // Before any message: every catalog (the TUI's and the CLI's) reads it from here.
    gitraptor_cli::present::i18n::Lang::choose(cli.lang);
    let global = commands::Global {
        display: cli.display,
    };
    match cli.command {
        None => commands::tui(cli.display),
        Some(command) => command.run(&global),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
