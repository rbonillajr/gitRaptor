//! `raptor ui`: developer tools of the TUI (TS-CKP-005); not a user surface.

use std::io::IsTerminal;
use std::process::ExitCode;

use super::Global;

/// Developer tools of the TUI (hidden: not a user surface).
#[derive(clap::Args)]
pub(crate) struct Cmd {
    #[command(subcommand)]
    action: UiAction,
}

#[derive(clap::Subcommand)]
enum UiAction {
    /// Every TUI component in every state, with sample data, browsable with the keyboard.
    Gallery {
        /// Print every story as plain text instead of opening the gallery.
        #[arg(long)]
        dump: bool,
        /// Theme mode to start in (or to dump).
        #[arg(long, value_enum, default_value = "truecolor")]
        mode: gitraptor_cli::tui::gallery::Mode,
    },
}

impl Cmd {
    pub(crate) fn run(self, _: &Global) -> ExitCode {
        match self.action {
            UiAction::Gallery { dump, mode } => ui_gallery(dump, mode),
        }
    }
}

/// `raptor ui gallery`: the TUI components with sample data (TS-CKP-005).
fn ui_gallery(dump: bool, mode: gitraptor_cli::tui::gallery::Mode) -> ExitCode {
    use gitraptor_cli::tui::gallery;
    let result = if dump {
        gallery::dump(mode, &mut std::io::stdout().lock())
    } else if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        gallery::run(mode)
    } else {
        eprintln!("raptor ui gallery: needs a terminal; use --dump");
        return ExitCode::from(2);
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // `--dump | head` closes the pipe early: not an error.
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("raptor ui gallery: {err}");
            ExitCode::FAILURE
        }
    }
}
