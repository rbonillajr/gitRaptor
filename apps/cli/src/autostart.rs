//! `raptor daemon enable` and `raptor daemon disable` (US-GRP-004,
//! ADR-GRP-005 § 3, PQ-1): run here, in the developer's CLI, never through
//! the channel. The engine is not involved and `disable` does not stop it.

use std::process::ExitCode;

use gitraptor_api::untrusted::sanitize;
use gitraptor_core::autostart::{Autostart, AutostartError};

use crate::i18n::t;

fn autostart(command: &str) -> Result<Autostart, ExitCode> {
    Autostart::for_current_user().ok_or_else(|| {
        eprintln!("{command}: {}", t("daemon.autostart.unsupported", &[]));
        ExitCode::FAILURE
    })
}

fn failed(command: &str, err: &AutostartError) -> ExitCode {
    let text = match err {
        AutostartError::Unsupported => t("daemon.autostart.unsupported", &[]),
        AutostartError::TransientBinary(path) => t(
            "daemon.enable.not-installed",
            &[("path", &sanitize(&path.display().to_string()))],
        ),
        AutostartError::ManagerUnavailable => t("daemon.enable.no-manager", &[]),
        AutostartError::Io(err) => t(
            "daemon.autostart.failed",
            &[("error", &sanitize(&err.to_string()))],
        ),
    };
    eprintln!("{command}: {text}");
    ExitCode::FAILURE
}

/// `raptor daemon enable`: registers the autostart for this very binary.
pub fn enable() -> ExitCode {
    const CMD: &str = "raptor daemon enable";
    let autostart = match autostart(CMD) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => return failed(CMD, &AutostartError::Io(err)),
    };
    match autostart.enable(&exe) {
        Ok(enabled) => {
            let artifact = sanitize(&enabled.artifact);
            let key = if enabled.changed {
                "daemon.enable.done"
            } else {
                "daemon.enable.already"
            };
            println!("{}", t(key, &[("artifact", &artifact)]));
            let when = if enabled.active_now {
                "daemon.enable.active"
            } else {
                "daemon.enable.next-login"
            };
            println!("{}", t(when, &[]));
            ExitCode::SUCCESS
        }
        Err(err) => failed(CMD, &err),
    }
}

/// `raptor daemon disable`: removes exactly what `enable` created.
pub fn disable() -> ExitCode {
    const CMD: &str = "raptor daemon disable";
    let autostart = match autostart(CMD) {
        Ok(a) => a,
        Err(code) => return code,
    };
    match autostart.disable() {
        Ok(true) => {
            println!("{}", t("daemon.disable.done", &[]));
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("{}", t("daemon.disable.already", &[]));
            ExitCode::SUCCESS
        }
        Err(err) => failed(CMD, &err),
    }
}
