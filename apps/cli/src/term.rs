//! What the TUI asks the terminal before it starts (ADR-CKP-003 § 10 and § 12): its background,
//! to pick the light or dark values of the palette (TS-CKP-004, 2026-10-05 amendment).
//!
//! `crates/theme` decides (precedence, bytes, parsing); this module only does the I/O, so the
//! termios state has a single owner. It must run **before** the TUI's event reader, or the reply
//! would be read as key presses.

// Wired by the TUI start-up (INF-CKP-001); until then only the pty tests use it, and they do
// not call `query`, which would touch the real terminal of whoever runs `cargo test`.
#![allow(dead_code)]

use std::time::Duration;

use gitraptor_theme::{Detection, QUERY_TIMEOUT, Rgb, ThemeChoice, parse_osc11_reply, resolve};

/// The theme to draw with: `flag` is `--theme`, `no_color` is `--no-color`; the rest comes from
/// the environment and, when nothing is explicit, from the terminal.
pub fn detect_theme(flag: Option<ThemeChoice>, no_color: bool) -> Detection {
    resolve(
        flag,
        no_color,
        |name| std::env::var(name).ok(),
        || query_background(QUERY_TIMEOUT),
    )
}

/// The terminal's background color, asked with OSC 11 on `/dev/tty`. `None` when stdin or stdout
/// is not a terminal, when the terminal does not answer within `timeout`, and on Windows
/// (Pendiente: etapa de validación multiplataforma).
pub fn query_background(timeout: Duration) -> Option<Rgb> {
    #[cfg(unix)]
    {
        unix::query(timeout).and_then(|reply| parse_osc11_reply(&reply))
    }
    #[cfg(not(unix))]
    {
        let _ = timeout;
        None
    }
}

#[cfg(unix)]
mod unix {
    use std::fs::{File, OpenOptions};
    use std::io::{IsTerminal, Read, Write};
    use std::time::{Duration, Instant};

    use gitraptor_theme::{OSC11_QUERY, QUERY_MAX_REPLY, reply_complete};
    use rustix::termios::{
        LocalModes, OptionalActions, QueueSelector, SpecialCodeIndex, Termios, tcflush, tcgetattr,
        tcsetattr,
    };

    pub(super) fn query(timeout: Duration) -> Option<Vec<u8>> {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return None;
        }
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        query_on(&tty, timeout)
    }

    /// Puts the saved mode back, whatever happens in between.
    struct Restore<'a> {
        tty: &'a File,
        saved: Termios,
    }

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            // Discard what already arrived and was not read, so it never reaches the TUI. A reply
            // later than the timeout still can: the TUI's reader ignores unknown sequences.
            let _ = tcflush(self.tty, QueueSelector::IFlush);
            let _ = tcsetattr(self.tty, OptionalActions::Now, &self.saved);
        }
    }

    /// Sends the request on `tty` and collects the reply until it is complete, `timeout` passes
    /// or [`QUERY_MAX_REPLY`] bytes arrive. `None` if `tty` is not a terminal.
    pub(super) fn query_on(tty: &File, timeout: Duration) -> Option<Vec<u8>> {
        let saved = tcgetattr(tty).ok()?;
        let mut quiet = saved.clone();
        // No line buffering and no echo; signals (ISIG) stay, so Ctrl-C still works.
        quiet
            .local_modes
            .remove(LocalModes::ICANON | LocalModes::ECHO);
        // Reads return after 0.1 s without data (VMIN = 0, VTIME = 1). Unlike poll(2), this works
        // on /dev/tty in macOS too.
        quiet.special_codes[SpecialCodeIndex::VMIN] = 0;
        quiet.special_codes[SpecialCodeIndex::VTIME] = 1;
        tcsetattr(tty, OptionalActions::Now, &quiet).ok()?;
        let _restore = Restore { tty, saved };

        let mut writer = tty;
        writer.write_all(OSC11_QUERY).ok()?;
        writer.flush().ok()?;
        let deadline = Instant::now() + timeout;
        let mut reply = Vec::new();
        let mut chunk = [0u8; 128];
        let mut reader = tty;
        while Instant::now() < deadline && !reply_complete(&reply) && reply.len() < QUERY_MAX_REPLY
        {
            match reader.read(&mut chunk) {
                Ok(n) => reply.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        Some(reply)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::fd::OwnedFd;
        use std::thread;

        use gitraptor_theme::{Rgb, parse_osc11_reply};
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

        /// A pseudo-terminal: `(controller, terminal)`. The test plays the terminal emulator on
        /// the controller side; the code under test talks to the other end as to `/dev/tty`.
        fn pty() -> (File, File) {
            let controller: OwnedFd =
                openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).expect("openpt");
            grantpt(&controller).expect("grantpt");
            unlockpt(&controller).expect("unlockpt");
            let name = ptsname(&controller, Vec::new()).expect("ptsname");
            let terminal = OpenOptions::new()
                .read(true)
                .write(true)
                .open(name.to_str().expect("utf-8 pty name"))
                .expect("open pty");
            (File::from(controller), terminal)
        }

        /// Reads the request on the controller, checks it and writes `answer` back.
        fn emulator(mut controller: File, answer: &'static [u8]) -> thread::JoinHandle<()> {
            thread::spawn(move || {
                let mut seen = Vec::new();
                let mut chunk = [0u8; 64];
                while !seen.ends_with(b"\x1b[c") {
                    let n = controller.read(&mut chunk).expect("read request");
                    assert!(n > 0, "request cut short: {seen:?}");
                    seen.extend_from_slice(&chunk[..n]);
                }
                assert_eq!(seen, OSC11_QUERY);
                controller.write_all(answer).expect("answer");
                // Keep the controller open until the query is done with it.
                thread::sleep(Duration::from_millis(300));
            })
        }

        #[test]
        fn a_terminal_that_answers_osc11_gives_its_background() {
            let (controller, terminal) = pty();
            let emu = emulator(controller, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b[?62;22c");
            let start = Instant::now();
            let reply = query_on(&terminal, Duration::from_secs(2)).expect("a terminal");
            // The DA1 reply ends the wait: nowhere near the timeout.
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "{:?}",
                start.elapsed()
            );
            assert_eq!(parse_osc11_reply(&reply), Some(Rgb(255, 255, 255)));
            emu.join().expect("emulator");
        }

        #[test]
        fn a_terminal_without_osc11_returns_at_once_with_nothing() {
            let (controller, terminal) = pty();
            let emu = emulator(controller, b"\x1b[?1;2c");
            let start = Instant::now();
            let reply = query_on(&terminal, Duration::from_secs(2)).expect("a terminal");
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "{:?}",
                start.elapsed()
            );
            assert_eq!(parse_osc11_reply(&reply), None);
            emu.join().expect("emulator");
        }

        #[test]
        fn a_silent_terminal_does_not_block_past_the_timeout_and_keeps_its_mode() {
            let (_controller, terminal) = pty();
            let before = tcgetattr(&terminal).expect("termios");
            let start = Instant::now();
            let reply = query_on(&terminal, Duration::from_millis(200)).expect("a terminal");
            let elapsed = start.elapsed();
            // One VTIME tick (0.1 s) of slack after the deadline.
            assert!(
                elapsed >= Duration::from_millis(200) && elapsed < Duration::from_millis(600),
                "{elapsed:?}"
            );
            assert_eq!(parse_osc11_reply(&reply), None);
            let after = tcgetattr(&terminal).expect("termios");
            // PENDIN is a status bit the kernel sets after the input flush.
            assert_eq!(
                after.local_modes - LocalModes::PENDIN,
                before.local_modes - LocalModes::PENDIN,
                "mode not restored"
            );
            for code in [SpecialCodeIndex::VMIN, SpecialCodeIndex::VTIME] {
                assert_eq!(after.special_codes[code], before.special_codes[code]);
            }
            assert!(
                after
                    .local_modes
                    .contains(LocalModes::ICANON | LocalModes::ECHO)
            );
        }

        #[test]
        fn a_file_that_is_not_a_terminal_is_not_queried() {
            let file = tempfile::tempfile().expect("temp file");
            let start = Instant::now();
            assert_eq!(query_on(&file, Duration::from_secs(2)), None);
            assert!(start.elapsed() < Duration::from_millis(100));
        }
    }
}
