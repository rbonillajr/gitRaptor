//! The loop of the main thread (ADR-CKP-003 § 3), the only owner of the
//! model and the terminal. Each iteration: (1) drain all the input queue;
//! (2) drain the engine queue until it is empty or the apply budget runs
//! out; (3) if the model is dirty, one `draw`; (4) stamp `t_render` on the
//! messages applied in that iteration. There is no fixed frame rate: what
//! arrives while painting is applied together in the next iteration.
//!
//! The backend is generic, so tests and the bench drive the same `App` on
//! `TestBackend` without a screen.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_api::clock::monotonic_ns;
use ratatui::Terminal;
use ratatui::backend::Backend;

use crate::client::LinkCmd;
use crate::model::{Cmd, Model, Msg};
use crate::queue::Inbox;
use crate::tui::metrics::Metrics;
use crate::tui::update::update;
use crate::tui::view::view;

/// Longest time the engine queue may hold the loop per iteration
/// (⚠️ ASSUMPTION of ADR-CKP-003 § 3; the bench of Validation V3 fixes it).
pub const APPLY_BUDGET: Duration = Duration::from_millis(8);

/// Period of [`Msg::Tick`]: relative ages and countdowns at 1 Hz.
pub const TICK: Duration = Duration::from_secs(1);

/// An engine message applied and not painted yet.
#[derive(Debug, Clone, Copy)]
struct Applied {
    recv_ns: u64,
    decoded_ns: u64,
    applied_ns: u64,
}

pub struct App<B: Backend> {
    terminal: Terminal<B>,
    pub model: Model,
    inbox: Inbox,
    link: Option<Sender<LinkCmd>>,
    pub metrics: Metrics,
    applied: Vec<Applied>,
    /// Keys read in this iteration, waiting for their frame.
    keys: Vec<u64>,
    /// The engine queue was left with messages: do not wait.
    backlog: bool,
    next_tick: Instant,
    /// Debug builds only: the view panics, to check that the terminal is
    /// restored (`GITRAPTOR_TUI_PANIC_IN_VIEW`).
    panic_in_view: bool,
}

impl<B: Backend> App<B> {
    pub fn new(terminal: Terminal<B>, model: Model, inbox: Inbox) -> Self {
        Self {
            terminal,
            model,
            inbox,
            link: None,
            metrics: Metrics::default(),
            applied: Vec::new(),
            keys: Vec::new(),
            backlog: false,
            next_tick: Instant::now() + TICK,
            panic_in_view: cfg!(debug_assertions)
                && std::env::var_os("GITRAPTOR_TUI_PANIC_IN_VIEW").is_some(),
        }
    }

    /// Where the commands for the channel go.
    pub fn attach(&mut self, link: Sender<LinkCmd>) {
        self.link = Some(link);
    }

    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// Runs until the model asks to quit.
    pub fn run(&mut self) -> Result<(), B::Error> {
        while self.step(TICK)? {}
        Ok(())
    }

    /// One iteration, waiting at most `wait` (and never past the next
    /// tick) when nothing is queued. Returns whether to go on.
    pub fn step(&mut self, wait: Duration) -> Result<bool, B::Error> {
        if !self.backlog {
            let until_tick = self.next_tick.saturating_duration_since(Instant::now());
            self.inbox.wait(wait.min(until_tick));
        }
        if Instant::now() >= self.next_tick {
            self.next_tick = Instant::now() + TICK;
            self.dispatch(Msg::Tick { now_ms: wall_ms() });
        }
        // (1) All the input first: a key never waits behind a burst.
        while let Some(msg) = self.inbox.next_input() {
            if matches!(msg, Msg::Key(_) | Msg::Paste(_)) {
                self.keys.push(monotonic_ns());
            }
            self.dispatch(msg);
        }
        // (2) The engine, within the apply budget.
        let start = Instant::now();
        self.backlog = false;
        while let Some(msg) = self.inbox.next_engine() {
            let stamps = match &msg {
                Msg::Engine(s) => Some((s.recv_ns, s.decoded_ns)),
                _ => None,
            };
            self.dispatch(msg);
            if let Some((recv_ns, decoded_ns)) = stamps {
                self.applied.push(Applied {
                    recv_ns,
                    decoded_ns,
                    applied_ns: monotonic_ns(),
                });
            }
            if start.elapsed() >= APPLY_BUDGET {
                self.backlog = true;
                break;
            }
        }
        // (3) One coalesced frame.
        if self.model.dirty {
            self.draw()?;
        }
        Ok(!self.model.ui.quit)
    }

    /// Paints now and stamps `t_render` (4).
    pub fn draw(&mut self) -> Result<(), B::Error> {
        let model = &self.model;
        let panic_in_view = self.panic_in_view;
        self.terminal.draw(|frame| {
            assert!(!panic_in_view, "GITRAPTOR_TUI_PANIC_IN_VIEW");
            view(model, frame);
        })?;
        let render_ns = monotonic_ns();
        self.model.dirty = false;
        self.metrics.frames += 1;
        for a in self.applied.drain(..) {
            self.metrics
                .record(a.recv_ns, a.decoded_ns, a.applied_ns, render_ns);
        }
        for key_ns in self.keys.drain(..) {
            self.metrics.key.record(render_ns.saturating_sub(key_ns));
        }
        Ok(())
    }

    /// `update`, then its commands outside it.
    fn dispatch(&mut self, msg: Msg) {
        for cmd in update(&mut self.model, msg) {
            let link_cmd = match cmd {
                Cmd::Resync { scope, resubscribe } => LinkCmd::Resync { scope, resubscribe },
                Cmd::Reconnect => LinkCmd::Reconnect,
                // The model already says quit; the loop ends after this iteration.
                Cmd::Quit => continue,
            };
            if let Some(link) = &self.link {
                let _ = link.send(link_cmd);
            }
        }
    }
}

/// The wall clock for the view, read here and never in `view`.
fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
