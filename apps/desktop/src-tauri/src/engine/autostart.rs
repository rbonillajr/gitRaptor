//! The login autostart choice of the app: whether to offer it, the texts of the confirmation and
//! the run of `raptor daemon enable|disable` after the human said yes.
//!
//! The confirmation is a native dialog the webview does not control; here it is an injected
//! closure, so the rule is testable without a window. Nothing runs without a "yes".

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gitraptor_api::client::ensure::EnsureReport;

use super::launcher::{ENSURE_TIMEOUT, FIXED_PATH, OUTPUT_LIMIT, POLL};
use crate::ipc::CommandError;

/// How long `defaults read` may run before it is given up on.
const LANG_TIMEOUT: Duration = Duration::from_secs(3);

/// Why the autostart is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotOfferedReason {
    /// The app does not run from `/Applications/` or `<home>/Applications/`.
    Location,
    /// The autostart is registered with a `raptor` that is not the bundle's.
    OtherBinary,
}

/// What the app may do about the autostart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// It can be enabled.
    Offered,
    /// It is enabled with the bundle's `raptor`; it can be disabled.
    Registered,
    /// Neither.
    NotOffered { reason: NotOfferedReason },
}

/// Whether `app_exe` lives where an installed app does. Paths are compared as given: nothing
/// touches the disk.
fn installed_location(app_exe: &Path, home: Option<&Path>) -> bool {
    let translocated = app_exe
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation");
    if translocated || app_exe.starts_with("/Volumes") {
        return false;
    }
    app_exe.starts_with("/Applications")
        || home.is_some_and(|home| app_exe.starts_with(home.join("Applications")))
}

fn decide(app_exe: &Path, home: Option<&Path>, report: &EnsureReport) -> Offer {
    if !installed_location(app_exe, home) {
        return Offer::NotOffered {
            reason: NotOfferedReason::Location,
        };
    }
    if !report.autostart.registered {
        return Offer::Offered;
    }
    let bundle = app_exe.parent().map(|dir| dir.join("raptor"));
    match (report.autostart.binary.as_deref(), bundle) {
        (Some(binary), Some(bundle)) if Path::new(binary) == bundle => Offer::Registered,
        _ => Offer::NotOffered {
            reason: NotOfferedReason::OtherBinary,
        },
    }
}

/// The pure rule of [`offer`]: `home` is the user's home, never read from `HOME`.
pub fn offer_with_home(app_exe: &Path, home: &Path, report: &EnsureReport) -> Offer {
    decide(app_exe, Some(home), report)
}

/// The offer for the running app: canonicalizes `app_exe` and the home of the current user
/// (from the user database, never from `HOME`) and applies [`offer_with_home`]. Without a home,
/// only `/Applications/` is accepted.
pub fn offer(app_exe: &Path, report: &EnsureReport) -> Offer {
    let Ok(app_exe) = app_exe.canonicalize() else {
        return Offer::NotOffered {
            reason: NotOfferedReason::Location,
        };
    };
    let home = user_home().and_then(|home| home.canonicalize().ok());
    decide(&app_exe, home.as_deref(), report)
}

#[cfg(unix)]
fn user_home() -> Option<PathBuf> {
    nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|user| user.dir)
}

#[cfg(not(unix))]
fn user_home() -> Option<PathBuf> {
    None
}

/// The texts of the confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: String,
    pub body: String,
    pub accept: String,
    pub cancel: String,
}

/// Language of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

/// `Es` when the first language of the output of `defaults read -g AppleLanguages` is Spanish;
/// `En` for anything else.
pub fn lang_from_apple_languages(output: &str) -> Lang {
    let first = output
        .lines()
        .map(|line| line.trim().trim_end_matches(',').trim_matches('"'))
        .find(|line| !line.is_empty() && *line != "(" && *line != ")");
    match first {
        Some(tag) if tag == "es" || tag.starts_with("es-") || tag.starts_with("es_") => Lang::Es,
        _ => Lang::En,
    }
}

/// The language of the system. Runs `/usr/bin/defaults read -g AppleLanguages` with a fixed argv
/// and only `PATH`; any failure is `En`.
pub fn system_lang() -> Lang {
    run_bounded(
        Path::new("/usr/bin/defaults"),
        &["read", "-g", "AppleLanguages"],
        LANG_TIMEOUT,
    )
    .filter(|run| run.success)
    .map_or(Lang::En, |run| {
        lang_from_apple_languages(&String::from_utf8_lossy(&run.stdout))
    })
}

/// The texts for enabling or disabling the autostart.
pub fn prompt(enable: bool, lang: Lang) -> Prompt {
    let (title, body, accept, cancel) = match (enable, lang) {
        (true, Lang::Es) => (
            "Arrancar GitRaptor al iniciar sesión",
            "GitRaptor arrancará su motor cada vez que inicies sesión. Si no lo pediste tú, cancela.",
            "Activar",
            "Cancelar",
        ),
        (true, Lang::En) => (
            "Start GitRaptor at login",
            "GitRaptor will start its engine every time you log in. If you did not ask for this, cancel.",
            "Enable",
            "Cancel",
        ),
        (false, Lang::Es) => (
            "Dejar de arrancar GitRaptor al iniciar sesión",
            "GitRaptor ya no arrancará su motor cuando inicies sesión. Si no lo pediste tú, cancela.",
            "Dejar de arrancar",
            "Cancelar",
        ),
        (false, Lang::En) => (
            "Stop starting GitRaptor at login",
            "GitRaptor will no longer start its engine when you log in. If you did not ask for this, cancel.",
            "Stop starting",
            "Cancel",
        ),
    };
    Prompt {
        title: title.to_owned(),
        body: body.to_owned(),
        accept: accept.to_owned(),
        cancel: cancel.to_owned(),
    }
}

/// Asks `confirm` and, after a "yes", runs `<raptor> daemon enable|disable` (fixed argv, only
/// `PATH`, cwd `/`, no stdin or stderr, at most 10 s). Returns whether the autostart is now
/// registered.
///
/// # Errors
///
/// [`CommandError::NotOffered`] when `enable` does not fit `offer` (nothing is asked or run);
/// [`CommandError::Cancelled`] when `confirm` says no; [`CommandError::Failed`] when `raptor`
/// cannot run, times out or exits with a failure.
pub fn run_choice(
    raptor: &Path,
    enable: bool,
    offer: &Offer,
    lang: Lang,
    confirm: &mut dyn FnMut(&Prompt) -> bool,
) -> Result<bool, CommandError> {
    let applies = matches!(
        (enable, offer),
        (true, Offer::Offered) | (false, Offer::Registered)
    );
    if !applies {
        return Err(CommandError::NotOffered);
    }
    if !confirm(&prompt(enable, lang)) {
        return Err(CommandError::Cancelled);
    }
    let action = if enable { "enable" } else { "disable" };
    match run_bounded(raptor, &["daemon", action], ENSURE_TIMEOUT) {
        Some(run) if run.success => Ok(enable),
        _ => Err(CommandError::Failed),
    }
}

struct Run {
    success: bool,
    stdout: Vec<u8>,
}

/// Runs `program` with a fixed argv and the fixed environment, killing it after `timeout`.
/// `None` when it cannot run or times out.
fn run_bounded(program: &Path, args: &[&str], timeout: Duration) -> Option<Run> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", FIXED_PATH)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.take(OUTPUT_LIMIT as u64).read_to_end(&mut out);
        let _ = sender.send(out);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    let stdout = received.recv_timeout(remaining).unwrap_or_default();
    Some(Run {
        success: status.success(),
        stdout,
    })
}
