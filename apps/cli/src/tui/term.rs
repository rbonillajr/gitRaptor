//! The real terminal (ADR-CKP-003 § 1): `ratatui::init()` enters raw mode
//! and the alternate screen and installs the panic hook that restores the
//! terminal; [`Restore`] restores it on every other way out.

use std::io;
use std::path::PathBuf;

use gitraptor_theme::Theme;

use crate::client::{self, Connector};
use crate::model::{Model, Size};
use crate::present::i18n::{Lang, Text};
use crate::queue;
use crate::tui::app::App;
use crate::tui::input::{InputThread, Pause};

/// Restores the terminal when dropped: normal exit and errors. A panic is
/// covered by the hook of `ratatui::init()`.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// `Ctrl-Z` (ADR-CKP-003 § 9): pause the input thread and wait for it, restore the terminal,
/// stop until the shell's `fg`, then take the terminal again and resume the input. The full
/// repaint is the loop's. The editor reuses [`suspended`].
fn suspend(pause: &Pause) -> io::Result<()> {
    suspended(pause, stop_until_resumed)
}

/// Runs `f` with the terminal handed back: no raw mode, no alternate screen, no input thread
/// reading. The terminal is taken again even when `f` fails.
pub fn suspended<T>(pause: &Pause, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    // A reader that did not confirm and is still alive would take the keys of whoever gets
    // the terminal: do not hand it over.
    if !pause.pause() {
        pause.resume();
        return Err(io::Error::other("the input thread did not pause"));
    }
    ratatui::restore();
    // `restore` leaves the cursor hidden; the shell needs it. The next frame hides it again.
    let _ = ratatui::crossterm::execute!(io::stdout(), ratatui::crossterm::cursor::Show);
    let result = f();
    let again = enter();
    pause.resume();
    let value = result?;
    again?;
    Ok(value)
}

/// Raw mode and the alternate screen again, as `ratatui::init()` left them (its panic hook is
/// still installed).
fn enter() -> io::Result<()> {
    use ratatui::crossterm::execute;
    use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)
}

/// `SIGTSTP` to this process group, as the terminal would send it (so a wrapper such as
/// `cargo run` stops too and the shell sees the job stop): the default action stops it until
/// `SIGCONT`. In an orphaned process group the system discards it and this returns at once.
#[cfg(unix)]
fn stop_until_resumed() -> io::Result<()> {
    use rustix::process::{Signal, kill_current_process_group};
    kill_current_process_group(Signal::TSTP).map_err(io::Error::from)
}

/// No job control: `update` never asks for a suspension here.
#[cfg(not(unix))]
fn stop_until_resumed() -> io::Result<()> {
    Ok(())
}

/// Opens the TUI and runs it until the user quits. `theme` is resolved before, while no
/// event reader is running (`term::theme` of the binary).
pub fn run(connector: impl Connector, cwd: Option<PathBuf>, theme: Theme) -> io::Result<()> {
    // Raw mode first, the drain, and only then the alternate screen: the alternate screen is
    // the signal that the cockpit is reading keys, so a key sent after it must never be
    // taken for typed-ahead input and discarded.
    ratatui::crossterm::terminal::enable_raw_mode()?;
    let _restore = Restore;
    drain_pending_input();
    let terminal = ratatui::try_init()?;
    let opened = std::time::Instant::now();
    let area = terminal.size()?;
    let size = Size {
        width: area.width,
        height: area.height,
    };
    let (inbox, input_out, engine_out) = queue::inbox();
    let mut app = App::new(
        terminal,
        Model::new(Lang::detect(), size).with_theme(theme),
        inbox,
    );
    // The first frame ("connecting…") before anything else.
    app.draw()?;
    let input = InputThread::spawn(input_out);
    let pause = input.pause();
    app.on_suspend(Box::new(move || suspend(&pause)));
    let channel = client::spawn(connector, cwd, engine_out);
    app.attach(channel.cmds.clone());
    let result = app.run();
    input.stop();
    channel.shutdown();
    let lang = app.model.ui.lang;
    if app.model.ui.input_lost {
        return Err(io::Error::other(Text::InputLost.render(lang)));
    }
    // Never a silent exit: a cockpit that closes within moments says which key closed it.
    if result.is_ok()
        && opened.elapsed() < EARLY_EXIT
        && let Some(key) = &app.model.ui.quit_key
    {
        // After the terminal is restored, or the alternate screen would swallow it.
        drop(_restore);
        eprintln!("raptor: {}", Text::ClosedEarly(key).render(lang));
    }
    result
}

/// A cockpit that ends sooner than this after opening is explained to the user.
const EARLY_EXIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Discards what the console already holds when the cockpit opens (a paste or typing ahead
/// while the shell was still running the previous line), so it is never read as commands.
fn drain_pending_input() {
    use ratatui::crossterm::event;
    while event::poll(std::time::Duration::ZERO).unwrap_or(false) {
        if event::read().is_err() {
            break;
        }
    }
}
