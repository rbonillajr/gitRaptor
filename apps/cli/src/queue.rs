//! The two queues of the main thread (ADR-CKP-003 § 3): input and engine.
//!
//! `std::sync::mpsc` cannot wait on two receivers, so every send also rings
//! a shared wake-up queue the main thread waits on. The input queue is
//! always drained first.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::model::Msg;

/// A sender into one of the queues. Sending fails only when the main
/// thread is gone.
#[derive(Debug, Clone)]
pub struct Outlet {
    tx: Sender<Msg>,
    wake: Sender<()>,
}

impl Outlet {
    pub fn send(&self, msg: Msg) -> Result<(), Closed> {
        self.tx.send(msg).map_err(|_| Closed)?;
        // A full stop of the receiver is seen on the next send.
        let _ = self.wake.send(());
        Ok(())
    }
}

/// The main thread is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

/// The receiving side, owned by the main thread.
#[derive(Debug)]
pub struct Inbox {
    input: Receiver<Msg>,
    engine: Receiver<Msg>,
    wake: Receiver<()>,
}

/// Builds the inbox and its two outlets: (inbox, input, engine).
pub fn inbox() -> (Inbox, Outlet, Outlet) {
    let (input_tx, input) = mpsc::channel();
    let (engine_tx, engine) = mpsc::channel();
    let (wake_tx, wake) = mpsc::channel();
    (
        Inbox {
            input,
            engine,
            wake,
        },
        Outlet {
            tx: input_tx,
            wake: wake_tx.clone(),
        },
        Outlet {
            tx: engine_tx,
            wake: wake_tx,
        },
    )
}

impl Inbox {
    /// Waits until something arrives or `timeout` passes. Returns at once
    /// when a queue already holds messages.
    pub fn wait(&self, timeout: Duration) {
        match self.wake.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
        while self.wake.try_recv().is_ok() {}
    }

    pub fn next_input(&self) -> Option<Msg> {
        self.input.try_recv().ok()
    }

    pub fn next_engine(&self) -> Option<Msg> {
        self.engine.try_recv().ok()
    }
}
