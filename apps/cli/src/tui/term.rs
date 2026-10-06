//! The real terminal (ADR-CKP-003 § 1): `ratatui::init()` enters raw mode
//! and the alternate screen and installs the panic hook that restores the
//! terminal; [`Restore`] restores it on every other way out.

use std::io;
use std::path::PathBuf;

use crate::client::{self, Connector};
use crate::model::{Model, Size};
use crate::present::i18n::Lang;
use crate::queue;
use crate::tui::app::App;
use crate::tui::input::InputThread;

/// Restores the terminal when dropped: normal exit and errors. A panic is
/// covered by the hook of `ratatui::init()`.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Opens the TUI and runs it until the user quits.
pub fn run(connector: impl Connector, cwd: Option<PathBuf>) -> io::Result<()> {
    let terminal = ratatui::try_init()?;
    let _restore = Restore;
    let area = terminal.size()?;
    let size = Size {
        width: area.width,
        height: area.height,
    };
    let (inbox, input_out, engine_out) = queue::inbox();
    let mut app = App::new(terminal, Model::new(Lang::detect(), size), inbox);
    // The first frame ("connecting…") before anything else.
    app.draw()?;
    let input = InputThread::spawn(input_out);
    let channel = client::spawn(connector, cwd, engine_out);
    app.attach(channel.cmds.clone());
    let result = app.run();
    input.stop();
    channel.shutdown();
    result
}
