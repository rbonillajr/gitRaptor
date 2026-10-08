//! The input thread (ADR-CKP-003 § 3): keys, paste and resize from
//! crossterm into the input queue. It polls with a short timeout so it can
//! stop. On Unix crossterm turns SIGWINCH into a resize event.
//!
//! It can be paused: before the terminal is handed to someone else (`Ctrl-Z`, the editor),
//! the main thread pauses it and waits for its confirmation, so it does not steal their keys
//! (ADR-CKP-003 § 9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event};

use crate::model::{Msg, Size};
use crate::queue::Outlet;

const POLL: Duration = Duration::from_millis(50);

/// Longest wait for the input thread to confirm a pause: a few polls. A thread that already
/// ended never confirms, and the pause must not hang the TUI.
const PAUSE_WAIT: Duration = Duration::from_millis(500);

pub struct InputThread {
    stop: Arc<AtomicBool>,
    pause: Pause,
    handle: Option<JoinHandle<()>>,
}

impl InputThread {
    pub fn spawn(out: Outlet) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Pause::default();
        let flag = Arc::clone(&stop);
        let gate = pause.clone();
        let handle = std::thread::Builder::new()
            .name("raptor-input".into())
            .spawn(move || {
                read(&flag, &gate, &out);
                gate.end();
            })
            .ok();
        Self {
            stop,
            pause,
            handle,
        }
    }

    /// Pauses and resumes the reading.
    pub fn pause(&self) -> Pause {
        self.pause.clone()
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.pause.resume();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
struct Gate {
    /// The main thread wants the reading paused.
    wanted: bool,
    /// The input thread is parked and reads nothing.
    parked: bool,
    /// The input thread ended: it reads nothing and confirms nothing.
    ended: bool,
}

/// The pause of the input thread, shared with the main thread.
#[derive(Clone, Default)]
pub struct Pause(Arc<(Mutex<Gate>, Condvar)>);

impl Pause {
    /// Asks the input thread to stop reading and waits until it confirms (bounded by
    /// [`PAUSE_WAIT`]). Returns whether it confirmed.
    pub fn pause(&self) -> bool {
        let (gate, signal) = &*self.0;
        let mut g = gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.wanted = true;
        signal.notify_all();
        let deadline = Instant::now() + PAUSE_WAIT;
        while !g.parked {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            g = signal
                .wait_timeout(g, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        true
    }

    pub fn resume(&self) {
        let (gate, signal) = &*self.0;
        let mut g = gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.wanted = false;
        signal.notify_all();
    }

    /// The input thread's side: parks while a pause is wanted.
    fn park(&self, stop: &AtomicBool) {
        let (gate, signal) = &*self.0;
        let mut g = gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !g.wanted {
            return;
        }
        g.parked = true;
        signal.notify_all();
        while g.wanted && !stop.load(Ordering::Relaxed) {
            g = signal
                .wait(g)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        g.parked = false;
    }

    /// The input thread's side: it stopped reading for good.
    fn end(&self) {
        let (gate, signal) = &*self.0;
        gate.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ended = true;
        signal.notify_all();
    }
}

fn read(stop: &AtomicBool, pause: &Pause, out: &Outlet) {
    while !stop.load(Ordering::Relaxed) {
        pause.park(stop);
        match event::poll(POLL) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(_) => return lost(out),
        }
        let msg = match event::read() {
            Ok(Event::Key(key)) => Msg::Key(key),
            Ok(Event::Paste(text)) => Msg::Paste(text),
            Ok(Event::Resize(width, height)) => Msg::Resize(Size { width, height }),
            Ok(_) => continue,
            Err(_) => return lost(out),
        };
        if out.send(msg).is_err() {
            return;
        }
    }
}

/// The terminal cannot be read: tell the loop, so the cockpit leaves with a reason instead of
/// staying on screen deaf to every key.
fn lost(out: &Outlet) {
    let _ = out.send(Msg::InputLost);
}
