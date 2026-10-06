//! The input thread (ADR-CKP-003 § 3): keys, paste and resize from
//! crossterm into the input queue. It polls with a short timeout so it can
//! stop. On Unix crossterm turns SIGWINCH into a resize event.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event};

use crate::model::{Msg, Size};
use crate::queue::Outlet;

const POLL: Duration = Duration::from_millis(50);

pub struct InputThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl InputThread {
    pub fn spawn(out: Outlet) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("raptor-input".into())
            .spawn(move || read(&flag, &out))
            .ok();
        Self { stop, handle }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read(stop: &AtomicBool, out: &Outlet) {
    while !stop.load(Ordering::Relaxed) {
        match event::poll(POLL) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(_) => return,
        }
        let msg = match event::read() {
            Ok(Event::Key(key)) => Msg::Key(key),
            Ok(Event::Paste(text)) => Msg::Paste(text),
            Ok(Event::Resize(width, height)) => Msg::Resize(Size { width, height }),
            Ok(_) => continue,
            Err(_) => return,
        };
        if out.send(msg).is_err() {
            return;
        }
    }
}
