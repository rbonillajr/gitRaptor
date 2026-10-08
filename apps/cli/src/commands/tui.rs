//! `raptor` and `raptor tui`: the cockpit (ADR-CKP-003).

use std::io::IsTerminal;
use std::process::ExitCode;

use gitraptor_api::untrusted::sanitize;

use super::Global;
use crate::{Display, term};

/// Open the cockpit (the default without a subcommand); needs a terminal.
#[derive(clap::Args)]
pub(crate) struct Cmd {
    #[command(flatten)]
    display: Display,
}

impl Cmd {
    pub(crate) fn run(self, global: &Global) -> ExitCode {
        let display = self.display;
        tui(Display {
            theme: display.theme.or(global.display.theme),
            no_color: display.no_color || global.display.no_color,
            ascii: display.ascii || global.display.ascii,
        })
    }
}

/// `raptor` or `raptor tui`: the cockpit when stdin and stdout are a
/// terminal; otherwise exit code 2 with a hint (ADR-CKP-003 § 11).
pub(crate) fn tui(display: Display) -> ExitCode {
    use gitraptor_cli::present::i18n::{Lang, Text};
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        eprintln!("raptor: {}", Text::NeedsTerminal.render(Lang::detect()));
        return ExitCode::from(2);
    }
    let connector = match engine_connector() {
        Ok(connector) => connector,
        Err(err) => {
            eprintln!("raptor: {}", sanitize(&err));
            return ExitCode::FAILURE;
        }
    };
    // Before the TUI's event reader: the terminal's answer must not be read as keys.
    let theme = term::theme(display.theme, display.no_color, display.ascii);
    match gitraptor_cli::tui::run(connector, std::env::current_dir().ok(), theme) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("raptor: {}", sanitize(&err.to_string()));
            ExitCode::FAILURE
        }
    }
}

/// The TUI's connector on the user's profile, as `raptor` resolves it. The engine's
/// launcher (on-demand start, autostart, clean environment) is built here, in the binary,
/// and reaches the TUI as a `Launch` (ADR-CKP-003, Enmienda 2026-10-08).
fn engine_connector() -> Result<gitraptor_cli::client::engine::EngineConnector, String> {
    use gitraptor_api::messages::ClientKind;
    use gitraptor_core::client::ClientOptions;
    use gitraptor_core::profile::ProfileDirs;
    let dirs = ProfileDirs::resolve().map_err(|err| err.to_string())?;
    let options = ClientOptions::new(dirs, ClientKind::Cli);
    Ok(gitraptor_cli::client::engine::EngineConnector::resolved(
        options.connect(),
        Box::new(options.launcher()),
    ))
}
